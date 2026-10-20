# Signer

This is AI-generated slop. Its only purpose is to provide test signatures for
Greenhold. Do not use it for real funds or as a reference implementation.

The signer derives single-key addresses from a BIP-39 mnemonic or a BIP-32 root
seed. It signs BIP-322 messages for these script types:

| Type          | Formats (default first) |
| ------------- | ----------------------- |
| `p2pkh`       | `full`, `legacy`        |
| `p2sh-p2wpkh` | `full`                  |
| `p2wpkh`      | `simple`, `full`        |
| `p2tr`        | `simple`, `full`        |

Signatures have no time locks. The signer reads secrets only from files.

## Usage

Install [Bun](https://bun.sh/), then run these commands from this directory:

```sh
bun install --frozen-lockfile
bun run check
bun src/main.ts address --type p2wpkh --mnemonic-file <FILE>
bun src/main.ts sign --type p2wpkh --message <TEXT> --mnemonic-file <FILE>
```

Without `--path`, the signer uses the BIP-44, BIP-49, BIP-84, or BIP-86 path
for the type. Run `bun src/main.ts --help` for all options.
