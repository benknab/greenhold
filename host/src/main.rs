mod cli;
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
        cli::Command::Context(options) => {
            let request = context::Request::new(
                options.network.into(),
                options.first_height,
                options.last_height,
            )?;
            let rpc = options.connections.rpc_config()?;
            let context = context::fetch(request, &options.connections.electrum, &rpc)?;
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
