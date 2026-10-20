mod artifacts;
mod cli;
mod coin;
mod connection;
mod context;
mod discover;
mod guest;
mod receipt;
mod verify;

#[cfg(test)]
mod test_support;

use anyhow::{Context as _, Result, bail};
use bitcoin::hex::DisplayHex;
use clap::Parser;
use greenhold_core::{
    claim::Claim,
    eligibility::EligibleCoin,
    input::ClaimInput,
    verification::{check_script, verify_signature},
};
use std::{fs, process::ExitCode};

fn main() -> ExitCode {
    match run(cli::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: cli::Cli) -> Result<()> {
    match cli.command {
        cli::Command::Discover(options) => {
            let network = options.network.into();
            let address = options
                .address
                .require_network(network)
                .context("address does not match the requested network")?;
            let rpc = options.connections.rpc_config()?;
            for outpoint in discover::outpoints(&address, network, &rpc)? {
                println!("{outpoint}");
            }
        }
        cli::Command::Claim(options) => prepare(&options)?,
        cli::Command::Prove(options) => prove(options)?,
        cli::Command::Verify(options) => {
            let receipt = receipt::read(&options.receipt)?;
            let expected = artifacts::read_claim(&options.claim)?;
            let rpc = options.connections.rpc_config()?;
            let claim = verify::claim(&receipt, options.network, &expected, &rpc)?;
            println!("{}", serde_json::to_string_pretty(&report(&claim))?);
        }
        cli::Command::Inspect(options) => {
            let claim = guest::verify(&receipt::read(&options.receipt)?)?;
            eprintln!(
                "Proof checked. Expected claim and chain state not checked: use verify to accept the claim."
            );
            println!("{}", serde_json::to_string_pretty(&report(&claim))?);
        }
    }
    Ok(())
}

fn prepare(options: &cli::ClaimOptions) -> Result<()> {
    if options.output == options.witness {
        bail!("claim and witness output paths must differ");
    }
    let network = options.context.network.into();
    let input = context::Input::new(
        network,
        options.context.first_height,
        options.context.last_height,
    )?;
    let rpc = options.context.connections.rpc_config()?;
    let (context, coin) = coin::fetch(options.outpoint, input, options.minimum_sats, &rpc)?;
    check_script(&coin.witness().coin().output().script_pubkey)?;
    let claim = Claim::new(&context, options.minimum_sats);
    let witness = artifacts::encode_witness(coin.witness())?;
    fs::write(&options.witness, witness).context("cannot write witness")?;
    fs::write(&options.output, claim.to_bytes()).context("cannot write claim")?;
    println!("{}", serde_json::to_string_pretty(&report(&claim))?);
    Ok(())
}

fn prove(options: cli::ProveOptions) -> Result<()> {
    let claim = artifacts::read_claim(&options.claim)?;
    if claim.network() != options.network {
        bail!("claim names {}, not {}", claim.network(), options.network);
    }
    let witness = artifacts::read_witness(&options.witness)?;
    // Early rejection uses the candidate script; guest membership authenticates it.
    verify_signature(
        &witness.coin().output().script_pubkey,
        &claim.signing_message(),
        &options.signature,
    )
    .context("BIP-322 signature check failed")?;
    let rpc = options.connections.rpc_config()?;
    let context = verify::context(&claim, &rpc)?;
    let coin = EligibleCoin::new(witness, &context, claim.minimum_sats())?;
    let input = ClaimInput::new(
        context,
        coin.witness().clone(),
        claim.minimum_sats(),
        options.signature,
    );
    // Execution is fast, so a rejected input fails before the slow proving step.
    let cycles = guest::execute(&input, &claim)?;
    let mut report = report(&claim);
    if let Some(path) = options.output {
        let bytes = receipt::encode(&guest::prove(&input, &claim)?)?;
        fs::write(&path, &bytes)
            .with_context(|| format!("cannot write receipt to {}", path.display()))?;
        report["receipt"] = path.display().to_string().into();
        report["receipt_bytes"] = bytes.len().into();
    } else {
        // The cycle count depends on the private coin. Only a dry run, which makes
        // no receipt to share, reports it.
        report["cycles"] = cycles.into();
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// The public claim in JSON and canonical hex, and the guest that commits it.
fn report(claim: &Claim) -> serde_json::Value {
    serde_json::json!({
        "claim": claim,
        "encoded": claim.to_bytes().as_hex().to_string(),
        "message": claim.signing_message(),
        "image_id": guest::image_id().to_string(),
    })
}
