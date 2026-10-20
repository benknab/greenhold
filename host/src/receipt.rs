use anyhow::{Context as _, Result};
use risc0_zkvm::Receipt;

const TAG: &[u8] = b"greenhold/receipt/v1\0";

/// Encode a receipt file: the version tag, then the receipt in bincode.
pub fn encode(receipt: &Receipt) -> Result<Vec<u8>> {
    let mut bytes = TAG.to_vec();
    bytes.extend(bincode::serialize(receipt).context("receipt encoding")?);
    Ok(bytes)
}
