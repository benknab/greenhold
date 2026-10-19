# Greenhold

An experimental Bitcoin proof-of-funds project using BIP-322 and RISC Zero.
The goal is to prove the following claim without revealing the address or coin:

> There exists an eligible coin worth at least M, created by X and unspent at T,
> whose controlling address authorized this exact claim.

M is the minimum amount in satoshis. X and T are block heights, with X ≤ T.
The coin must have been created at or before X and remain unspent in the accepted
chain state at T. Authorization means a valid signature for the coin's controlling
address over the exact claim.

The signature establishes authorization at signing time. It does not identify a
person or prove that the signer held the key at X, at T, or throughout that period.
The signature alone does not establish when it was made.

## Getting started

Install [rustup](https://rustup.rs/) and
[`rzup`](https://dev.risczero.com/api/zkvm/install). The repository selects Rust
1.97.0 automatically and uses RISC Zero SDK 3.0.6.

```sh
git clone https://github.com/benknab/greenhold.git
cd greenhold
rzup install rust 1.97.0
rzup default rust 1.97.0
cargo run --locked --release -- context 900000
```

This builds the host and guest, then retrieves public chain context from live nodes.
`rzup default` selects the guest compiler for all projects on your machine.

## Development

```sh
cargo fmt --all --check
cargo lint
cargo test --locked --release --workspace --exclude greenhold-guest
cargo build --locked --release
```
