use anyhow::{Context as _, Result, bail};
use bincode::{DefaultOptions, Options};
use risc0_zkvm::Receipt;
use std::fs::File;
use std::path::Path;

const TAG: &[u8] = b"greenhold/receipt/v1\0";

/// Succinct receipts are about 224 KB. The limit bounds reads of untrusted files.
const MAX_FILE_BYTES: u64 = 1 << 20;

/// The `bincode::serialize` format, with no trailing bytes. The limit keeps a whole
/// file within `MAX_FILE_BYTES`, so `prove` never writes a file that `read` rejects.
fn options() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_FILE_BYTES - TAG.len() as u64)
}

/// Encode a receipt file: the version tag, then the receipt in bincode.
pub fn encode(receipt: &Receipt) -> Result<Vec<u8>> {
    let mut bytes = TAG.to_vec();
    bytes.extend(options().serialize(receipt).context("receipt encoding")?);
    Ok(bytes)
}

/// Read and decode a receipt file. Decoding checks the format only, not the proof.
pub fn read(path: &Path) -> Result<Receipt> {
    use std::io::Read as _;

    let file =
        File::open(path).with_context(|| format!("cannot read receipt {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read receipt {}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("receipt file exceeds {MAX_FILE_BYTES} bytes");
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<Receipt> {
    let encoded = bytes
        .strip_prefix(TAG)
        .context("receipt tag or version is not supported")?;
    options().deserialize(encoded).context("receipt decoding")
}

#[cfg(test)]
mod tests;
