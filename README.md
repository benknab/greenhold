# Greenhold

Experimental Bitcoin proof-of-funds using BIP-322 and RISC Zero. Prove that a coin
meets a minimum amount and an unspent period without revealing the coin or address.

## Install

Release binaries are for macOS 11 or later on Apple silicon. On other systems,
[build from source](#build-from-source).

1. Download the `.tar.gz` archive and `SHA256SUMS` from
   [Releases](https://github.com/benknab/greenhold/releases).
2. In the download folder, check the archive and extract the binary:

```sh
shasum -a 256 -c SHA256SUMS
tar -xzf greenhold-v*-aarch64-apple-darwin.tar.gz
./greenhold --version
```

- If macOS blocks the binary, run `xattr -d com.apple.quarantine greenhold`.
  The binary is not notarized by Apple.
- The prover and verifier must use the same guest image ID, or receipts fail
  verification. `greenhold --version` reports the guest image ID.

## Usage

Examples use Bitcoin mainnet and public servers; see [Privacy](#privacy) to override them.

### Discover

List the coins of an address with their outpoints (`TXID:VOUT`). Skip this step if
your wallet shows the outpoint of the coin you want to prove.

```sh
./greenhold discover --address 'YOUR_BITCOIN_ADDRESS'
```

### Claim

Replace the example values and prepare the claim:

```sh
./greenhold claim \
  --outpoint 'TXID:VOUT' \
  --unspent-since 314159 \
  --minimum-sats 1337 \
  --output claim.bin \
  --witness coin.witness
```

- The minimum amount applies to one coin.
- `coin.witness` identifies your coin. Keep it private.
- Optionally add `--nonce 'CHALLENGE_TEXT'` to bind the claim to a verifier's challenge.
- The output includes the address and message to sign.

### Prove

1. Sign the printed message with the printed address.
2. Paste the signature below.

Accepted signatures: BIP-322 simple for P2WPKH/P2TR, full for
P2PKH/P2SH-P2WPKH/P2WPKH/P2TR, or legacy for P2PKH.

```sh
./greenhold prove \
  --claim claim.bin \
  --witness coin.witness \
  --signature 'SIGNATURE' \
  --output claim.receipt
```

- Proving runs locally and can take several minutes.
- Share `claim.receipt` with the verifier.

### Verify

```sh
./greenhold verify --receipt claim.receipt
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

## Build from source

Building from source is optional. Use it on other systems, or to check a release.

### Install dependencies

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

### Create reproducible build

- Pinned Docker tooling builds the same guest program as the releases. Builds from
  the same commit give the same guest image ID and compatible receipts.
- Start Docker, then run:

```sh
cargo reproducible
cd target/reproducible
```

### Check a release

1. Check out the release tag, for example `v0.1.0-beta.1`.
2. On macOS, start Docker and run `scripts/build-release.sh`.
3. Compare the SHA-256 values in `target/dist/v<VERSION>/` with the release files.

- The release `build-info.txt` file lists the tool versions. Other Rust or Xcode
  versions can give a different binary.
- The guest image ID depends only on the commit, so it must match in all cases.

## Development

- Build: `cargo build --locked`.
- Run: `cargo run --locked -- --help`.
- Format: `cargo fmt --all`.
- Focused tests: `cargo test --locked -p greenhold-host cli::tests`.
- Workspace lint: `cargo lint`.
- Full native suite: `cargo quick`.
- Node scenarios: `cargo regtest <test-filter>` (requires Docker).

Use dev/debug builds and checks relevant to the change. Use release binaries or
reproducible builds for shared receipts. Use `scripts/build-release.sh` for release
validation.
