use crate::connection::utreexod_rpc;
use anyhow::{Context as _, Result, bail};
use bitcoin::{BlockHash, Network, block::Header, blockdata::constants::genesis_block};
use greenhold_core::context::{AccumulatorState, BlockEndpoint, ChainContext};
use greenhold_core::network::SupportedNetwork;

/// Lookup heights. An omitted last height uses the selected block or the RPC tip.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    network: SupportedNetwork,
    first: Option<u32>,
    last: Option<u32>,
}

impl Input {
    pub fn new(network: Network, first: Option<u32>, last: Option<u32>) -> Result<Self> {
        if let (Some(first), Some(last)) = (first, last)
            && first > last
        {
            bail!("first height must not exceed last height");
        }
        Ok(Self {
            network: network.try_into()?,
            first,
            last,
        })
    }
}

/// Retrieve public state from the caller's chosen Utreexod node on this run.
/// The RPC node supplies headers, height bindings, and roots at the last block.
/// This function never accepts a saved context as authority.
pub fn fetch(input: Input, config: &utreexod_rpc::Config) -> Result<ChainContext> {
    fetch_selected(input, config, None)
}

/// Retrieve context for an exact proof block, treating its hash only as a lookup input.
/// The node supplies its height, header, and roots. Later tip changes do not replace it.
/// An omitted first height defaults to this block.
pub fn fetch_at(
    network: SupportedNetwork,
    first: Option<u32>,
    config: &utreexod_rpc::Config,
    block: BlockHash,
) -> Result<ChainContext> {
    let input = Input {
        network,
        first,
        last: None,
    };
    fetch_selected(input, config, Some(block))
}

fn fetch_selected(
    input: Input,
    config: &utreexod_rpc::Config,
    selected: Option<BlockHash>,
) -> Result<ChainContext> {
    let mut rpc = utreexod_rpc::Client::new(config);
    let genesis = genesis_block(Network::from(input.network)).block_hash();
    if rpc.block_hash(0)? != genesis {
        bail!(
            "RPC node is not on the requested network ({})",
            input.network
        );
    }
    // Never replace a selected block with a later tip during retrieval.
    let (last_height, pinned) = if let Some(hash) = selected {
        let block = selected_endpoint(&mut rpc, input.network, hash)?;
        (block.height(), Some(block))
    } else if let Some(height) = input.last {
        (height, None)
    } else {
        let block = rpc
            .best_block()
            .context("cannot resolve Utreexod RPC tip")?;
        let height = block.height;
        let header =
            block_header(&mut rpc, &block.hash).context("cannot retrieve RPC tip header")?;
        (
            height,
            Some(BlockEndpoint::new(input.network.into(), height, header)?),
        )
    };
    let first_height = input.first.unwrap_or(last_height);
    if first_height > last_height {
        bail!("first height must not exceed last height");
    }
    let first = if let Some(block) = &pinned
        && first_height == last_height
    {
        block.clone()
    } else {
        endpoint_block(&mut rpc, input.network.into(), first_height)?
    };
    let last = if first_height == last_height {
        first.clone()
    } else if let Some(block) = pinned {
        block
    } else {
        endpoint_block(&mut rpc, input.network.into(), last_height)?
    };
    let roots = rpc.utreexo_roots(&last.hash()).with_context(|| {
        format!(
            "cannot retrieve Utreexo state at height {} ({})",
            last.height(),
            last.hash()
        )
    })?;
    let state = AccumulatorState::new(roots.num_leaves, roots.roots)?;

    ChainContext::new(first, last, state)
}

fn selected_endpoint(
    rpc: &mut utreexod_rpc::Client<'_>,
    network: SupportedNetwork,
    hash: BlockHash,
) -> Result<BlockEndpoint> {
    let utreexod_rpc::BlockHeaderResponse::Verbose(location) = rpc.block_header(&hash, true)?
    else {
        bail!("expected a verbose RPC block header");
    };
    if location.hash != hash {
        bail!("RPC block location does not match requested hash");
    }
    BlockEndpoint::new(network.into(), location.height, block_header(rpc, &hash)?)
}

fn endpoint_block(
    rpc: &mut utreexod_rpc::Client<'_>,
    network: Network,
    height: u32,
) -> Result<BlockEndpoint> {
    let hash = rpc
        .block_hash(height)
        .with_context(|| format!("cannot resolve RPC block at height {height}"))?;
    let header = block_header(rpc, &hash)
        .with_context(|| format!("cannot retrieve RPC header at height {height}"))?;
    BlockEndpoint::new(network, height, header)
}

fn block_header(rpc: &mut utreexod_rpc::Client<'_>, hash: &BlockHash) -> Result<Header> {
    let utreexod_rpc::BlockHeaderResponse::Raw(header) = rpc.block_header(hash, false)? else {
        bail!("expected a raw RPC block header");
    };
    if header.block_hash() != *hash {
        bail!("RPC header does not match requested block hash");
    }
    Ok(header)
}

#[cfg(test)]
mod tests;
