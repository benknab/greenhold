# Greenhold

Experimental Bitcoin proof-of-funds using BIP-322 and RISC Zero. Prove that a coin
meets a minimum amount and an unspent period without revealing the coin or address.

## Install dependencies

1. C/C++ compiler.
2. [rustup](https://rustup.rs/).
3. [rzup](https://dev.risczero.com/api/zkvm/install).
4. [Docker](https://docs.docker.com/get-docker/).

```sh
rzup install rust 1.97.0
rzup default rust 1.97.0
rzup install cpp 2024.1.5
rzup install r0vm 3.0.6
```

## Create reproducible build

- There are no binary releases yet.
- Pinned Docker tooling lets the prover and verifier build the same guest program
  and exchange compatible receipts. Use the same commit.
- Start Docker, then run:

```sh
cargo reproducible
GH="$PWD/target/reproducible/greenhold"
```

## Usage

### Claim

- Examples use Bitcoin mainnet and public servers; see [Privacy](#privacy) to override them.
- If you know the outpoint (`TXID:VOUT`), use it directly. Otherwise, use `discover`:

```sh
"$GH" discover --address 'YOUR_BITCOIN_ADDRESS'
```

Replace the example values and prepare the claim:

```sh
DATA="$HOME/.local/share/greenhold"
mkdir -p "$DATA"

"$GH" claim \
  --outpoint 'TXID:VOUT' \
  --unspent-since 950000 \
  --minimum-sats 1337 \
  --output "$DATA/claim.bin" \
  --witness "$DATA/coin.witness"
```

- The minimum amount applies to one coin.
- Optionally add `--nonce 'CHALLENGE_TEXT'` to bind the claim to a verifier's challenge.
- The output includes the address and message to sign.

### Prove

1. Sign the printed message with the printed address.
2. Paste the signature below.

Accepted signatures: BIP-322 simple for P2WPKH/P2TR, full for
P2PKH/P2SH-P2WPKH/P2WPKH/P2TR, or legacy for P2PKH.

```sh
"$GH" prove \
  --claim "$DATA/claim.bin" \
  --witness "$DATA/coin.witness" \
  --signature 'SIGNATURE' \
  --output "$DATA/claim.receipt"
```

- Proving runs locally and can take several minutes.
- Share `claim.receipt` with the verifier.

### Verify

```sh
"$GH" verify --receipt "$DATA/claim.receipt"
```

- If the claim used a nonce, pass the same original text with `--nonce 'CHALLENGE_TEXT'`.
- Check the reported minimum amount and block heights against your requirements.

## Privacy

Public servers are used by default for ease of use. Override them with your own servers for private queries.

### Electrum

- Override discovery with `--electrum 'tcp://HOST:PORT'` or `--electrum 'tls://HOST:PORT'`.
- The server needs a synchronized address/script index.

### Utreexod

- Required: `--noassumeutreexo`, `--prune=0`, and either `--utreexoproofindex` or
  `--flatutreexoproofindex` for membership proofs and historical roots.
- For `discover --utreexod`, also enable `--addrindex --txindex` for address searches.
- Wait for synchronization and indexing to finish.
- Add these flags to `claim`, `prove`, `verify`, or `discover --utreexod`:

```sh
--utreexod-rpc 'http://HOST:PORT/' \
--utreexod-rpc-user 'USER' \
--utreexod-rpc-password 'PASSWORD'
```

### RISC Zero privacy notice

- [RISC Zero](https://dev.risczero.com/api/security-model#zero-knowledge-proving)
  states that its system hides inputs and witness data, but lacks a mathematical
  proof of its zero-knowledge property.
- It advises caution for critical privacy requirements. Greenhold inherits this limitation.

## Development

- Build: `cargo build --locked`.
- Run: `cargo run --locked -- --help`.
- Format: `cargo fmt --all`.
- Focused tests: `cargo test --locked -p greenhold-host cli::tests`.
- Workspace lint: `cargo lint`.
- Full native suite: `cargo quick`.
- Node scenarios: `cargo regtest <test-filter>` (requires Docker).

Use dev/debug builds and checks relevant to the change. Use reproducible builds
for shared receipts and release validation.
