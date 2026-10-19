use anyhow::{Context, Result, bail};
use bitcoin::{Amount, BlockHash, ScriptBuf, TxOut, hashes::Hash, hex::FromHex};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
#[serde(try_from = "OutputFields")]
pub struct TxOutResponse {
    observed_at: BlockHash,
    confirmations: u32,
    coinbase: bool,
    output: TxOut,
}

#[derive(Deserialize)]
struct ScriptFields {
    hex: String,
}

#[derive(Deserialize)]
struct OutputFields {
    bestblock: BlockHash,
    confirmations: u32,
    value: serde_json::Number,
    coinbase: bool,
    #[serde(rename = "scriptPubKey")]
    script: ScriptFields,
}

impl TryFrom<OutputFields> for TxOutResponse {
    type Error = anyhow::Error;
    fn try_from(fields: OutputFields) -> Result<Self> {
        let script_pubkey = ScriptBuf::from_bytes(Vec::<u8>::from_hex(&fields.script.hex)?);
        if script_pubkey.len() > 10_000 {
            bail!("RPC output script exceeds Bitcoin limit");
        }
        Ok(Self {
            observed_at: fields.bestblock,
            confirmations: fields.confirmations,
            coinbase: fields.coinbase,
            output: TxOut {
                value: amount(&fields.value)?,
                script_pubkey,
            },
        })
    }
}

impl TxOutResponse {
    pub fn observed_at(&self) -> BlockHash {
        self.observed_at
    }

    pub fn confirmations(&self) -> u32 {
        self.confirmations
    }

    pub fn is_coinbase(&self) -> bool {
        self.coinbase
    }

    pub fn output(&self) -> &TxOut {
        &self.output
    }
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
    if sats > u128::from(Amount::MAX_MONEY.to_sat()) {
        bail!("RPC coin amount exceeds Bitcoin limit");
    }
    Ok(Amount::from_sat(u64::try_from(sats)?))
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "ProofFields")]
pub struct ChainTipProof {
    proved_at: BlockHash,
    targets: Vec<u64>,
    leaves: Vec<[u8; 32]>,
    hashes: Vec<[u8; 32]>,
}

#[derive(Deserialize)]
struct ProofFields {
    provedathash: BlockHash,
    prooftargets: Vec<u64>,
    // This RPC reverses hashes for display, unlike getutreexoroots.
    proofhashes: Vec<bitcoin::hashes::sha256d::Hash>,
    hashesproven: Vec<bitcoin::hashes::sha256d::Hash>,
}

impl TryFrom<ProofFields> for ChainTipProof {
    type Error = anyhow::Error;
    fn try_from(fields: ProofFields) -> Result<Self> {
        if fields.prooftargets.len() != fields.hashesproven.len() {
            bail!("RPC proof target and leaf counts differ");
        }
        Ok(Self {
            proved_at: fields.provedathash,
            targets: fields.prooftargets,
            leaves: fields
                .hashesproven
                .into_iter()
                .map(Hash::to_byte_array)
                .collect(),
            hashes: fields
                .proofhashes
                .into_iter()
                .map(Hash::to_byte_array)
                .collect(),
        })
    }
}

impl ChainTipProof {
    pub fn proved_at(&self) -> BlockHash {
        self.proved_at
    }

    pub fn targets(&self) -> &[u64] {
        &self.targets
    }

    pub fn leaves(&self) -> &[[u8; 32]] {
        &self.leaves
    }

    pub fn hashes(&self) -> &[[u8; 32]] {
        &self.hashes
    }
}

/// Parsed accumulator state reported by the node, not independently verified chain state.
#[derive(Debug, Deserialize)]
#[serde(try_from = "RootsResponse")]
pub struct Roots {
    num_leaves: u64,
    roots: Vec<[u8; 32]>,
}

impl Roots {
    pub fn num_leaves(&self) -> u64 {
        self.num_leaves
    }

    /// Root bytes in the node's order, with no block-hash byte reversal.
    pub fn roots(&self) -> &[[u8; 32]] {
        &self.roots
    }
}

#[derive(Deserialize)]
struct RootsResponse {
    #[serde(rename = "numleaves")]
    num_leaves: u64,
    #[serde(deserialize_with = "deserialize_roots")]
    roots: Vec<[u8; 32]>,
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

impl TryFrom<RootsResponse> for Roots {
    type Error = anyhow::Error;
    fn try_from(response: RootsResponse) -> Result<Self> {
        let RootsResponse { num_leaves, roots } = response;
        if roots.len() != usize::try_from(num_leaves.count_ones())? {
            bail!("RPC root count does not match leaf count");
        }
        Ok(Self { num_leaves, roots })
    }
}

/// Hash and height from a node's block lookup.
#[derive(Debug, Deserialize)]
pub struct BlockLocation {
    hash: BlockHash,
    height: u32,
}

impl BlockLocation {
    pub fn hash(&self) -> BlockHash {
        self.hash
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests;
