use anyhow::{Context, Result, bail};
use bitcoin::{Amount, BlockHash, ScriptBuf, hashes::sha256d, hex::FromHex};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub struct TxOutResponse {
    #[serde(rename = "bestblock")]
    pub best_block: BlockHash,
    pub confirmations: u32,
    pub coinbase: bool,
    #[serde(deserialize_with = "deserialize_amount")]
    pub value: Amount,
    #[serde(rename = "scriptPubKey")]
    pub script_pub_key: ScriptPubKey,
}

#[derive(Debug, Deserialize)]
pub struct ScriptPubKey {
    pub hex: ScriptBuf,
}

fn deserialize_amount<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Amount, D::Error> {
    let number = serde_json::Number::deserialize(deserializer)?;
    amount(&number).map_err(serde::de::Error::custom)
}

// Parse RPC decimal/scientific BTC notation without ever rounding through f64.
fn amount(number: &serde_json::Number) -> Result<Amount> {
    let text = number.as_str();
    let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if !whole
        .bytes()
        .chain(fraction.bytes())
        .all(|byte| byte.is_ascii_digit())
    {
        bail!("invalid RPC coin amount");
    }
    let digits = format!("{whole}{fraction}").parse::<u128>()?;
    let scale = 8i64 + i64::from(exponent.parse::<i32>()?) - i64::try_from(fraction.len())?;
    let power = 10u128
        .checked_pow(u32::try_from(scale.unsigned_abs())?)
        .context("RPC coin amount exponent outside range")?;
    let sats = if scale >= 0 {
        digits
            .checked_mul(power)
            .context("RPC coin amount overflow")?
    } else {
        if digits % power != 0 {
            bail!("RPC coin amount contains fractional satoshis");
        }
        digits / power
    };
    Ok(Amount::from_sat(u64::try_from(sats)?))
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
