use crate::connection::utreexod_rpc;
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use greenhold_core::{eligibility::MinimumAmount, network::SupportedNetwork};
use url::Url;

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
    /// Check one coin's eligibility and print the public claim (node proofs are tip-only).
    Claim(ClaimOptions),
    /// Test tool: fetch headers and historical Utreexo state. Replaced by verify.
    #[command(hide = true)]
    Context(ContextOptions),
}

#[derive(Debug, Args)]
pub struct ClaimOptions {
    /// Candidate output as TXID:VOUT (zero-based output index).
    pub outpoint: bitcoin::OutPoint,

    /// Minimum coin value M in satoshis, up to 2100000000000000.
    #[arg(long, value_name = "SATS", default_value = "1")]
    pub minimum_sats: MinimumAmount,

    #[command(flatten)]
    pub context: ContextOptions,
}

#[derive(Debug, Args)]
pub struct ContextOptions {
    /// Bitcoin network: bitcoin, signet, or regtest.
    #[arg(long, default_value = "bitcoin")]
    pub network: SupportedNetwork,

    /// Last block height T. Defaults to the RPC tip (the proof's block for claim).
    pub last_height: Option<u32>,

    /// First block height X. Defaults to T for a single-block context.
    #[arg(long, value_name = "HEIGHT")]
    pub first_height: Option<u32>,

    #[command(flatten)]
    pub connections: ConnectionConfig,
}

#[derive(Debug, Args)]
#[expect(
    clippy::struct_field_names,
    reason = "field names match the public RPC flags and Clap argument dependencies"
)]
pub struct ConnectionConfig {
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
mod tests;
