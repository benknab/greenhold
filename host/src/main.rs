mod cli;
mod coin;
mod connection;
mod context;

#[cfg(test)]
mod test_support;

use anyhow::Result;
use clap::Parser;
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
        cli::Command::Coin(options) => {
            let input = context::Input::new(
                options.context.network.into(),
                options.context.first_height,
                options.context.last_height,
            )?;
            let rpc = options.context.connections.rpc_config()?;
            let (context, coin) = coin::fetch(options.outpoint, input, options.minimum_sats, &rpc)?;
            let witness = coin.witness();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "network": context.network(),
                    "context_digest": context.digest(),
                    "context": context,
                    "witness": witness,
                    "minimum_sats": options.minimum_sats,
                    "amount_sats": witness.coin().output().value.to_sat(),
                    "address": bitcoin::Address::from_script(&witness.coin().output().script_pubkey,
                        context.network()).ok().map(|address| address.to_string()),
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
