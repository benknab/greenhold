# Greenhold

An experimental Bitcoin proof-of-funds project using BIP-322 and RISC Zero.
It proves the following claim without revealing the coin or its script:

> There exists an eligible coin worth at least M, created by X and unspent at T,
> whose locking script verifies a signature over the expected message.

M is the minimum amount in satoshis. X and T are block heights, with X ≤ T.
The coin must have been created at or before X and remain unspent in the accepted
chain state at T. Authorization means a valid BIP-322 signature for the coin's
locking script (its scriptPubKey) over an arbitrary message. Signing the exact
funds claim is planned for later.

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

## Utreexod requirements

Configure Utreexod with these flags:

- `--addrindex` enables address transaction searches through `searchrawtransactions`.
- `--txindex` enables confirmed transaction lookups and supports the address index.
- `--prune=0` retains historical blocks for transaction retrieval and index construction.
- `--utreexoproofindex` or `--flatutreexoproofindex` enables Utreexo membership
  proofs and historical roots for proving and verification.

## Usage

The default public endpoint lets you test Greenhold without running a Utreexod node.

Warning: the `prove` command sends your outpoint to that node, which can link the
coin to your request. For privacy, use your own node with `--utreexod-rpc`,
`--utreexod-rpc-user`, and `--utreexod-rpc-password`.

```sh
cargo run --locked --release -- prove <TXID:VOUT> --address <ADDRESS> --message <TEXT> --signature <SIGNATURE> --first-height <X> --minimum-sats <M> --output <FILE>
```

This command builds the host and guest. It then proves the claim for one coin:

1. The host checks the signature against the address and message before node access.
2. The node supplies the coin, its Utreexo membership proof, and the chain state.
   The host checks eligibility and that the address's script locks the coin.
3. The host runs the guest in the zkVM without proving. The guest checks the
   coin's membership and eligibility again, verifies the signature against the
   authenticated coin's script, and commits the claim with the message's SHA-256 digest.
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

Simple signatures (`smp`, or unprefixed) support P2WPKH and P2TR key-path spends.
Full signatures (`ful`) also support P2PKH and P2SH-P2WPKH. Legacy signatures
support P2PKH only. Time-locked and proof-of-funds signatures are rejected.
See [`signer/`](signer/) for generating test signatures.

To check a receipt from another person, use your own node:

```sh
cargo run --locked --release -- verify <FILE> --message <TEXT>
```

1. The host reads the file, up to 1 MiB, and decodes the receipt.
2. The host checks that the receipt is a succinct proof of this guest's run.
3. The host reads the claim from the receipt. The claim must name the network
   from `--network`, which is `bitcoin` by default, and the SHA-256 digest of
   `--message`. Messages use exact UTF-8 bytes without normalization.
4. The node supplies its blocks at heights X and T and its Utreexo state at T.
   The host builds the expected claim from that data, the claim's M, and the expected message.
5. The host accepts the receipt only if the two claims are equal. It then prints
   the claim.

If the node cannot supply the state at T, verification fails. T can be an old
block, so the node must keep historical Utreexo roots. The coin can be spent
after T, because the claim is about T.

Check the printed X, T, and M yourself. `verify` accepts any eligible claim
that matches the chain, including one with a lower M than you expect.

`inspect <FILE>` checks the receipt proof and prints the claim without node access.
It does not check an expected message or chain state, so do not accept a claim
from `inspect` alone.

The message and signature are private guest inputs. The public message digest
allows anyone with a candidate message to check whether it matches.

## Privacy

The receipt hides the coin only as well as RISC Zero hides guest inputs.
[RISC Zero's security model](https://dev.risczero.com/api/security-model) says that
the zkVM hides all program inputs and witness data, and targets perfect
zero-knowledge. It also says that RISC Zero has not yet written a mathematical
proof that the system is zero-knowledge. RISC Zero advises caution for critical
privacy requirements. Greenhold has the same limit.

Greenhold makes succinct receipts. RISC Zero states that receipts without its
recursion step leak information about the execution length. Here, the execution
length depends on the coin. Succinct receipts pass through the recursion step, so
they do not leak it.

The prover sees all private data, so `prove` runs the prover on your machine.

## Development

```sh
cargo fmt --all --check
cargo lint
cargo test --locked --release --workspace --exclude greenhold-guest
cargo build --locked --release
```
