use crate::connection::{electrum, utreexod_rpc};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use greenhold_core::{
    claim::Nonce, eligibility::MinimumAmount, network::SupportedNetwork, verification::Signature,
};
use std::path::PathBuf;
use url::Url;

pub const DEFAULT_RPC: &str = "https://utreexo.hospitablealpaca.com/";
pub const DEFAULT_ELECTRUM: &str = "tls://electrum.hospitablealpaca.com:50002";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Bitcoin proof-of-funds tools",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Print structured JSON instead of human-readable output.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List confirmed coins with outpoint, value in sats, creation height, and creation block hash.
    Discover(DiscoverOptions),
    /// Prepare a claim and private coin witness to sign (node proofs are tip-only).
    Claim(ClaimOptions),
    /// Prove a prepared claim using its saved witness and signature.
    Prove(ProveOptions),
    /// Verify a receipt and its claim against the node's chain state.
    Verify(VerifyOptions),
    /// Check a receipt's proof and print its claim, without node access or chain checks.
    Inspect(InspectOptions),
}

#[derive(Debug, Args)]
pub struct DiscoverOptions {
    /// Address whose current coins should be listed.
    #[arg(long)]
    pub address: bitcoin::Address<bitcoin::address::NetworkUnchecked>,

    /// Bitcoin network: bitcoin, signet, or regtest.
    #[arg(long, default_value = "bitcoin")]
    pub network: SupportedNetwork,

    /// Discover through Electrum (the default).
    /// Without URL, use <tls://electrum.hospitablealpaca.com:50002>.
    /// Electrum excludes unconfirmed outputs and coins spent by pending transactions.
    #[arg(long, value_name = "URL", num_args = 0..=1,
        default_value = DEFAULT_ELECTRUM, default_missing_value = DEFAULT_ELECTRUM,
        default_value_if("utreexod", "true", None),
        default_value_if("utreexod_rpc", clap::builder::ArgPredicate::IsPresent, None),
        conflicts_with_all = ["utreexod", "utreexod_rpc", "utreexod_rpc_user", "utreexod_rpc_password"])]
    pub electrum: Option<electrum::Endpoint>,

    /// Discover through Utreexod.
    /// Use <https://utreexo.hospitablealpaca.com/> unless custom RPC settings are supplied.
    #[arg(long)]
    pub utreexod: bool,

    #[command(flatten)]
    pub connections: ConnectionConfig,
}

#[derive(Debug, Args)]
pub struct VerifyOptions {
    /// Receipt file to verify.
    #[arg(long, value_name = "FILE")]
    pub receipt: PathBuf,

    /// Optional independently expected claim file. Requires an exact match.
    #[arg(long, value_name = "FILE")]
    pub claim: Option<PathBuf>,

    /// Bitcoin network the claim must name: bitcoin, signet, or regtest.
    #[arg(long, default_value = "bitcoin")]
    pub network: SupportedNetwork,

    #[command(flatten)]
    pub connections: ConnectionConfig,
}

#[derive(Debug, Args)]
pub struct InspectOptions {
    /// Receipt file to inspect.
    #[arg(long, value_name = "FILE")]
    pub receipt: PathBuf,
}

#[derive(Debug, Args)]
pub struct ProveOptions {
    /// Public claim file written by claim --output.
    #[arg(long, value_name = "FILE")]
    pub claim: PathBuf,

    /// Private coin witness captured with the claim.
    #[arg(long, value_name = "FILE")]
    pub witness: PathBuf,

    /// BIP-322 signature over the claim's exact signing message.
    #[arg(long)]
    pub signature: Signature,

    /// Receipt file to write. Replaces an existing file.
    #[arg(long, value_name = "FILE", required_unless_present = "dry_run")]
    pub output: Option<PathBuf>,

    /// Run the guest without proving, and write no receipt.
    #[arg(long, conflicts_with = "output")]
    pub dry_run: bool,

    /// Bitcoin network the claim must name: bitcoin, signet, or regtest.
    #[arg(long, default_value = "bitcoin")]
    pub network: SupportedNetwork,

    #[command(flatten)]
    pub connections: ConnectionConfig,
}

#[derive(Debug, Args)]
pub struct ClaimOptions {
    /// Public nonce: 64 hex characters. Defaults to zero, without a fresh challenge.
    #[arg(long, value_name = "HEX", default_value_t = Nonce::default())]
    pub nonce: Nonce,

    /// Candidate output as TXID:VOUT (zero-based output index).
    #[arg(long)]
    pub outpoint: bitcoin::OutPoint,

    /// Minimum coin value M in satoshis, up to 2100000000000000.
    #[arg(long, value_name = "SATS", default_value = "1")]
    pub minimum_sats: MinimumAmount,

    /// Public claim file to write. Replaces an existing file.
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,

    /// Private coin witness file to write. Replaces an existing file.
    #[arg(long, value_name = "FILE")]
    pub witness: PathBuf,

    #[command(flatten)]
    pub context: ContextOptions,
}

#[derive(Debug, Args)]
pub struct ContextOptions {
    /// Bitcoin network: bitcoin, signet, or regtest.
    #[arg(long, default_value = "bitcoin")]
    pub network: SupportedNetwork,

    /// First block height X. Defaults to the tip proof's block height T.
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
