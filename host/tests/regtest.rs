// Run explicitly with `cargo regtest`. Missing Docker is a failure, never a skip.
#[path = "regtest/discover.rs"]
mod discover;
#[path = "regtest/prove.rs"]
mod prove;
mod support;
