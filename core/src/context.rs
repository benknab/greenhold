//! Public Bitcoin chain context. Parsing establishes structure, not chain acceptance.
//! A verifier must retrieve these values from its own chosen nodes.

use crate::network::SupportedNetwork;
use anyhow::{Result, bail};
use bitcoin::{
    BlockHash, Network,
    block::Header,
    blockdata::constants::genesis_block,
    consensus::{Params, serialize},
    hashes::{Hash, HashEngine, sha256},
};
use serde::{Deserialize, Serialize};

/// A network, block height, and header with valid network-range proof of work.
/// The height and chain membership still require a trusted node lookup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BlockEndpointFields")]
pub struct BlockEndpoint {
    network: SupportedNetwork,
    height: u32,
    header: Header,
}

#[derive(Deserialize)]
struct BlockEndpointFields {
    network: SupportedNetwork,
    height: u32,
    header: Header,
}

impl BlockEndpoint {
    /// Parse a block endpoint for the selected network.
    ///
    /// # Errors
    /// Rejects unsupported networks, invalid proof of work, targets above the network's
    /// limit, and a wrong genesis.
    pub fn new(network: Network, height: u32, header: Header) -> Result<Self> {
        let supported = SupportedNetwork::try_from(network)?;
        if header.target() > Params::new(network).max_attainable_target {
            bail!("header target exceeds {network} proof-of-work limit");
        }
        header.validate_pow(header.target())?;
        if height == 0 && header != genesis_block(network).header {
            bail!("height zero must be {network} genesis");
        }
        Ok(Self {
            network: supported,
            height,
            header,
        })
    }

    #[must_use]
    pub fn network(&self) -> Network {
        self.network.into()
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    #[must_use]
    pub fn hash(&self) -> BlockHash {
        self.header.block_hash()
    }
}

impl TryFrom<BlockEndpointFields> for BlockEndpoint {
    type Error = anyhow::Error;

    fn try_from(fields: BlockEndpointFields) -> Result<Self> {
        Self::new(fields.network.into(), fields.height, fields.header)
    }
}

/// Accumulator roots in node order. Root bytes are never block-hash reversed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AccumulatorStateFields")]
pub struct AccumulatorState {
    num_leaves: u64,
    roots: Vec<[u8; 32]>,
}

#[derive(Deserialize)]
struct AccumulatorStateFields {
    num_leaves: u64,
    roots: Vec<[u8; 32]>,
}

impl AccumulatorState {
    /// Parse a bounded Utreexo forest. An empty forest has no roots.
    ///
    /// # Errors
    /// Rejects leaf counts outside the supported position range and wrong root counts.
    pub fn new(num_leaves: u64, roots: Vec<[u8; 32]>) -> Result<Self> {
        if num_leaves >= 1 << 62 {
            bail!("Utreexo leaf count exceeds supported position range");
        }
        if roots.len() != usize::try_from(num_leaves.count_ones())? {
            bail!("Utreexo root count does not match leaf count");
        }
        Ok(Self { num_leaves, roots })
    }

    #[must_use]
    pub fn num_leaves(&self) -> u64 {
        self.num_leaves
    }

    #[must_use]
    pub fn roots(&self) -> &[[u8; 32]] {
        &self.roots
    }
}

impl TryFrom<AccumulatorStateFields> for AccumulatorState {
    type Error = anyhow::Error;

    fn try_from(fields: AccumulatorStateFields) -> Result<Self> {
        Self::new(fields.num_leaves, fields.roots)
    }
}

/// Two endpoints on one network and the accumulator state at the last endpoint.
/// Construction does not authenticate the state or establish ancestry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ContextFields")]
pub struct ChainContext {
    first: BlockEndpoint,
    last: BlockEndpoint,
    state: AccumulatorState,
}

#[derive(Deserialize)]
struct ContextFields {
    first: BlockEndpoint,
    last: BlockEndpoint,
    state: AccumulatorState,
}

impl ChainContext {
    /// Bind parsed endpoints and the state supplied for the last block.
    ///
    /// # Errors
    /// Rejects mixed networks, reversed heights, or different headers at the same height.
    pub fn new(first: BlockEndpoint, last: BlockEndpoint, state: AccumulatorState) -> Result<Self> {
        if first.network != last.network {
            bail!("context endpoints must use the same network");
        }
        if first.height > last.height {
            bail!("first height must not exceed last height");
        }
        if first.height == last.height && first != last {
            bail!("one height cannot name two blocks");
        }
        Ok(Self { first, last, state })
    }

    #[must_use]
    pub fn network(&self) -> Network {
        self.first.network()
    }

    #[must_use]
    pub fn first(&self) -> &BlockEndpoint {
        &self.first
    }

    #[must_use]
    pub fn last(&self) -> &BlockEndpoint {
        &self.last
    }

    #[must_use]
    pub fn state(&self) -> &AccumulatorState {
        &self.state
    }

    /// SHA-256 over the domain, the network's consensus-encoded genesis hash,
    /// then both (LE u32 height, 80-byte consensus header)
    /// pairs, LE u64 leaf count, LE u32 root count, and ordered 32-byte roots.
    /// This binds public values; it does not make supplied roots trusted.
    #[must_use]
    pub fn digest(&self) -> sha256::Hash {
        let mut engine = sha256::Hash::engine();
        engine.input(b"greenhold/chain-context/v1\0");
        engine.input(&serialize(&genesis_block(self.network()).block_hash()));
        for block in [&self.first, &self.last] {
            engine.input(&block.height.to_le_bytes());
            engine.input(&serialize(&block.header));
        }
        engine.input(&self.state.num_leaves.to_le_bytes());
        engine.input(&self.state.num_leaves.count_ones().to_le_bytes());
        for root in &self.state.roots {
            engine.input(root);
        }
        sha256::Hash::from_engine(engine)
    }
}

impl TryFrom<ContextFields> for ChainContext {
    type Error = anyhow::Error;

    fn try_from(fields: ContextFields) -> Result<Self> {
        Self::new(fields.first, fields.last, fields.state)
    }
}
