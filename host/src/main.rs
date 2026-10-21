mod artifacts;
mod cli;
mod coin;
mod connection;
mod context;
mod discover;
mod guest;
mod output;
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
            let expected = options
                .claim
                .as_deref()
                .map(artifacts::read_claim)
                .transpose()?;
            let rpc = options.connections.rpc_config()?;
            let claim = verify::claim(&receipt, options.network, expected.as_ref(), &rpc)?;
            let mut report = Report::new(&claim, Status::Verified);
            report.expected_claim_checked = Some(expected.is_some());
            report.print(cli.json)?;
        }
        cli::Command::Inspect(options) => {
            let claim = guest::verify(&receipt::read(&options.receipt)?)?;
            eprintln!(
                "Proof checked. Expected claim and chain state not checked: use verify to accept the claim."
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
    let input = context::Input::new(
        options.context.network.into(),
        options.context.first_height,
        options.context.last_height,
    )?;
    let rpc = options.context.connections.rpc_config()?;
    let (context, coin) = coin::fetch(options.outpoint, input, options.minimum_sats, &rpc)?;
    check_script(&coin.witness().coin().output().script_pubkey)?;
    let address = bitcoin::Address::from_script(
        &coin.witness().coin().output().script_pubkey,
        bitcoin::Network::from(options.context.network),
    )?;
    let claim = Claim::new(&context, options.minimum_sats);
    let witness = artifacts::encode_witness(coin.witness())?;
    fs::write(&options.witness, witness).context("cannot write witness")?;
    fs::write(&options.output, artifacts::encode_claim(&claim))
        .context("cannot write claim file")?;
    let mut report = Report::new(&claim, Status::Prepared);
    report.claim_file = Some(&options.output);
    report.witness = Some(&options.witness);
    report.address = Some(address);
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
    let mut report = Report::new(&claim, Status::Executed);
    if let Some(path) = &options.output {
        let bytes = receipt::encode(&guest::prove(&input, &claim)?)?;
        fs::write(path, &bytes)
            .with_context(|| format!("cannot write receipt to {}", path.display()))?;
        report = Report::new(&claim, Status::Proved);
        report.receipt = Some(path);
        report.receipt_bytes = Some(bytes.len());
    } else {
        // The cycle count depends on the private coin. Only a dry run, which makes
        // no receipt to share, reports it.
        report.cycles = Some(cycles);
    }
    report.print(json)?;
    Ok(())
}
