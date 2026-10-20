//! The public statement a receipt proves. It names no coin.

use crate::{
    context::{BlockEndpoint, ChainContext},
    eligibility::MinimumAmount,
    network::SupportedNetwork,
};
use anyhow::{Context as _, Result, bail};
use bitcoin::{
    BlockHash, Network,
    blockdata::constants::genesis_block,
    hashes::{Hash, sha256},
};
use serde::Serialize;

/// Domain and format version of the canonical claim bytes.
const TAG: &[u8] = b"greenhold/claim/v1\0";

/// A block named by height and hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct BlockRef {
    height: u32,
    hash: BlockHash,
}

/// Some eligible coin worth at least M, created by X and unspent at T, exists in
/// the chain state that the context digest names, and its script verifies a signature
/// over this exact claim's signing message. Parsing does not make that true;
/// a verifier rebuilds the expected claim from its own nodes and compares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Claim {
    network: SupportedNetwork,
    first: BlockRef,
    last: BlockRef,
    minimum_sats: MinimumAmount,
    context_digest: sha256::Hash,
}

impl Claim {
    #[must_use]
    pub fn new(context: &ChainContext, minimum: MinimumAmount) -> Self {
        let block = |endpoint: &BlockEndpoint| BlockRef {
            height: endpoint.height(),
            hash: endpoint.hash(),
        };
        Self {
            network: context.supported_network(),
            first: block(context.first()),
            last: block(context.last()),
            minimum_sats: minimum,
            context_digest: context.digest(),
        }
    }

    #[must_use]
    pub fn network(&self) -> SupportedNetwork {
        self.network
    }

    /// Lookup input for X. A verifier fetches its own block at this height.
    #[must_use]
    pub fn first_height(&self) -> u32 {
        self.first.height
    }

    /// Lookup input for T. A verifier fetches its own block at this height.
    #[must_use]
    pub fn last_height(&self) -> u32 {
        self.last.height
    }

    #[must_use]
    pub fn minimum_sats(&self) -> MinimumAmount {
        self.minimum_sats
    }

    /// The exact 80-byte ASCII message to sign: a domain prefix and the lowercase
    /// SHA-256 hex digest of the canonical claim, without a newline or terminator.
    #[must_use]
    pub fn signing_message(&self) -> String {
        format!("greenhold/claim:{}", sha256::Hash::hash(&self.to_bytes()))
    }

    /// Fixed-length canonical bytes: the tag, the network's genesis hash, then for
    /// X and T a LE u32 height and 32-byte block hash, LE u64 M, and the context digest.
    /// Hashes use internal byte order, not reversed display order.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        let genesis = genesis_block(Network::from(self.network)).block_hash();
        bytes.extend_from_slice(genesis.as_byte_array());
        for block in [self.first, self.last] {
            bytes.extend_from_slice(&block.height.to_le_bytes());
            bytes.extend_from_slice(block.hash.as_byte_array());
        }
        bytes.extend_from_slice(&u64::from(self.minimum_sats).to_le_bytes());
        bytes.extend_from_slice(self.context_digest.as_byte_array());
        bytes
    }
}

impl TryFrom<&[u8]> for Claim {
    type Error = anyhow::Error;

    /// Parse canonical bytes. Structure only: the blocks and digest still need a node check.
    fn try_from(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader(
            bytes
                .strip_prefix(TAG)
                .context("claim tag or version is not supported")?,
        );
        let network = SupportedNetwork::from_genesis(BlockHash::from_byte_array(reader.take()?))?;
        let mut block = || -> Result<BlockRef> {
            Ok(BlockRef {
                height: u32::from_le_bytes(reader.take()?),
                hash: BlockHash::from_byte_array(reader.take()?),
            })
        };
        let first = block()?;
        let last = block()?;
        let minimum_sats = MinimumAmount::try_from(u64::from_le_bytes(reader.take()?))?;
        let context_digest = sha256::Hash::from_byte_array(reader.take()?);
        if !reader.0.is_empty() {
            bail!("claim has trailing bytes");
        }
        check_blocks(first, last)?;
        Ok(Self {
            network,
            first,
            last,
            minimum_sats,
            context_digest,
        })
    }
}

/// The same block-order rules a chain context enforces.
fn check_blocks(first: BlockRef, last: BlockRef) -> Result<()> {
    if first.height > last.height {
        bail!("first height must not exceed last height");
    }
    if first.height == last.height && first.hash != last.hash {
        bail!("one height cannot name two blocks");
    }
    Ok(())
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .context("claim is truncated")?;
        self.0 = rest;
        Ok(*head)
    }
}

#[cfg(test)]
mod tests;
