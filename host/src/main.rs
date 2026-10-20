mod cli;
mod coin;
mod connection;
mod context;
mod guest;
mod receipt;

#[cfg(test)]
mod test_support;

use anyhow::{Context as _, Result};
use bitcoin::hex::DisplayHex;
use clap::Parser;
use greenhold_core::{claim::Claim, input::ClaimInput};
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
        cli::Command::Prove(options) => {
            let input = context::Input::new(
                options.context.network.into(),
                options.context.first_height,
                options.context.last_height,
            )?;
            let rpc = options.context.connections.rpc_config()?;
            let (context, coin) = coin::fetch(options.outpoint, input, options.minimum_sats, &rpc)?;
            let claim = Claim::new(&context, options.minimum_sats);
            let input = ClaimInput::new(context, coin.witness().clone(), options.minimum_sats);
            // Execution is fast, so a rejected input fails before the slow proving step.
            let cycles = guest::execute(&input, &claim)?;
            let mut report = serde_json::json!({
                "claim": claim,
                "encoded": claim.to_bytes().as_hex().to_string(),
                "image_id": guest::image_id().to_string(),
            });
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
        }
        cli::Command::Context(options) => {
            let input = context::Input::new(
                options.network.into(),
                options.first_height,
                options.last_height,
            )?;
            let rpc = options.connections.rpc_config()?;
            let context = context::fetch(input, &rpc)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "network": context.network(),
                    "first_block_hash": context.first().hash(),
                    "last_block_hash": context.last().hash(),
                    "context": context,
                    "context_digest": context.digest(),
                }))?
            );
        }
    }
    Ok(())
}
