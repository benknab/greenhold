use anyhow::Result;
use bitcoin::blockdata::constants;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::SecretKey;
use bitcoin::{Amount, BlockHash, Network, OutPoint, TxOut, Txid};
use greenhold_core::claim::Claim;
use greenhold_core::coin::{Coin, CoinWitness};
use greenhold_core::context::{AccumulatorState, BlockEndpoint, ChainContext};
use greenhold_core::eligibility::MinimumAmount;
use greenhold_core::input::ClaimInput;
use greenhold_signer::{Format, ScriptType};

pub const VALUE: u64 = 12_345;

/// Synthetic chain data, for guest and CLI boundaries only.
pub fn signed(script: ScriptType, format: Format) -> Result<(ClaimInput, Claim)> {
    let key = SecretKey::from_slice(&[1; 32])?;
    let coin = Coin::try_new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: greenhold_signer::address(script, &key, Network::Regtest)
                .script_pubkey(),
        },
    )?;
    let header = constants::genesis_block(Network::Regtest).header;
    let context = ChainContext::try_new(
        BlockEndpoint::try_new(Network::Regtest, 10, header)?,
        BlockEndpoint::try_new(Network::Regtest, 20, header)?,
        AccumulatorState::try_new(1, vec![coin.leaf_hash()])?,
    )?;
    let witness = CoinWitness::try_new(coin, context.unspent_at().hash(), 0, vec![])?;
    let minimum = MinimumAmount::try_from(VALUE)?;
    let nonce = "ab".repeat(32).parse()?;
    let claim = Claim::new(&context, minimum, nonce);
    let signature = greenhold_signer::sign(
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
