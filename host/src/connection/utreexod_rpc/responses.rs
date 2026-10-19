use anyhow::Result;
use bitcoin::{Amount, BlockHash, ScriptBuf, hashes::sha256d, hex::FromHex};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub struct TxOutResponse {
    #[serde(rename = "bestblock")]
    pub best_block: BlockHash,
    pub confirmations: u32,
    pub coinbase: bool,
    #[serde(with = "bitcoin::amount::serde::as_btc")]
    pub value: Amount,
    #[serde(rename = "scriptPubKey")]
    pub script_pub_key: ScriptPubKey,
}

#[derive(Debug, Deserialize)]
pub struct ScriptPubKey {
    pub hex: ScriptBuf,
}

#[derive(Debug, Deserialize)]
pub struct ChainTipProof {
    #[serde(rename = "provedathash")]
    pub proved_at_hash: BlockHash,
    #[serde(rename = "prooftargets")]
    pub proof_targets: Vec<u64>,
    // This RPC reverses hashes for display, unlike getutreexoroots.
    #[serde(rename = "hashesproven")]
    pub hashes_proven: Vec<sha256d::Hash>,
    #[serde(rename = "proofhashes")]
    pub proof_hashes: Vec<sha256d::Hash>,
}

/// Parsed accumulator state reported by the node, not independently verified chain state.
#[derive(Debug, Deserialize)]
pub struct Roots {
    #[serde(rename = "numleaves")]
    pub num_leaves: u64,
    /// Root bytes in the node's order, with no block-hash byte reversal.
    #[serde(deserialize_with = "deserialize_roots")]
    pub roots: Vec<[u8; 32]>,
}

fn deserialize_roots<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<[u8; 32]>, D::Error> {
    // Go encodes a nil slice as null for an empty accumulator.
    let roots = Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default();
    roots
        .iter()
        .map(|text| {
            <[u8; 32]>::from_hex(text).map_err(|_| serde::de::Error::custom("invalid root hash"))
        })
        .collect()
}

/// Hash and height from a node's block lookup.
#[derive(Debug, Deserialize)]
pub struct BlockLocation {
    pub hash: BlockHash,
    pub height: u32,
}

#[cfg(test)]
mod tests;
