//! Single-coin membership. The caller must obtain the context from its own node.

use crate::context::ChainContext;
use anyhow::{Result, bail};
use bitcoin::{Amount, BlockHash, OutPoint, TxOut, consensus::serialize, hashes::Hash};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512, Sha512_256};

/// Data committed by a Utreexod `UtreexoV1` leaf.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CoinFields")]
pub struct Coin {
    outpoint: OutPoint,
    creation_block_hash: BlockHash,
    creation_height: u32,
    coinbase: bool,
    output: TxOut,
}

#[derive(Deserialize)]
struct CoinFields {
    outpoint: OutPoint,
    creation_block_hash: BlockHash,
    creation_height: u32,
    coinbase: bool,
    output: TxOut,
}

impl Coin {
    /// Parse leaf data. Membership must still be checked.
    ///
    /// # Errors
    /// Rejects a null outpoint, unrepresentable height, amount, or oversized script.
    pub fn new(
        outpoint: OutPoint,
        creation_block_hash: BlockHash,
        creation_height: u32,
        coinbase: bool,
        output: TxOut,
    ) -> Result<Self> {
        if outpoint.is_null() || creation_height == 0 || creation_height > i32::MAX as u32 {
            bail!("invalid coin outpoint or creation height");
        }
        if output.value > Amount::MAX_MONEY || output.script_pubkey.len() > 10_000 {
            bail!("coin amount or script exceeds Bitcoin limits");
        }
        Ok(Self {
            outpoint,
            creation_block_hash,
            creation_height,
            coinbase,
            output,
        })
    }

    #[must_use]
    pub fn output(&self) -> &TxOut {
        &self.output
    }

    /// Tagged SHA-512/256 over node leaf serialization, using raw hash bytes.
    #[must_use]
    pub fn leaf_hash(&self) -> [u8; 32] {
        let tag = Sha512::digest(b"UtreexoV1");
        let mut hash = Sha512_256::new();
        hash.update(tag);
        hash.update(tag);
        hash.update(self.creation_block_hash.to_byte_array());
        hash.update(serialize(&self.outpoint));
        hash.update(((self.creation_height << 1) | u32::from(self.coinbase)).to_le_bytes());
        hash.update(serialize(&self.output));
        hash.finalize().into()
    }
}

impl TryFrom<CoinFields> for Coin {
    type Error = anyhow::Error;
    fn try_from(fields: CoinFields) -> Result<Self> {
        Self::new(
            fields.outpoint,
            fields.creation_block_hash,
            fields.creation_height,
            fields.coinbase,
            fields.output,
        )
    }
}

/// One leaf and its path at one exact block. Targets use node 63-row positions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "WitnessFields")]
pub struct CoinWitness {
    coin: Coin,
    proved_at: BlockHash,
    target: u64,
    hashes: Vec<[u8; 32]>,
}

#[derive(Deserialize)]
struct WitnessFields {
    coin: Coin,
    proved_at: BlockHash,
    target: u64,
    hashes: Vec<[u8; 32]>,
}

impl CoinWitness {
    /// Parse a bounded single-leaf path; does not authenticate it.
    ///
    /// # Errors
    /// Rejects paths or target rows outside the supported forest range.
    pub fn new(
        coin: Coin,
        proved_at: BlockHash,
        target: u64,
        hashes: Vec<[u8; 32]>,
    ) -> Result<Self> {
        if target.leading_ones() > 61 || hashes.len() > 61 {
            bail!("membership path exceeds supported forest range");
        }
        Ok(Self {
            coin,
            proved_at,
            target,
            hashes,
        })
    }

    #[must_use]
    pub fn coin(&self) -> &Coin {
        &self.coin
    }

    /// Verify the requested outpoint, creation cutoff, state block, and full leaf path.
    /// This establishes unspentness, not authorization or coinbase maturity.
    ///
    /// # Errors
    /// Rejects mismatched claims, out-of-forest targets, and incorrect paths or roots.
    pub fn verify(&self, context: &ChainContext, expected: OutPoint) -> Result<()> {
        if self.coin.outpoint != expected {
            bail!("coin does not match requested outpoint");
        }
        if self.proved_at != context.last().hash() {
            bail!("membership proof block does not match selected context");
        }
        if self.coin.creation_height > context.first().height() {
            bail!("coin was created after first-height cutoff");
        }
        let row = self.target.leading_ones();
        let start = if row == 0 { 0 } else { u64::MAX << (64 - row) };
        let offset = self.target - start;
        // u128 keeps hostile wire offsets from overflowing before range checks.
        let base = u128::from(offset) << row;
        let end = base + (1u128 << row);
        let mut root_start = 0u128;
        let mut roots = context.state().roots().iter();
        for root_row in (0..62).rev() {
            if context.state().num_leaves() & (1u64 << root_row) == 0 {
                continue;
            }
            let root = roots
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing accumulator root"))?;
            let root_end = root_start + (1u128 << root_row);
            if base < root_start || end > root_end || row > root_row {
                root_start = root_end;
                continue;
            }
            if self.hashes.len() != usize::try_from(root_row - row)? {
                bail!("membership path length does not match target subtree");
            }
            return self.verify_path(root, offset);
        }
        bail!("membership target outside occupied forest")
    }

    fn verify_path(&self, root: &[u8; 32], mut offset: u64) -> Result<()> {
        let mut current = self.coin.leaf_hash();
        for sibling in &self.hashes {
            let mut hash = Sha512_256::new();
            if offset & 1 == 0 {
                hash.update(current);
                hash.update(sibling);
            } else {
                hash.update(sibling);
                hash.update(current);
            }
            current = hash.finalize().into();
            offset >>= 1;
        }
        if current != *root {
            bail!("coin membership mismatch");
        }
        Ok(())
    }
}

impl TryFrom<WitnessFields> for CoinWitness {
    type Error = anyhow::Error;
    fn try_from(fields: WitnessFields) -> Result<Self> {
        Self::new(fields.coin, fields.proved_at, fields.target, fields.hashes)
    }
}

#[cfg(test)]
mod tests;
