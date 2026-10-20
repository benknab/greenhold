#![no_main]

use greenhold_core::input::ClaimInput;
use risc0_zkvm::guest::env;

risc0_zkvm::guest::entry!(main);

/// Commit the claim bytes for an eligible coin. Exit code 1 means a rejected input
/// and commits nothing.
fn main() {
    let input: ClaimInput = env::read();
    match input.check() {
        Ok(claim) => env::commit_slice(&claim.to_bytes()),
        Err(_) => env::exit(1),
    }
}
