use crate::connection::{electrum, utreexod_rpc};
use anyhow::{Context as _, Result, bail};
use bitcoin::{Network, blockdata::constants::genesis_block};
use walledcity_core::context::{AccumulatorState, BlockEndpoint, ChainContext};
use walledcity_core::network::SupportedNetwork;

/// A fixed pair of lookup heights, never an implicit moving tip.
#[derive(Clone, Copy, Debug)]
pub struct Request {
    network: SupportedNetwork,
    first: u32,
    last: u32,
}

impl Request {
    pub fn new(network: Network, first: u32, last: u32) -> Result<Self> {
        if first > last {
            bail!("first height must not exceed last height");
        }
        Ok(Self {
            network: network.try_into()?,
            first,
            last,
        })
    }
}

/// Retrieve public state from the caller's chosen stock nodes on this run.
/// Electrum checks the chain endpoints; the RPC node supplies the roots bound
/// to the last block. This function never accepts a saved context as authority.
pub fn fetch(
    request: Request,
    endpoint: &electrum::Endpoint,
    config: &utreexod_rpc::Config,
) -> Result<ChainContext> {
    let mut rpc = utreexod_rpc::Client::new(config);
    let mut chain = electrum::Client::connect(endpoint)?;
    chain.server_version()?;
    let genesis = genesis_block(Network::from(request.network)).block_hash();
    if rpc.block_hash(0)? != genesis {
        bail!(
            "RPC node is not on the requested network ({})",
            request.network
        );
    }
    if chain.genesis_hash()? != genesis {
        bail!(
            "Electrum node is not on the requested network ({})",
            request.network
        );
    }

    let first = endpoint_block(&mut rpc, request.network.into(), request.first)?;
    let last = if request.first == request.last {
        first.clone()
    } else {
        endpoint_block(&mut rpc, request.network.into(), request.last)?
    };
    let roots = rpc.utreexo_roots(&last.hash()).with_context(|| {
        format!(
            "cannot retrieve Utreexo state at height {} ({})",
            last.height(),
            last.hash()
        )
    })?;
    let state = AccumulatorState::new(roots.num_leaves(), roots.roots().to_vec())?;

    // Read the height bindings again after fetching roots. Tip advances are fine;
    // an observed reorg of either requested endpoint is not.
    crosscheck(&mut rpc, &mut chain, &first)?;
    if request.first != request.last {
        crosscheck(&mut rpc, &mut chain, &last)?;
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

fn crosscheck(
    rpc: &mut utreexod_rpc::Client<'_>,
    chain: &mut electrum::Client,
    block: &BlockEndpoint,
) -> Result<()> {
    if rpc.block_hash(block.height())? != block.hash() {
        bail!(
            "RPC chain changed at height {} during context retrieval",
            block.height()
        );
    }
    let header = chain.block_header(block.height()).with_context(|| {
        format!(
            "cannot retrieve Electrum header at height {}",
            block.height()
        )
    })?;
    if header != *block.header() {
        bail!(
            "RPC and Electrum disagree on the header at height {}",
            block.height()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
