use crate::{
    Result,
    connection::{electrum::ElectrumEndpoint, rpc::RpcConfig, validate_socket_url},
};
use clap::{Args, Parser, Subcommand};
use url::Url;

pub const DEFAULT_ELECTRUM: &str = "tls://electrum.hospitablealpaca.com:50002";
pub const DEFAULT_UTREEXOD: &str = "tcp://utreexo.hospitablealpaca.com:8335";
pub const DEFAULT_RPC: &str = "https://utreexo.hospitablealpaca.com/";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Bitcoin proof-of-funds tools",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read live node status over Electrum, Utreexod P2P, and Utreexod RPC.
    Nodes(ConnectionConfig),
}

#[derive(Debug, Args)]
pub struct ConnectionConfig {
    /// Electrum server: tcp://HOST:PORT for plaintext, tls://HOST:PORT for TLS.
    /// ssl:// is an alias for the same TLS connection.
    #[arg(
        long,
        value_name = "ENDPOINT",
        default_value = DEFAULT_ELECTRUM
    )]
    pub electrum: ElectrumEndpoint,

    /// Utreexod mainnet Bitcoin v1 P2P endpoint: tcp://HOST:PORT.
    #[arg(
        long,
        value_name = "ENDPOINT",
        value_parser = p2p_endpoint,
        default_value = DEFAULT_UTREEXOD
    )]
    pub utreexod: Url,

    /// Custom HTTP(S) RPC URL. Requires both Utreexod RPC auth flags.
    /// If omitted, use the anonymous public HTTPS guard on port 443.
    #[arg(long, value_name = "URL", value_parser = rpc_url,
        requires_all = ["utreexod_rpc_user", "utreexod_rpc_password"])]
    pub utreexod_rpc: Option<Url>,

    /// RPC username. Required with --utreexod-rpc.
    #[arg(long, value_name = "USER", requires = "utreexod_rpc", value_parser = rpc_user)]
    pub utreexod_rpc_user: Option<String>,

    /// RPC password. Required with --utreexod-rpc.
    #[arg(long, value_name = "PASSWORD", requires = "utreexod_rpc", value_parser = rpc_password)]
    pub utreexod_rpc_password: Option<Redacted>,
}

impl ConnectionConfig {
    pub fn rpc_config(&self) -> Result<RpcConfig> {
        match &self.utreexod_rpc {
            None => Ok(RpcConfig::new(rpc_url(DEFAULT_RPC)?, None)),
            Some(url) => {
                let user = self
                    .utreexod_rpc_user
                    .as_ref()
                    .ok_or("--utreexod-rpc requires --utreexod-rpc-user")?;
                let password = self
                    .utreexod_rpc_password
                    .as_ref()
                    .ok_or("--utreexod-rpc requires --utreexod-rpc-password")?;
                Ok(RpcConfig::new(
                    url.clone(),
                    Some((user.clone(), password.value().to_owned())),
                ))
            }
        }
    }
}

#[derive(Clone)]
pub struct Redacted {
    value: String,
}

impl Redacted {
    pub fn new(value: String) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

fn p2p_endpoint(text: &str) -> std::result::Result<Url, &'static str> {
    let url = Url::parse(text).map_err(|_| "invalid endpoint URL")?;
    validate_socket_url(&url)?;
    if url.scheme() != "tcp" {
        return Err("P2P endpoint must use tcp://");
    }
    Ok(url)
}

fn rpc_user(text: &str) -> std::result::Result<String, &'static str> {
    if text.is_empty() || text.contains(':') {
        return Err("RPC username must be nonempty and must not contain ':'");
    }
    Ok(text.to_owned())
}

fn rpc_password(text: &str) -> std::result::Result<Redacted, &'static str> {
    if text.is_empty() {
        return Err("RPC password must be nonempty");
    }
    Ok(Redacted::new(text.to_owned()))
}

fn rpc_url(text: &str) -> std::result::Result<Url, &'static str> {
    let url = Url::parse(text).map_err(|_| "invalid RPC URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("RPC URL must use HTTP(S) without credentials, query, or fragment");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() -> Result<()> {
        let Command::Nodes(nodes) = Cli::try_parse_from(["host", "nodes"])?.command;
        assert_eq!(
            nodes.electrum.url().host_str(),
            Some("electrum.hospitablealpaca.com")
        );
        assert_eq!(nodes.electrum.url().port(), Some(50002));
        assert_eq!(nodes.electrum.url().scheme(), "tls");
        assert_eq!(nodes.utreexod.port(), Some(8335));
        assert!(nodes.utreexod_rpc.is_none());
        assert!(nodes.rpc_config().is_ok());
        let Command::Nodes(nodes) = Cli::try_parse_from([
            "host",
            "nodes",
            "--electrum",
            "tls://[::1]:50002",
            "--utreexod",
            "tcp://localhost:8335",
            "--utreexod-rpc",
            "http://localhost:8334/",
            "--utreexod-rpc-user",
            "user",
            "--utreexod-rpc-password",
            "secret-password",
        ])?
        .command;
        assert_eq!(
            nodes.electrum.url().host(),
            Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        );
        assert_eq!(nodes.utreexod.host_str(), Some("localhost"));
        assert_eq!(
            nodes.utreexod_rpc.as_ref().map(Url::as_str),
            Some("http://localhost:8334/")
        );
        assert!(nodes.rpc_config().is_ok());
        assert!(!format!("{nodes:?}").contains("secret-password"));
        Ok(())
    }

    #[test]
    fn custom_rpc_requires_both_auth_flags_and_auth_requires_url() -> Result<()> {
        for args in [
            vec!["host", "nodes", "--utreexod-rpc", "http://localhost/"],
            vec![
                "host",
                "nodes",
                "--utreexod-rpc-user",
                "user",
                "--utreexod-rpc-password",
                "pass",
            ],
            vec![
                "host",
                "nodes",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-user",
                "user",
            ],
            vec![
                "host",
                "nodes",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-password",
                "pass",
            ],
        ] {
            let error = Cli::try_parse_from(args)
                .err()
                .ok_or("incomplete RPC arguments must fail during parsing")?;
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::MissingRequiredArgument
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_rpc_credentials_during_cli_parsing() -> Result<()> {
        for (user, password) in [("", "pass"), ("user", ""), ("user:name", "pass")] {
            let error = Cli::try_parse_from([
                "host",
                "nodes",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-user",
                user,
                "--utreexod-rpc-password",
                password,
            ])
            .err()
            .ok_or("invalid RPC credentials must fail during parsing")?;
            assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        }
        Ok(())
    }

    #[test]
    fn electrum_transport_selection() -> Result<()> {
        for (text, scheme) in [
            ("tcp://[::1]:50001", "tcp"),
            ("tls://localhost:50002", "tls"),
            ("ssl://localhost:50002", "ssl"),
        ] {
            let Command::Nodes(config) =
                Cli::try_parse_from(["host", "nodes", "--electrum", text])?.command;
            assert_eq!(config.electrum.url().scheme(), scheme);
        }
        for text in [
            "localhost:50002",
            "[::1]:50002",
            "https://localhost:50002",
            "tcp://localhost:0",
            "ssl://localhost",
            "tcp://ssl://localhost:50002",
        ] {
            assert!(Cli::try_parse_from(["host", "nodes", "--electrum", text]).is_err());
        }
        Ok(())
    }

    #[test]
    fn p2p_requires_explicit_tcp() -> Result<()> {
        let Command::Nodes(nodes) =
            Cli::try_parse_from(["host", "nodes", "--utreexod", "tcp://[::1]:8335"])?.command;
        assert_eq!(
            nodes.utreexod.host(),
            Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        );
        assert_eq!(nodes.utreexod.port(), Some(8335));
        for endpoint in [
            "localhost:8335",
            "[::1]:8335",
            "ssl://localhost:8335",
            "https://localhost:8335",
            "tcp://localhost:0",
            "tcp://localhost",
            "tcp://tcp://localhost:8335",
        ] {
            assert!(Cli::try_parse_from(["host", "nodes", "--utreexod", endpoint]).is_err());
        }
        Ok(())
    }

    #[test]
    fn reject_invalid_endpoints() {
        for endpoint in [
            "tcp://host:0",
            "tcp://host:65536",
            "tcp://::1:50002",
            "https://host:443",
            "tcp://user:password@host:123",
            "tcp://user@host:123",
            "tcp://[bad]:123",
            "tcp://host:123/path",
            "tcp://host:123?query",
            "tcp://host:123#fragment",
            "tcp:host:123",
        ] {
            assert!(endpoint.parse::<ElectrumEndpoint>().is_err(), "{endpoint}");
            assert!(p2p_endpoint(endpoint).is_err(), "{endpoint}");
        }
        for url in [
            "ftp://host/",
            "http://user:password@host/",
            "http://host/?secret",
            "http://host/#fragment",
        ] {
            assert!(rpc_url(url).is_err());
        }
    }
}
