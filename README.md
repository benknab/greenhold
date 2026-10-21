# Greenhold

An experimental Bitcoin proof-of-funds project using BIP-322 and RISC Zero.
It proves the following claim without revealing the coin or its script:

> There exists an eligible coin worth at least M, created by X and unspent at T,
> whose locking script verifies a signature over this exact claim.

M is the minimum amount in satoshis. X and T are block heights, with X ≤ T.
The coin must have been created at or before X and remain unspent in the accepted
chain state at T. Authorization means a valid BIP-322 signature for the coin's
locking script (its scriptPubKey) over a deterministic message derived from the
canonical claim's SHA-256 hash.

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
rzup install r0vm 3.0.6
```

`rzup default` selects the guest compiler for all projects on your machine.

Install `r0vm` at the SDK version shown above for fast guest image ID calculation
during builds. It is optional: without it, builds use a slower fallback and print
a confusing “No such file or directory” message. The host binary contains the
RISC Zero executor and prover, so it does not need `r0vm` to execute or prove.
The first build downloads one RISC Zero circuit file and checks its SHA-256 hash.

The reproducible build also needs [Docker](https://docs.docker.com/get-docker/).

## Builds

Greenhold has two builds. They compile the same guest program in different
environments, so they give different guest image IDs. The image ID identifies the
guest program. A binary accepts a receipt only if the receipt has the binary's
image ID.

- **Reproducible build:** `cargo reproducible` writes
  `target/reproducible/greenhold-host`. It compiles the guest in RISC Zero's
  builder image `risczero/risc0-guest-builder:r0.1.97.0`, pinned by digest, for
  `linux/amd64`. On other platforms, Docker runs the image in emulation. The
  build fails unless the image ID equals the value in `guest-builder/image-id`.
- **Development build:** `cargo build` and `cargo run` compile the guest with
  your local rzup toolchain. The image ID depends on your machine. Use this build
  only for development and tests.

Use the reproducible build for every receipt that you share or verify. Build it
from the same commit as the other person. A receipt from a different guest
version fails verification.

## Utreexod requirements

Configure Utreexod with these flags:

- `--addrindex` enables address transaction searches through `searchrawtransactions`.
- `--txindex` enables confirmed transaction lookups and supports the address index.
- `--prune=0` retains historical blocks for transaction retrieval and index construction.
- `--utreexoproofindex` or `--flatutreexoproofindex` enables Utreexo membership
  proofs and historical roots for proving and verification.

## Usage

The default public endpoint lets you test Greenhold without running a Utreexod node.

The `claim` command sends your outpoint to that node, which can link the
coin to your request. For privacy, use your own node with `--utreexod-rpc`,
`--utreexod-rpc-user`, and `--utreexod-rpc-password`.

```sh
cargo reproducible
target/reproducible/greenhold-host claim <TXID:VOUT> --first-height <X> --minimum-sats <M> --witness coin.witness
```

### Prepare and sign

`claim` retrieves the coin and its tip membership proof, fetches the chain context
at that proof's exact block, and checks eligibility. It writes `coin.witness`,
the private binary coin data and membership path. Keep this file private.

It prints JSON containing readable `claim` details, the Base64 claim in `encoded`,
the guest `image_id`, and the exact `message` to sign. The `encoded` value is
220 characters of standard padded Base64 encoding the 163 canonical claim bytes;
pass it directly to `prove` or `verify --claim`. No claim file is needed.
Sign `message` with the key for the coin's
locking script. The message is `greenhold/claim:` followed by the lowercase
SHA-256 hex digest of the binary claim, with no newline or terminator.

The canonical bytes are `greenhold/claim/v1\0`, the network's 32-byte genesis hash,
then for X and T a little-endian u32 height and 32-byte block hash, little-endian
u64 M, and the 32-byte chain-context digest. Block hashes use internal byte order;
the SHA-256 digest is not reversed. JSON and Base64 text are not the hash input.

T is the node's tip when the membership proof is captured. Without
`--first-height`, X is T. Without `--minimum-sats`, M is 1 satoshi. An explicit
positional T must match the proof's block. Use `--network signet` or
`--network regtest` on each command for other networks.

### Prove the prepared claim

```sh
target/reproducible/greenhold-host prove '<BASE64_CLAIM>' --witness coin.witness --signature '<SIGNATURE>' --output claim.receipt
```

1. The host checks the signature over the claim's derived message against the
   saved coin's script before node access.
2. The node supplies historical chain state at X and T. The host reconstructs
   the claim, requires exact equality with the decoded Base64 claim, and checks the saved
   coin's membership and eligibility against that state.
3. The guest authenticates the saved coin against that state, checks eligibility,
   constructs the claim, derives its signing message, and verifies the signature
   against the authenticated script. It commits the canonical claim bytes.
4. The host proves the run and verifies the receipt and its journal before writing
   `claim.receipt`. Its JSON report includes the receipt path and size.

Witness and receipt files use binary formats, without Base64 wrapping.

Use `--dry-run` instead of `--output` to stop after step 3 and write no receipt.
A dry run also prints the guest cycle count. The cycle count depends on the coin,
so do not share it.

The public report and receipt contain no coin data. The saved witness remains
usable after tip advancement and spending after T: proving checks membership at
T, not whether the coin remains unspent now. Historical node state must still be
available and accepted. A reorg replacing T causes chain-state checks to fail.

Simple signatures (`smp`, or unprefixed) support P2WPKH and P2TR key-path spends.
Full signatures (`ful`) also support P2PKH and P2SH-P2WPKH. Legacy signatures
support P2PKH only. Time-locked and proof-of-funds signatures are rejected.
See [`signer/`](signer/) for generating test signatures.

### Verify against the intended claim

Use your own node, a reproducible build, and an independently approved expected claim:

```sh
target/reproducible/greenhold-host verify claim.receipt --claim '<EXPECTED_BASE64_CLAIM>'
```

1. The host reads the file, up to 1 MiB, and decodes the receipt.
2. The host checks that the receipt is a succinct proof of this guest's run.
3. The receipt's claim must name the network from `--network` (`bitcoin` by
   default) and exactly equal the decoded expected claim, including X, T, and M.
4. The node supplies its blocks at heights X and T and its Utreexo state at T.
   The host reconstructs the claim from that data and the expected M.
5. The host accepts the receipt only if the two claims are equal. It then prints
   the claim.

If the node cannot supply the state at T, verification fails. T can be an old
block, so the node must keep historical Utreexo roots. The coin can be spent
after T, because the claim is about T.

Choose the expected claim independently; copying it from the submitted receipt
does not establish your intended X, T, or M. Repeated verification of the same
receipt remains valid while the historical chain state is accepted.

`inspect <FILE>` checks the receipt proof and prints the claim without node access.
It does not check an expected claim or chain state, so do not accept a claim
from `inspect` alone.

The coin witness and signature are private guest inputs. The signing message is
public and derived inside the guest; it cannot be supplied independently of the claim.

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

These checks use the development build:

```sh
cargo fmt --all --check
cargo lint
cargo test --locked --release --workspace --exclude greenhold-guest
cargo build --locked --release
```
