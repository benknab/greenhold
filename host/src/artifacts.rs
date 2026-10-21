//! Prepared public claims and private coin witnesses. Neither file is trusted state.

use anyhow::{Context as _, Result, bail};
use bincode::Options as _;
use greenhold_core::{claim::Claim, coin::CoinWitness};
use std::{fs::File, io::Read as _, path::Path};

// Canonical claim bytes include the domain and version tag.
const CLAIM_BYTES: u64 = 163;
const WITNESS_TAG: &[u8] = b"greenhold/witness/v1\0";
// A bounded coin script (10 KB), at most 61 sibling hashes, and fixed coin fields.
const MAX_WITNESS_BYTES: u64 = 16 * 1024;

pub fn encode_claim(claim: &Claim) -> Vec<u8> {
    claim.to_bytes()
}

pub fn read_claim(path: &Path) -> Result<Claim> {
    let bytes = read(path, CLAIM_BYTES).context("cannot read claim file")?;
    Claim::try_from(bytes.as_slice()).context("claim decoding")
}

pub fn read_witness(path: &Path) -> Result<CoinWitness> {
    let bytes = read(path, MAX_WITNESS_BYTES).context("cannot read witness")?;
    let encoded = bytes
        .strip_prefix(WITNESS_TAG)
        .context("witness tag or version is not supported")?;
    options().deserialize(encoded).context("witness decoding")
}

pub fn encode_witness(witness: &CoinWitness) -> Result<Vec<u8>> {
    let mut bytes = WITNESS_TAG.to_vec();
    bytes.extend(options().serialize(witness).context("witness encoding")?);
    Ok(bytes)
}

fn options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_WITNESS_BYTES - WITNESS_TAG.len() as u64)
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("file exceeds {limit} bytes");
    }
    Ok(bytes)
}
