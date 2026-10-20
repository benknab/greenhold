//! Shared unit-test fixtures. Synthetic chain data, never accepted chain state.

use crate::{
    coin::{Coin, CoinWitness},
    context::{AccumulatorState, BlockEndpoint, ChainContext},
};
use anyhow::Result;
use bitcoin::{
    Amount, BlockHash, Network, OutPoint, ScriptBuf, TxOut, Txid,
    blockdata::constants::genesis_block, hashes::Hash,
};

pub const VALUE: u64 = 12_345;

#[path = "../tests/fixtures/signatures.rs"]
pub mod signatures;

pub fn coin(creation_height: u32, coinbase: bool) -> Result<Coin> {
    Coin::new(
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

/// A one-leaf forest holding `coin` at the last block T.
pub fn proved(coin: Coin, first: u32, last: u32) -> Result<(CoinWitness, ChainContext)> {
    let header = genesis_block(Network::Regtest).header;
    let context = ChainContext::new(
        BlockEndpoint::new(Network::Regtest, first, header)?,
        BlockEndpoint::new(Network::Regtest, last, header)?,
        AccumulatorState::new(1, vec![coin.leaf_hash()])?,
    )?;
    let witness = CoinWitness::new(coin, context.last().hash(), 0, vec![])?;
    Ok((witness, context))
}
