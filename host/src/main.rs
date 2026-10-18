mod cli;
mod connection;

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
        cli::Command::Nodes(options) => {
            let rpc = options.rpc_config()?;
            let status = connection::status(&options.electrum, &options.utreexod, &rpc)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
    }
    Ok(())
}
