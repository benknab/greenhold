use crate::connection::{electrum, utreexod_rpc};
use anyhow::{Context, Result, bail};
use bitcoin::Network;
use clap::{Args, Parser, Subcommand};
use url::Url;

pub const DEFAULT_ELECTRUM: &str = "tls://electrum.hospitablealpaca.com:50002";
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
    /// Fetch headers and historical Utreexo state from the chosen network's nodes.
    Context(ContextOptions),
}

#[derive(Debug, Args)]
pub struct ContextOptions {
    /// Bitcoin network: mainnet, testnet, testnet4, signet, or regtest.
    #[arg(long, default_value = "mainnet", value_parser = parse_network)]
    pub network: Network,

    /// Fixed last block height T. State is fetched at this block.
    pub last_height: u32,

    /// First block height X. Defaults to T for a single-block context.
    #[arg(long, value_name = "HEIGHT")]
    pub first_height: Option<u32>,

    #[command(flatten)]
    pub connections: ConnectionConfig,
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
    pub electrum: electrum::Endpoint,

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
    pub fn rpc_config(&self) -> Result<utreexod_rpc::Config> {
        match &self.utreexod_rpc {
            None => Ok(utreexod_rpc::Config::new(rpc_url(DEFAULT_RPC)?, None)),
            Some(url) => {
                let user = self
                    .utreexod_rpc_user
                    .as_ref()
                    .context("--utreexod-rpc requires --utreexod-rpc-user")?;
                let password = self
                    .utreexod_rpc_password
                    .as_ref()
                    .context("--utreexod-rpc requires --utreexod-rpc-password")?;
                Ok(utreexod_rpc::Config::new(
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

fn parse_network(text: &str) -> Result<Network> {
    if text == "mainnet" {
        return Ok(Network::Bitcoin);
    }
    text.parse().context("invalid Bitcoin network")
}

fn rpc_user(text: &str) -> Result<String> {
    if text.is_empty() || text.contains(':') {
        bail!("RPC username must be nonempty and must not contain ':'");
    }
    Ok(text.to_owned())
}

fn rpc_password(text: &str) -> Result<Redacted> {
    if text.is_empty() {
        bail!("RPC password must be nonempty");
    }
    Ok(Redacted::new(text.to_owned()))
}

fn rpc_url(text: &str) -> Result<Url> {
    let url = Url::parse(text).context("invalid RPC URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("RPC URL must use HTTP(S) without credentials, query, or fragment");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_context<const N: usize>(args: [&str; N]) -> Result<ContextOptions> {
        let Command::Context(options) = Cli::try_parse_from(args)?.command;
        Ok(options)
    }

    #[test]
    fn context_network_selection() -> Result<()> {
        for (name, network) in [
            ("mainnet", Network::Bitcoin),
            ("testnet", Network::Testnet),
            ("testnet4", Network::Testnet4),
            ("signet", Network::Signet),
            ("regtest", Network::Regtest),
        ] {
            let options = parse_context(["host", "context", "100", "--network", name])?;
            assert_eq!(options.network, network);
        }
        let options = parse_context(["host", "context", "100"])?;
        assert_eq!(options.network, Network::Bitcoin);
        assert!(Cli::try_parse_from(["host", "context", "100", "--network", "unknown"]).is_err());
        Ok(())
    }

    #[test]
    fn defaults_and_overrides() -> Result<()> {
        let options = parse_context(["host", "context", "100"])?;
        assert_eq!(
            options.connections.electrum.socket_url().url().host_str(),
            Some("electrum.hospitablealpaca.com")
        );
        assert_eq!(
            options.connections.electrum.socket_url().url().port(),
            Some(50002)
        );
        assert_eq!(
            options.connections.electrum.socket_url().url().scheme(),
            "tls"
        );
        assert!(options.connections.utreexod_rpc.is_none());
        assert!(options.connections.rpc_config().is_ok());
        let options = parse_context([
            "host",
            "context",
            "100",
            "--electrum",
            "tls://[::1]:50002",
            "--utreexod-rpc",
            "http://localhost:8334/",
            "--utreexod-rpc-user",
            "user",
            "--utreexod-rpc-password",
            "secret-password",
        ])?;
        assert_eq!(
            options.connections.electrum.socket_url().url().host(),
            Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        );
        assert_eq!(
            options.connections.utreexod_rpc.as_ref().map(Url::as_str),
            Some("http://localhost:8334/")
        );
        assert!(options.connections.rpc_config().is_ok());
        assert!(!format!("{options:?}").contains("secret-password"));
        Ok(())
    }

    #[test]
    fn custom_rpc_requires_both_auth_flags_and_auth_requires_url() -> Result<()> {
        for args in [
            vec![
                "host",
                "context",
                "100",
                "--utreexod-rpc",
                "http://localhost/",
            ],
            vec![
                "host",
                "context",
                "100",
                "--utreexod-rpc-user",
                "user",
                "--utreexod-rpc-password",
                "pass",
            ],
            vec![
                "host",
                "context",
                "100",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-user",
                "user",
            ],
            vec![
                "host",
                "context",
                "100",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-password",
                "pass",
            ],
        ] {
            let error = Cli::try_parse_from(args)
                .err()
                .context("incomplete RPC arguments must fail during parsing")?;
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
                "context",
                "100",
                "--utreexod-rpc",
                "http://localhost/",
                "--utreexod-rpc-user",
                user,
                "--utreexod-rpc-password",
                password,
            ])
            .err()
            .context("invalid RPC credentials must fail during parsing")?;
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
            let config = parse_context(["host", "context", "100", "--electrum", text])?;
            assert_eq!(
                config.connections.electrum.socket_url().url().scheme(),
                scheme
            );
        }
        for text in [
            "localhost:50002",
            "[::1]:50002",
            "https://localhost:50002",
            "tcp://localhost:0",
            "ssl://localhost",
            "tcp://ssl://localhost:50002",
        ] {
            assert!(Cli::try_parse_from(["host", "context", "100", "--electrum", text]).is_err());
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
            assert!(
                endpoint.parse::<electrum::Endpoint>().is_err(),
                "{endpoint}"
            );
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
