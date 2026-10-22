use crate::connection::{electrum, utreexod_rpc};
use anyhow::{Context as _, Result, bail};
use bitcoin::{Address, BlockHash, Network, OutPoint, blockdata::constants::genesis_block};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    fmt,
};

const PAGE_SIZE: u32 = 1_000;
const COIN_BATCH_SIZE: usize = 100;

/// Node-reported candidate details, without authenticated membership or eligibility.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct CoinSummary {
    outpoint: OutPoint,
    value_sats: u64,
    creation_height: u32,
    creation_hash: BlockHash,
}

impl fmt::Display for CoinSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:<75} {:>16} {:>15} {}",
            self.outpoint.to_string(),
            self.value_sats,
            self.creation_height,
            self.creation_hash
        )
    }
}

/// Electrum reports candidates only. Proving must fetch and verify each coin independently.
/// Its unspent list excludes pending spends, even when the coin itself is confirmed.
pub fn electrum_coins(
    address: &Address,
    network: Network,
    endpoint: &electrum::Endpoint,
) -> Result<Vec<CoinSummary>> {
    let mut client = electrum::Client::connect(endpoint)?;
    client.server_version()?;
    if client.genesis_hash()? != genesis_block(network).block_hash() {
        bail!("Electrum server is not on the requested network ({network})");
    }
    let candidates = client.list_unspent(&address.script_pubkey())?;
    let mut hashes = BTreeMap::new();
    let mut coins = BTreeMap::new();
    for coin in candidates.into_iter().filter(|coin| coin.height > 0) {
        let creation_height = u32::try_from(coin.height)?;
        let creation_hash = cached_hash(&mut hashes, creation_height, |height| {
            Ok(client.block_header(height)?.block_hash())
        })?;
        let outpoint = OutPoint::new(coin.tx_hash, coin.tx_pos);
        let summary = CoinSummary {
            outpoint,
            value_sats: coin.value,
            creation_height,
            creation_hash,
        };
        if coins.insert(outpoint, summary).is_some_and(|previous| {
            previous.value_sats != coin.value || previous.creation_height != creation_height
        }) {
            bail!("Electrum returned conflicting details for the same outpoint");
        }
    }
    Ok(coins.into_values().collect())
}

/// Discover candidates from this node's address index, without proving membership.
/// Use confirmed chain state only. A changed tip requires a new discovery run.
pub fn coins(
    address: &Address,
    network: Network,
    config: &utreexod_rpc::Config,
) -> Result<Vec<CoinSummary>> {
    let mut rpc = utreexod_rpc::Client::new(config);
    if rpc.block_hash(0)? != genesis_block(network).block_hash() {
        bail!("RPC node is not on the requested network ({network})");
    }
    let tip = rpc.best_block()?;
    let script = address.script_pubkey();
    let address = address.to_string();
    let mut candidates = BTreeSet::new();
    let mut skip = 0;
    loop {
        let page = rpc
            .search_raw_transactions(
                &address,
                utreexod_rpc::SearchOptions {
                    skip,
                    count: PAGE_SIZE,
                    input_details: utreexod_rpc::InputDetails::Omit,
                    order: utreexod_rpc::HistoryOrder::OldestFirst,
                },
            )
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
    let mut hashes = BTreeMap::new();
    let candidates: Vec<_> = candidates.into_iter().collect();
    for batch in candidates.chunks(COIN_BATCH_SIZE) {
        let outputs = rpc.tx_outs(batch, false)?;
        for (candidate, output) in batch
            .iter()
            .zip(outputs)
            .filter_map(|(candidate, output)| output.map(|output| (candidate, output)))
        {
            if output.best_block != tip.hash {
                bail!("chain tip changed during discovery; run discover again");
            }
            if output.confirmations == 0 || output.script_pub_key.hex != script {
                bail!("RPC coin does not match the confirmed address output");
            }
            let creation_height = tip
                .height
                .checked_sub(output.confirmations - 1)
                .context("RPC confirmation count exceeds observed height")?;
            let creation_hash = cached_hash(&mut hashes, creation_height, |height| {
                rpc.block_hash(height)
            })?;
            unspent.push(CoinSummary {
                outpoint: *candidate,
                value_sats: output.value.to_sat(),
                creation_height,
                creation_hash,
            });
        }
    }
    if rpc.best_block()?.hash != tip.hash {
        bail!("chain tip changed during discovery; run discover again");
    }
    Ok(unspent)
}

fn cached_hash(
    hashes: &mut BTreeMap<u32, BlockHash>,
    height: u32,
    fetch: impl FnOnce(u32) -> Result<BlockHash>,
) -> Result<BlockHash> {
    Ok(*match hashes.entry(height) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => entry.insert(fetch(height)?),
    })
}

#[cfg(test)]
mod tests;
