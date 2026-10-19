use crate::{connection::utreexod_rpc, context};
use anyhow::{Context, Result, bail};
use bitcoin::{OutPoint, TxOut, hashes::Hash};
use greenhold_core::{
    coin::{Coin, CoinWitness},
    context::ChainContext,
    eligibility::{EligibleCoin, MinimumAmount},
};

/// Fetch a tip proof before its exact public context. The proof's block hash is a
/// lookup input; the node supplies and checks the context on this run.
pub fn fetch(
    outpoint: OutPoint,
    input: context::Input,
    minimum: MinimumAmount,
    config: &utreexod_rpc::Config,
) -> Result<(ChainContext, EligibleCoin)> {
    let mut rpc = utreexod_rpc::Client::new(config);
    let output = rpc
        .tx_out(&outpoint.txid, outpoint.vout, false)?
        .context("coin is absent from the confirmed UTXO set")?;
    let utreexod_rpc::BlockHeaderResponse::Verbose(observed) =
        rpc.block_header(&output.best_block, true)?
    else {
        bail!("expected a verbose RPC block header");
    };
    if observed.hash != output.best_block {
        bail!("RPC block location does not match requested hash");
    }
    let creation_height = creation_height(output.confirmations, observed.height)?;
    let creation_hash = rpc.block_hash(creation_height)?;
    let coin = Coin::new(
        outpoint,
        creation_hash,
        creation_height,
        output.coinbase,
        TxOut {
            value: output.value,
            script_pubkey: output.script_pub_key.hex,
        },
    )?;
    let proof = rpc.prove_utxo_chain_tip_inclusion(&[outpoint.txid], &[outpoint.vout]).context(
        "cannot retrieve coin membership proof; coin must be unspent and node needs a proof index",
    )?;
    let proved_at = proof.proved_at_hash;
    let witness = bind(coin, proof)?;
    let context = context::fetch_at(input, config, proved_at)?;
    let coin = EligibleCoin::new(witness, &context, minimum)?;
    Ok((context, coin))
}

fn creation_height(confirmations: u32, observed_height: u32) -> Result<u32> {
    let age = confirmations
        .checked_sub(1)
        .context("coin is not confirmed")?;
    observed_height
        .checked_sub(age)
        .context("RPC confirmation count exceeds observed height")
}

fn bind(coin: Coin, proof: utreexod_rpc::ChainTipProof) -> Result<CoinWitness> {
    let [target] = proof.proof_targets.as_slice() else {
        bail!("expected one proof target");
    };
    let [leaf] = proof.hashes_proven.as_slice() else {
        bail!("expected one proven leaf");
    };
    if leaf.to_byte_array() != coin.leaf_hash() {
        bail!("RPC proven leaf does not match reconstructed coin");
    }
    CoinWitness::new(
        coin,
        proof.proved_at_hash,
        *target,
        proof
            .proof_hashes
            .into_iter()
            .map(Hash::to_byte_array)
            .collect(),
    )
}

#[cfg(test)]
mod tests;
