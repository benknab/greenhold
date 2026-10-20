mod cli;
mod coin;
mod connection;
mod context;
mod guest;

#[cfg(test)]
mod test_support;

use anyhow::Result;
use bitcoin::hex::DisplayHex;
use clap::Parser;
use greenhold_core::{claim::Claim, input::ClaimInput};
use std::process::ExitCode;

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
        cli::Command::Claim(options) => {
            let input = context::Input::new(
                options.context.network.into(),
                options.context.first_height,
                options.context.last_height,
            )?;
            let rpc = options.context.connections.rpc_config()?;
            let (context, coin) = coin::fetch(options.outpoint, input, options.minimum_sats, &rpc)?;
            let claim = Claim::new(&context, options.minimum_sats);
            let input = ClaimInput::new(context, coin.witness().clone(), options.minimum_sats);
            let cycles = guest::execute(&input, &claim)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "claim": claim,
                    "encoded": claim.to_bytes().as_hex().to_string(),
                    "image_id": guest::image_id().to_string(),
                    "cycles": cycles,
                }))?
            );
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
