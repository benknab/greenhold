mod artifacts;
mod cli;
mod coin;
mod connection;
mod context;
mod discover;
mod guest;
mod output;
mod progress;
mod receipt;
mod verify;

#[cfg(test)]
mod test_support;

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use greenhold_core::{
    claim::Claim,
    eligibility::EligibleCoin,
    input::ClaimInput,
    verification::{check_script, verify_signature},
};
use output::{Report, Status};
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
            let coins = if let Some(endpoint) = options.electrum {
                discover::electrum_coins(&address, network, &endpoint)?
            } else {
                let rpc = options.connections.rpc_config()?;
                discover::coins(&address, network, &rpc)?
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&coins)?);
            } else {
                println!(
                    "{:<75} {:>16} {:>15} CREATION_HASH",
                    "OUTPOINT", "VALUE_SATS", "CREATION_HEIGHT"
                );
                for coin in coins {
                    println!("{coin}");
                }
            }
        }
        cli::Command::Claim(options) => prepare(&options, cli.json)?,
        cli::Command::Prove(options) => prove(options, cli.json)?,
        cli::Command::Verify(options) => {
            let receipt = receipt::read(&options.receipt)?;
            let rpc = options.connections.rpc_config()?;
            let claim = verify::claim(&receipt, options.network, options.nonce, &rpc)?;
            Report::new(&claim, Status::Verified).print(cli.json)?;
        }
        cli::Command::Inspect(options) => {
            let claim = guest::verify(&receipt::read(&options.receipt)?)?;
            eprintln!(
                "Proof checked. Chain state not checked: use verify to check the claim against the node."
            );
            Report::new(&claim, Status::Inspected).print(cli.json)?;
        }
    }
    Ok(())
}

fn prepare(options: &cli::ClaimOptions, json: bool) -> Result<()> {
    if options.output == options.witness {
        bail!("claim and witness must use different output files");
    }
    let rpc = options.context.connections.rpc_config()?;
    let (context, coin) = coin::fetch(
        options.outpoint,
        options.context.network,
        options.context.unspent_since,
        options.minimum_sats,
        &rpc,
    )?;
    check_script(&coin.witness().coin().output().script_pubkey)?;
    let address = bitcoin::Address::from_script(
        &coin.witness().coin().output().script_pubkey,
        bitcoin::Network::from(options.context.network),
    )?;
    let claim = Claim::new(&context, options.minimum_sats, options.nonce.hash());
    let witness = artifacts::encode_witness(coin.witness())?;
    fs::write(&options.witness, witness).context("cannot write witness")?;
    fs::write(&options.output, artifacts::encode_claim(&claim))
        .context("cannot write claim file")?;
    let mut report = Report::new(&claim, Status::Prepared);
    report.claim_file = Some(&options.output);
    report.witness = Some(&options.witness);
    report.address = Some(address);
    report.nonce_text = Some(options.nonce.text());
    report.print(json)?;
    Ok(())
}

fn prove(options: cli::ProveOptions, json: bool) -> Result<()> {
    let claim = artifacts::read_claim(&options.claim)?;
    if claim.network() != options.network {
        bail!("claim names {}, not {}", claim.network(), options.network);
    }
    let witness = artifacts::read_witness(&options.witness)?;
    // Early rejection uses the candidate script; guest membership authenticates it.
    progress::run("Checking signature", || {
        verify_signature(
            &witness.coin().output().script_pubkey,
            &claim.signing_message(),
            &options.signature,
        )
        .context("BIP-322 signature check failed")
    })?;
    let rpc = options.connections.rpc_config()?;
    let (context, coin) = progress::run("Checking chain state", || {
        let context = verify::context(&claim, &rpc)?;
        let coin = EligibleCoin::new(witness, &context, claim.minimum_sats())?;
        Ok((context, coin))
    })?;
    let input = ClaimInput::new(
        context,
        coin.witness().clone(),
        claim.minimum_sats(),
        claim.nonce(),
        options.signature,
    );
    // Execution is fast, so a rejected input fails before the slow proving step.
    let cycles = progress::run("Executing guest", || guest::execute(&input, &claim))?;
    let mut report = Report::new(&claim, Status::Executed);
    if let Some(path) = &options.output {
        let receipt = guest::prove(&input, &claim)?;
        let size = progress::run("Saving receipt", || {
            let bytes = receipt::encode(&receipt)?;
            fs::write(path, &bytes)
                .with_context(|| format!("cannot write receipt to {}", path.display()))?;
            Ok(bytes.len())
        })?;
        report = Report::new(&claim, Status::Proved);
        report.receipt = Some(path);
        report.receipt_bytes = Some(size);
    } else {
        // The cycle count depends on the private coin. Only a dry run, which makes
        // no receipt to share, reports it.
        report.cycles = Some(cycles);
    }
    report.print(json)?;
    Ok(())
}
