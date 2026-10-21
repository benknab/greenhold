use anyhow::Result;
use bitcoin::{
    Amount, BlockHash, Network, OutPoint, TxOut, Txid, blockdata::constants::genesis_block,
    hashes::Hash, secp256k1::SecretKey,
};
use greenhold_core::{
    claim::Claim,
    coin::{Coin, CoinWitness},
    context::{AccumulatorState, BlockEndpoint, ChainContext},
    eligibility::MinimumAmount,
    input::ClaimInput,
};
use greenhold_signer::{Format, ScriptType, address, sign};

pub const VALUE: u64 = 12_345;

/// Synthetic chain data, for guest and CLI boundaries only.
pub fn signed(script: ScriptType, format: Format) -> Result<(ClaimInput, Claim)> {
    let key = SecretKey::from_slice(&[1; 32])?;
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: address(script, &key, Network::Regtest).script_pubkey(),
        },
    )?;
    let header = genesis_block(Network::Regtest).header;
    let context = ChainContext::new(
        BlockEndpoint::new(Network::Regtest, 10, header)?,
        BlockEndpoint::new(Network::Regtest, 20, header)?,
        AccumulatorState::new(1, vec![coin.leaf_hash()])?,
    )?;
    let witness = CoinWitness::new(coin, context.unspent_at().hash(), 0, vec![])?;
    let minimum = MinimumAmount::try_from(VALUE)?;
    let nonce = "ab".repeat(32).parse()?;
    let claim = Claim::new(&context, minimum, nonce);
    let signature = sign(
        script,
        format,
        &key,
        Network::Regtest,
        &claim.signing_message(),
    )?
    .signature
    .parse()?;
    Ok((
        ClaimInput::new(context, witness, minimum, nonce, signature),
        claim,
    ))
}
