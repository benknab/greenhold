use crate::connection::utreexod_rpc;
use anyhow::{Context as _, Result, bail};
use bitcoin::{Address, Network, OutPoint, blockdata::constants::genesis_block};
use std::collections::BTreeSet;

const PAGE_SIZE: u32 = 100;

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
    loop {
        let page = rpc
            .search_raw_transactions(&address, skip, PAGE_SIZE, false, false)
            .context("cannot search address transactions; node needs --addrindex and RPC access")?;
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
        if count < PAGE_SIZE {
            break;
        }
        skip = skip
            .checked_add(count)
            .context("address history is too large")?;
    }

    let mut unspent = Vec::new();
    for candidate in candidates {
        if let Some(output) = rpc.tx_out(&candidate.txid, candidate.vout, false)? {
            if output.best_block != tip {
                bail!("chain tip changed during discovery; run discover again");
            }
            if output.confirmations == 0 || output.script_pub_key.hex != script {
                bail!("RPC coin does not match the confirmed address output");
            }
            unspent.push(candidate);
        }
    }
    if rpc.best_block()?.hash != tip {
        bail!("chain tip changed during discovery; run discover again");
    }
    Ok(unspent)
}

#[cfg(test)]
mod tests;
