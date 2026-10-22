//! The public statement a receipt proves. It names no coin.

use crate::context::{BlockEndpoint, ChainContext};
use crate::eligibility::MinimumAmount;
use crate::network::SupportedNetwork;
use anyhow::{Context as _, Result, bail};
use bitcoin::blockdata::constants;
use bitcoin::hashes::{Hash, sha256};
use bitcoin::{BlockHash, Network};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Domain and format version of the canonical claim bytes.
const TAG: &[u8] = b"greenhold/claim/v2\0";

/// Public challenge bytes, encoded as exactly 64 hexadecimal characters.
/// Text challenges use a domain-separated digest. The default hashes empty text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Nonce([u8; 32]);

impl Nonce {
    /// Hash `greenhold/nonce/v1\0` followed by the exact UTF-8 bytes.
    /// Preserve whitespace and case. Empty text does not provide a fresh challenge.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        use bitcoin::hashes::HashEngine as _;

        let mut engine = sha256::Hash::engine();
        engine.input(b"greenhold/nonce/v1\0");
        engine.input(text.as_bytes());
        Self(sha256::Hash::from_engine(engine).to_byte_array())
    }
}

impl Default for Nonce {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl FromStr for Nonce {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        use bitcoin::hex::FromHex as _;

        Ok(Self(<[u8; 32]>::from_hex(text).context(
            "nonce must contain exactly 64 hexadecimal characters",
        )?))
    }
}

impl TryFrom<String> for Nonce {
    type Error = anyhow::Error;

    fn try_from(text: String) -> Result<Self> {
        text.parse()
    }
}

impl From<Nonce> for String {
    fn from(nonce: Nonce) -> Self {
        nonce.to_string()
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use bitcoin::hex::DisplayHex as _;

        write!(f, "{}", self.0.as_hex())
    }
}

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
    unspent_since: BlockRef,
    unspent_at: BlockRef,
    minimum_sats: MinimumAmount,
    context_digest: sha256::Hash,
    nonce: Nonce,
}

impl Claim {
    #[must_use]
    pub fn new(context: &ChainContext, minimum: MinimumAmount, nonce: Nonce) -> Self {
        let block = |endpoint: &BlockEndpoint| BlockRef {
            height: endpoint.height(),
            hash: endpoint.hash(),
        };
        Self {
            network: context.supported_network(),
            unspent_since: block(context.unspent_since()),
            unspent_at: block(context.unspent_at()),
            minimum_sats: minimum,
            context_digest: context.digest(),
            nonce,
        }
    }

    #[must_use]
    pub fn network(&self) -> SupportedNetwork {
        self.network
    }

    /// Lookup input for `unspent_since` (X). A verifier fetches its own block at this height.
    #[must_use]
    pub fn unspent_since(&self) -> u32 {
        self.unspent_since.height
    }

    /// Lookup input for `unspent_at` (T). A verifier fetches its own block at this height.
    #[must_use]
    pub fn unspent_at(&self) -> u32 {
        self.unspent_at.height
    }

    #[must_use]
    pub fn minimum_sats(&self) -> MinimumAmount {
        self.minimum_sats
    }

    #[must_use]
    pub fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// The exact 80-byte ASCII message to sign: a domain prefix and the lowercase
    /// SHA-256 hex digest of the canonical claim, without a newline or terminator.
    #[must_use]
    pub fn signing_message(&self) -> String {
        format!("greenhold/claim:{}", sha256::Hash::hash(&self.to_bytes()))
    }

    /// Fixed-length canonical bytes: the tag, the network's genesis hash, then for
    /// X and T a LE u32 height and 32-byte block hash, LE u64 M, the context digest,
    /// and the 32-byte nonce.
    /// Hashes use internal byte order, not reversed display order.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        let genesis = constants::genesis_block(Network::from(self.network)).block_hash();
        bytes.extend_from_slice(genesis.as_byte_array());
        for block in [self.unspent_since, self.unspent_at] {
            bytes.extend_from_slice(&block.height.to_le_bytes());
            bytes.extend_from_slice(block.hash.as_byte_array());
        }
        bytes.extend_from_slice(&u64::from(self.minimum_sats).to_le_bytes());
        bytes.extend_from_slice(self.context_digest.as_byte_array());
        bytes.extend_from_slice(&self.nonce.0);
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
        let unspent_since = block()?;
        let unspent_at = block()?;
        let minimum_sats = MinimumAmount::try_from(u64::from_le_bytes(reader.take()?))?;
        let context_digest = sha256::Hash::from_byte_array(reader.take()?);
        let nonce = Nonce(reader.take()?);
        if !reader.0.is_empty() {
            bail!("claim has trailing bytes");
        }
        check_blocks(unspent_since, unspent_at)?;
        Ok(Self {
            network,
            unspent_since,
            unspent_at,
            minimum_sats,
            context_digest,
            nonce,
        })
    }
}

/// The same block-order rules a chain context enforces.
fn check_blocks(unspent_since: BlockRef, unspent_at: BlockRef) -> Result<()> {
    if unspent_since.height > unspent_at.height {
        bail!("unspent_since must not exceed unspent_at");
    }
    if unspent_since.height == unspent_at.height && unspent_since.hash != unspent_at.hash {
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
