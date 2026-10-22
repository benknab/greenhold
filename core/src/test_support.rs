//! Shared unit-test fixtures. Synthetic chain data, never accepted chain state.

use crate::{
    coin::{Coin, CoinWitness},
    context::{AccumulatorState, BlockEndpoint, ChainContext},
};
use anyhow::Result;
use bitcoin::{
    Amount, BlockHash, Network, OutPoint, ScriptBuf, TxOut, Txid, blockdata::constants,
    hashes::Hash,
};

pub const VALUE: u64 = 12_345;

#[path = "../tests/fixtures/signatures.rs"]
pub mod signatures;

pub fn coin(creation_height: u32, coinbase: bool) -> Result<Coin> {
    Coin::try_new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        creation_height,
        coinbase,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
        },
    )
}

/// A one-leaf forest holding `coin` at `unspent_at` (T).
pub fn proved(
    coin: Coin,
    unspent_since: u32,
    unspent_at: u32,
) -> Result<(CoinWitness, ChainContext)> {
    let header = constants::genesis_block(Network::Regtest).header;
    let context = ChainContext::try_new(
        BlockEndpoint::try_new(Network::Regtest, unspent_since, header)?,
        BlockEndpoint::try_new(Network::Regtest, unspent_at, header)?,
        AccumulatorState::try_new(1, vec![coin.leaf_hash()])?,
    )?;
    let witness = CoinWitness::try_new(coin, context.unspent_at().hash(), 0, vec![])?;
    Ok((witness, context))
}
