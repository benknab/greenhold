use crate::connection::utreexod_rpc;
use anyhow::{Context as _, Result, bail};
use bitcoin::{BlockHash, Network, block::Header, blockdata::constants};
use greenhold_core::context::{AccumulatorState, BlockEndpoint, ChainContext};
use greenhold_core::network::SupportedNetwork;

/// Lookup heights. An omitted `unspent_at` uses the selected block or the RPC tip.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    network: SupportedNetwork,
    unspent_since: Option<u32>,
    unspent_at: Option<u32>,
}

impl Input {
    pub fn new(
        network: Network,
        unspent_since: Option<u32>,
        unspent_at: Option<u32>,
    ) -> Result<Self> {
        if let (Some(unspent_since), Some(unspent_at)) = (unspent_since, unspent_at)
            && unspent_since > unspent_at
        {
            bail!("unspent_since must not exceed unspent_at");
        }
        Ok(Self {
            network: network.try_into()?,
            unspent_since,
            unspent_at,
        })
    }
}

/// Retrieve public state from the caller's chosen Utreexod node on this run.
/// The RPC node supplies headers, height bindings, and roots at `unspent_at`.
/// This function never accepts a saved context as authority.
pub fn fetch(input: Input, config: &utreexod_rpc::Config) -> Result<ChainContext> {
    fetch_selected(input, config, None)
}

/// Retrieve context for an exact proof block, treating its hash only as a lookup input.
/// The node supplies its height, header, and roots. Later tip changes do not replace it.
/// An omitted `unspent_since` defaults to this block's height.
pub fn fetch_at(
    network: SupportedNetwork,
    unspent_since: Option<u32>,
    config: &utreexod_rpc::Config,
    block: BlockHash,
) -> Result<ChainContext> {
    let input = Input {
        network,
        unspent_since,
        unspent_at: None,
    };
    fetch_selected(input, config, Some(block))
}

fn fetch_selected(
    input: Input,
    config: &utreexod_rpc::Config,
    selected: Option<BlockHash>,
) -> Result<ChainContext> {
    let mut rpc = utreexod_rpc::Client::new(config);
    let genesis = constants::genesis_block(Network::from(input.network)).block_hash();
    if rpc.block_hash(0)? != genesis {
        bail!(
            "RPC node is not on the requested network ({})",
            input.network
        );
    }
    // Never replace a selected block with a later tip during retrieval.
    let (unspent_at, pinned) = if let Some(hash) = selected {
        let block = selected_endpoint(&mut rpc, input.network, hash)?;
        (block.height(), Some(block))
    } else if let Some(height) = input.unspent_at {
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
    let unspent_since = input.unspent_since.unwrap_or(unspent_at);
    if unspent_since > unspent_at {
        bail!("unspent_since must not exceed unspent_at");
    }
    let since_block = if let Some(block) = &pinned
        && unspent_since == unspent_at
    {
        block.clone()
    } else {
        endpoint_block(&mut rpc, input.network.into(), unspent_since)?
    };
    let at_block = if unspent_since == unspent_at {
        since_block.clone()
    } else if let Some(block) = pinned {
        block
    } else {
        endpoint_block(&mut rpc, input.network.into(), unspent_at)?
    };
    let roots = rpc.utreexo_roots(&at_block.hash()).with_context(|| {
        format!(
            "cannot retrieve Utreexo state at height {} ({})",
            at_block.height(),
            at_block.hash()
        )
    })?;
    let state = AccumulatorState::new(roots.num_leaves, roots.roots)?;

    ChainContext::new(since_block, at_block, state)
}

fn selected_endpoint(
    rpc: &mut utreexod_rpc::Client<'_>,
    network: SupportedNetwork,
    hash: BlockHash,
) -> Result<BlockEndpoint> {
    let location = rpc.block_location(&hash)?;
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
    let header = rpc.block_header(hash)?;
    if header.block_hash() != *hash {
        bail!("RPC header does not match requested block hash");
    }
    Ok(header)
}

#[cfg(test)]
mod tests;
