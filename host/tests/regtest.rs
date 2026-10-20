// Run explicitly with `cargo regtest`. Missing Docker is a failure, never a skip.
#[path = "regtest/prove.rs"]
mod prove;
mod support;
