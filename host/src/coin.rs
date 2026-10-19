use crate::{connection::utreexod_rpc, context};
use anyhow::{Result, bail};
use bitcoin::OutPoint;
use walledcity_core::{coin::CoinWitness, context::ChainContext};

/// All state is fetched on this run. A proof cannot select a substitute context.
pub fn fetch(
    outpoint: OutPoint,
    request: context::Request,
    config: &utreexod_rpc::Config,
) -> Result<(ChainContext, CoinWitness)> {
    let context = context::fetch(request, config)?;
    let mut rpc = utreexod_rpc::Client::new(config);
    let output = rpc.unspent_output(outpoint)?;
    let observed = rpc.block_location(output.observed_at())?;
    let creation_height = output.creation_height(observed.height())?;
    let creation_hash = rpc.block_hash(creation_height)?;
    let coin = output.into_coin(outpoint, creation_hash, creation_height)?;
    let witness = rpc.inclusion_proof(outpoint)?.bind(coin)?;
    witness.verify(&context, outpoint)?;
    for (height, hash) in [
        (creation_height, creation_hash),
        (context.first().height(), context.first().hash()),
        (context.last().height(), context.last().hash()),
    ] {
        if rpc.block_hash(height)? != hash {
            bail!("RPC chain changed during coin retrieval");
        }
    }
    Ok((context, witness))
}

#[cfg(test)]
mod tests;
