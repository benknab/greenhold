use crate::connection::utreexod_rpc;
use anyhow::{Context as _, Result, bail};
use bitcoin::{Network, blockdata::constants::genesis_block};
use walledcity_core::context::{AccumulatorState, BlockEndpoint, ChainContext};
use walledcity_core::network::SupportedNetwork;

/// Lookup heights. An omitted last height selects the RPC tip once during retrieval.
#[derive(Clone, Copy, Debug)]
pub struct Request {
    network: SupportedNetwork,
    first: Option<u32>,
    last: Option<u32>,
}

impl Request {
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

/// Retrieve public state from the caller's chosen stock Utreexod node on this run.
/// The RPC node supplies headers, height bindings, and roots at the last block.
/// This function never accepts a saved context as authority.
pub fn fetch(request: Request, config: &utreexod_rpc::Config) -> Result<ChainContext> {
    let mut rpc = utreexod_rpc::Client::new(config);
    let genesis = genesis_block(Network::from(request.network)).block_hash();
    if rpc.block_hash(0)? != genesis {
        bail!(
            "RPC node is not on the requested network ({})",
            request.network
        );
    }
    // Pin the tip's hash and height from one RPC snapshot. Never replace it with a
    // later tip while retrieving this context.
    let (last_height, tip) = if let Some(height) = request.last {
        (height, None)
    } else {
        let block = rpc
            .best_block()
            .context("cannot resolve Utreexod RPC tip")?;
        let height = block.height;
        let header = rpc
            .block_header(&block.hash)
            .context("cannot retrieve RPC tip header")?;
        (
            height,
            Some(BlockEndpoint::new(request.network.into(), height, header)?),
        )
    };
    let first_height = request.first.unwrap_or(last_height);
    if first_height > last_height {
        bail!("first height must not exceed last height");
    }
    let first = if let Some(tip) = &tip
        && first_height == last_height
    {
        tip.clone()
    } else {
        endpoint_block(&mut rpc, request.network.into(), first_height)?
    };
    let last = if first_height == last_height {
        first.clone()
    } else if let Some(tip) = tip {
        tip
    } else {
        endpoint_block(&mut rpc, request.network.into(), last_height)?
    };
    let roots = rpc.utreexo_roots(&last.hash()).with_context(|| {
        format!(
            "cannot retrieve Utreexo state at height {} ({})",
            last.height(),
            last.hash()
        )
    })?;
    let state = AccumulatorState::new(roots.num_leaves, roots.roots)?;

    // Read the height bindings again after fetching roots. Tip advances are fine;
    // an observed reorg of either requested endpoint is not.
    crosscheck(&mut rpc, &first)?;
    if first.height() != last.height() {
        crosscheck(&mut rpc, &last)?;
    }
    ChainContext::new(first, last, state)
}

fn endpoint_block(
    rpc: &mut utreexod_rpc::Client<'_>,
    network: Network,
    height: u32,
) -> Result<BlockEndpoint> {
    let hash = rpc
        .block_hash(height)
        .with_context(|| format!("cannot resolve RPC block at height {height}"))?;
    let header = rpc
        .block_header(&hash)
        .with_context(|| format!("cannot retrieve RPC header at height {height}"))?;
    BlockEndpoint::new(network, height, header)
}

fn crosscheck(rpc: &mut utreexod_rpc::Client<'_>, block: &BlockEndpoint) -> Result<()> {
    if rpc.block_hash(block.height())? != block.hash() {
        bail!(
            "RPC chain changed at height {} during context retrieval",
            block.height()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
