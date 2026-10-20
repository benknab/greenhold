use crate::connection::{electrum, utreexod_rpc};
use anyhow::{Context as _, Result, bail};
use bitcoin::{Address, Network, OutPoint, blockdata::constants::genesis_block};
use std::collections::BTreeSet;

const PAGE_SIZE: u32 = 1_000;
const COIN_BATCH_SIZE: usize = 100;

/// Electrum reports candidates only. Proving must fetch and verify each coin independently.
/// Its unspent list excludes pending spends, even when the coin itself is confirmed.
pub fn electrum_outpoints(
    address: &Address,
    network: Network,
    endpoint: &electrum::Endpoint,
) -> Result<Vec<OutPoint>> {
    let mut client = electrum::Client::connect(endpoint)?;
    client.server_version()?;
    if client.genesis_hash()? != genesis_block(network).block_hash() {
        bail!("Electrum server is not on the requested network ({network})");
    }
    let candidates = client.list_unspent(&address.script_pubkey())?;
    Ok(candidates
        .into_iter()
        .filter(|coin| coin.height > 0)
        .map(|coin| OutPoint::new(coin.tx_hash, coin.tx_pos))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// Discover candidates from this node's address index, without proving membership.
/// Use confirmed chain state only. A changed tip requires a new discovery run.
pub fn outpoints(
    address: &Address,
    network: Network,
    config: &utreexod_rpc::Config,
) -> Result<Vec<OutPoint>> {
    let mut rpc = utreexod_rpc::Client::new(config);
    if rpc.block_hash(0)? != genesis_block(network).block_hash() {
        bail!("RPC node is not on the requested network ({network})");
    }
    let tip = rpc.best_block()?.hash;
    let script = address.script_pubkey();
    let address = address.to_string();
    let mut candidates = BTreeSet::new();
    let mut skip = 0;
    let mut page_size = PAGE_SIZE;
    loop {
        let page = match rpc.search_raw_transactions(&address, skip, page_size, false, false) {
            Err(error) if error.is::<utreexod_rpc::ResponseTooLarge>() && page_size > 1 => {
                // Retry the same offset. Keep the smaller size for the rest of this run.
                page_size /= 2;
                continue;
            }
            result => result.context(
                "cannot search address transactions; node needs --addrindex and RPC access",
            )?,
        };
        let count = u32::try_from(page.len())?;
        // Confirmed history is oldest-first, including transaction order within a block.
        // Remove spends as we go, so gettxout runs only for the surviving candidates.
        for transaction in page.into_iter().filter(|tx| tx.confirmations > 0) {
            for spent in transaction.vin {
                candidates.remove(&spent);
            }
            candidates.extend(
                transaction
                    .vout
                    .into_iter()
                    .filter(|output| output.script_pub_key.hex == script)
                    .map(|output| OutPoint::new(transaction.txid, output.n)),
            );
        }
        if count < page_size {
            break;
        }
        skip = skip
            .checked_add(count)
            .context("address history is too large")?;
    }

    let mut unspent = Vec::new();
    let candidates: Vec<_> = candidates.into_iter().collect();
    for batch in candidates.chunks(COIN_BATCH_SIZE) {
        let outputs = rpc.tx_outs(batch, false)?;
        for (candidate, output) in batch
            .iter()
            .zip(outputs)
            .filter_map(|(candidate, output)| output.map(|output| (candidate, output)))
        {
            if output.best_block != tip {
                bail!("chain tip changed during discovery; run discover again");
            }
            if output.confirmations == 0 || output.script_pub_key.hex != script {
                bail!("RPC coin does not match the confirmed address output");
            }
            unspent.push(*candidate);
        }
    }
    if rpc.best_block()?.hash != tip {
        bail!("chain tip changed during discovery; run discover again");
    }
    Ok(unspent)
}

#[cfg(test)]
mod tests;
