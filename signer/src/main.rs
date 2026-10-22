//! Command-line interface for the test signer.

use anyhow::{Context as _, Result, ensure};
use bitcoin::Network;
use bitcoin::bip32::DerivationPath;
use bitcoin::hex::FromHex;
use bitcoin::secp256k1::SecretKey;
use clap::{Args, Parser};
use greenhold_signer::{Format, ScriptType};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Derive test-wallet addresses and sign BIP-322 messages")]
enum Command {
    /// Derive a single-key address.
    Address(Wallet),
    /// Sign an exact UTF-8 message without time locks.
    Sign {
        #[command(flatten)]
        wallet: Wallet,
        #[arg(long)]
        message: String,
        /// Default: full for p2pkh/p2sh-p2wpkh, simple for p2wpkh/p2tr.
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("secret").required(true).args(["mnemonic_file", "seed_file"])))]
struct Wallet {
    #[arg(long = "type", value_enum)]
    script_type: ScriptType,
    #[arg(long, default_value = "bitcoin", value_parser = parse_network)]
    network: Network,
    #[arg(long, conflicts_with_all = ["account", "change", "index"])]
    path: Option<DerivationPath>,
    #[arg(long)]
    account: Option<u32>,
    #[arg(long)]
    change: Option<u32>,
    #[arg(long)]
    index: Option<u32>,
    /// File containing an English BIP-39 mnemonic.
    #[arg(long)]
    mnemonic_file: Option<PathBuf>,
    /// File containing the BIP-39 passphrase (one trailing newline is removed).
    #[arg(long, requires = "mnemonic_file", conflicts_with = "seed_file")]
    passphrase_file: Option<PathBuf>,
    /// File containing a hexadecimal BIP-32 seed (16–64 bytes).
    #[arg(long)]
    seed_file: Option<PathBuf>,
}

fn parse_network(text: &str) -> Result<Network> {
    match text {
        "bitcoin" => Ok(Network::Bitcoin),
        "signet" => Ok(Network::Signet),
        "regtest" => Ok(Network::Regtest),
        _ => anyhow::bail!("expected bitcoin, signet, or regtest"),
    }
}

fn read_secret(path: &PathBuf) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("cannot read secret file {}", path.display()))
}

impl Wallet {
    fn derive(&self) -> Result<(DerivationPath, SecretKey)> {
        let path = match &self.path {
            Some(path) => path.clone(),
            None => self.script_type.default_path(
                self.network,
                self.account.unwrap_or(0),
                self.change.unwrap_or(0),
                self.index.unwrap_or(0),
            )?,
        };
        let seed = if let Some(file) = &self.mnemonic_file {
            let passphrase = self.passphrase_file.as_ref().map(read_secret).transpose()?;
            let passphrase = passphrase.as_deref().unwrap_or("");
            let passphrase = passphrase
                .strip_suffix("\r\n")
                .or_else(|| passphrase.strip_suffix('\n'))
                .unwrap_or(passphrase);
            greenhold_signer::seed_from_mnemonic(&read_secret(file)?, passphrase)?.to_vec()
        } else {
            let file = self.seed_file.as_ref().context("missing seed file")?;
            Vec::<u8>::from_hex(read_secret(file)?.trim())?
        };
        let key = greenhold_signer::derive_key(&seed, &path)?;
        Ok((path, key))
    }
}

fn main() -> Result<()> {
    let (wallet, signing) = match Command::parse() {
        Command::Address(wallet) => (wallet, None),
        Command::Sign {
            wallet,
            message,
            format,
        } => {
            let format = format.unwrap_or(wallet.script_type.formats()[0]);
            ensure!(
                wallet.script_type.formats().contains(&format),
                "--format is not supported for this script type"
            );
            (wallet, Some((message, format)))
        }
    };
    // Validate the public request before reading any secret files.
    let (path, key) = wallet.derive()?;
    let mut output = if let Some((message, format)) = signing {
        let signed =
            greenhold_signer::sign(wallet.script_type, format, &key, wallet.network, &message)?;
        let mut output = serde_json::to_value(signed)?;
        output["format"] = json!(format);
        output
    } else {
        json!({"address": greenhold_signer::address(wallet.script_type, &key, wallet.network)})
    };
    output["type"] = json!(wallet.script_type);
    output["network"] = json!(wallet.network);
    output["path"] = json!(format!("m/{path}").trim_end_matches('/'));
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
