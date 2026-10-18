# Walledcity

An experimental Bitcoin proof-of-funds project using BIP-322 and RISC Zero.
The goal is to prove that funds meet a balance threshold without revealing the
individual Bitcoin outputs.

## Getting started

Install [rustup](https://rustup.rs/) and
[`rzup`](https://dev.risczero.com/api/zkvm/install). The repository selects Rust
1.97.0 automatically and uses RISC Zero SDK 3.0.6.

```sh
git clone https://github.com/benknab/walledcity.git
cd walledcity
rzup install rust 1.97.0
rzup default rust 1.97.0
cargo run --locked --release -- nodes
```

This builds the host and guest, then checks live node access.
Use `--utreexod bip324://HOST:PORT` for encrypted Bitcoin P2P v2.
The default `tcp://` endpoint uses v1. Explicit v2 connections fail rather than fall back to v1.
`rzup default` selects the guest compiler for all projects on your machine.

## Development

```sh
cargo fmt --all --check
cargo lint
cargo test --locked --release --workspace --exclude walledcity-guest
cargo build --locked --release
```
