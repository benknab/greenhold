use anyhow::Result;
use bitcoin::hashes::sha256d;
use bitcoin::hex::FromHex;
use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, Txid};
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

/// Only the fields needed to discover candidate outpoints from address history.
#[derive(Debug, Deserialize)]
pub struct AddressTransaction {
    pub txid: Txid,
    // Utreexod omits this field for mempool transactions.
    #[serde(default)]
    pub confirmations: u64,
    #[serde(deserialize_with = "deserialize_inputs")]
    pub vin: Vec<OutPoint>,
    pub vout: Vec<AddressOutput>,
}

/// Coinbase inputs carry script bytes rather than an outpoint.
fn deserialize_inputs<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<OutPoint>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Spend { txid: Txid, vout: u32 },
        Coinbase { coinbase: String },
    }
    let mut outpoints = Vec::new();
    for input in Vec::<Input>::deserialize(deserializer)? {
        match input {
            Input::Spend { txid, vout } => outpoints.push(OutPoint::new(txid, vout)),
            Input::Coinbase { coinbase } => {
                Vec::<u8>::from_hex(&coinbase).map_err(serde::de::Error::custom)?;
            }
        }
    }
    Ok(outpoints)
}

#[derive(Debug, Deserialize)]
pub struct AddressOutput {
    pub n: u32,
    #[serde(rename = "scriptPubKey")]
    pub script_pub_key: ScriptPubKey,
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
