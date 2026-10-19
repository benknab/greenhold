use super::Client;
use anyhow::{Context, Result, bail};
use bitcoin::{Amount, BlockHash, OutPoint, ScriptBuf, TxOut, hashes::Hash, hex::FromHex};
use serde::Deserialize;
use std::num::NonZeroU32;
use walledcity_core::coin::{Coin, CoinWitness};

#[derive(Debug, Deserialize)]
#[serde(try_from = "OutputFields")]
pub struct UnspentOutput {
    observed_at: BlockHash,
    confirmations: NonZeroU32,
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
    confirmations: NonZeroU32,
    value: serde_json::Number,
    coinbase: bool,
    #[serde(rename = "scriptPubKey")]
    script: ScriptFields,
}

impl TryFrom<OutputFields> for UnspentOutput {
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

impl UnspentOutput {
    pub fn observed_at(&self) -> BlockHash {
        self.observed_at
    }

    pub fn creation_height(&self, observed_height: u32) -> Result<u32> {
        observed_height
            .checked_sub(self.confirmations.get() - 1)
            .context("RPC confirmation count exceeds observed height")
    }

    pub fn into_coin(self, outpoint: OutPoint, hash: BlockHash, height: u32) -> Result<Coin> {
        Coin::new(outpoint, hash, height, self.coinbase, self.output)
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
pub struct InclusionProof {
    proved_at: BlockHash,
    target: u64,
    leaf: [u8; 32],
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

impl TryFrom<ProofFields> for InclusionProof {
    type Error = anyhow::Error;
    fn try_from(fields: ProofFields) -> Result<Self> {
        let [target]: [u64; 1] = fields
            .prooftargets
            .try_into()
            .map_err(|_| anyhow::anyhow!("expected one proof target"))?;
        let [leaf]: [bitcoin::hashes::sha256d::Hash; 1] = fields
            .hashesproven
            .try_into()
            .map_err(|_| anyhow::anyhow!("expected one proven leaf"))?;
        if target.leading_ones() > 61 || fields.proofhashes.len() > 61 {
            bail!("RPC membership path exceeds supported forest range");
        }
        Ok(Self {
            proved_at: fields.provedathash,
            target,
            leaf: leaf.to_byte_array(),
            hashes: fields
                .proofhashes
                .into_iter()
                .map(Hash::to_byte_array)
                .collect(),
        })
    }
}

impl InclusionProof {
    pub fn bind(self, coin: Coin) -> Result<CoinWitness> {
        if self.leaf != coin.leaf_hash() {
            bail!("RPC proven leaf does not match reconstructed coin");
        }
        CoinWitness::new(coin, self.proved_at, self.target, self.hashes)
    }
}

impl Client<'_> {
    pub fn unspent_output(&mut self, outpoint: OutPoint) -> Result<UnspentOutput> {
        let output: Option<UnspentOutput> =
            self.call("gettxout", (outpoint.txid, outpoint.vout, false))?;
        output.context("coin is absent from the confirmed UTXO set")
    }

    pub fn block_location(&mut self, hash: BlockHash) -> Result<super::BlockLocation> {
        let result: super::BlockLocation = self.call("getblockheader", (hash, true))?;
        if result.hash() != hash {
            bail!("RPC block location does not match requested hash");
        }
        Ok(result)
    }

    pub fn inclusion_proof(&mut self, outpoint: OutPoint) -> Result<InclusionProof> {
        self.call("proveutxochaintipinclusion", ([outpoint.txid], [outpoint.vout], 1))
            .context("cannot retrieve coin membership proof; coin must be unspent and node needs a proof index")
    }
}

#[cfg(test)]
mod tests;
