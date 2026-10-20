# Greenhold

An experimental Bitcoin proof-of-funds project using BIP-322 and RISC Zero.
The goal is to prove the following claim without revealing the coin or its script:

> There exists an eligible coin worth at least M, created by X and unspent at T,
> whose locking script authorized this exact claim.

M is the minimum amount in satoshis. X and T are block heights, with X ≤ T.
The coin must have been created at or before X and remain unspent in the accepted
chain state at T. Authorization means a valid BIP-322 signature for the coin's
locking script (its scriptPubKey) over the exact claim. The script does not need
an address form.

The signature establishes authorization at signing time. It does not identify a
person or prove that the signer held the keys at X, at T, or throughout that period.
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
```

`rzup default` selects the guest compiler for all projects on your machine.

The guest needs only the rzup Rust toolchain. You do not need `r0vm` or a full
`rzup install`, because the host binary contains the RISC Zero executor and
prover. The first build downloads one RISC Zero circuit file and checks its
SHA-256 hash.

## Usage

Warning: the `prove` command sends your outpoint to the Utreexod node. The default
public node can link that coin to your request. To keep the coin private, use your
own node with `--utreexod-rpc`, `--utreexod-rpc-user`, and `--utreexod-rpc-password`.

```sh
cargo run --locked --release -- prove <TXID:VOUT> --first-height <X> --minimum-sats <M> --output <FILE>
```

This command builds the host and guest. It then proves the claim for one coin:

1. The node supplies the coin, its Utreexo membership proof, and the chain state.
2. The host checks that the coin is eligible for the claim.
3. The host runs the guest in the zkVM without proving. The guest checks the
   coin's membership and eligibility again and commits the claim.
4. The host proves the guest run. It verifies the receipt against the guest image
   ID and checks that the receipt contains the expected claim.
5. The host writes the receipt to `<FILE>`. It prints the public claim as JSON and
   as canonical bytes in hex, the guest image ID, and the receipt size.

Use `--dry-run` instead of `--output` to stop after step 3 and write no receipt.
A dry run also prints the guest cycle count. The cycle count depends on the coin,
so do not share it.

The output contains no coin data. T is the node's current tip, because the node
proves membership only at its tip. Without `--first-height`, X is T. Without
`--minimum-sats`, M is 1 satoshi. Use `--network signet` or `--network regtest`
for other networks.

## Development

```sh
cargo fmt --all --check
cargo lint
cargo test --locked --release --workspace --exclude greenhold-guest
cargo build --locked --release
```
