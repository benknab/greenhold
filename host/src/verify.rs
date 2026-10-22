use crate::{connection::utreexod_rpc, context, guest};
use anyhow::{Result, bail};
use greenhold_core::{
    claim::{Claim, Nonce},
    network::SupportedNetwork,
};
use risc0_zkvm::Receipt;

/// Check the receipt's claim against the network, expected nonce, and node state.
/// The claim's heights are lookup inputs. Its hashes and digest are only compared.
pub fn claim(
    receipt: &Receipt,
    network: SupportedNetwork,
    nonce: Nonce,
    rpc: &utreexod_rpc::Config,
) -> Result<Claim> {
    let claim = guest::verify(receipt)?;
    check_expected(&claim, network, nonce)?;
    context(&claim, rpc)?;
    Ok(claim)
}

fn check_expected(claim: &Claim, network: SupportedNetwork, nonce: Nonce) -> Result<()> {
    if claim.network() != network {
        bail!("receipt claims {}, not {network}", claim.network());
    }
    if claim.nonce() != nonce {
        bail!("receipt nonce does not match the expected nonce");
    }
    Ok(())
}

/// Rebuild a prepared claim from independently retrieved historical node state.
pub fn context(
    claim: &Claim,
    rpc: &utreexod_rpc::Config,
) -> Result<greenhold_core::context::ChainContext> {
    let input = context::Input::try_new(
        claim.network().into(),
        Some(claim.unspent_since()),
        Some(claim.unspent_at()),
    )?;
    let context = context::fetch(input, rpc)?;
    if Claim::new(&context, claim.minimum_sats(), claim.nonce()) != *claim {
        bail!("claim does not match the node's chain state");
    }
    Ok(context)
}

#[cfg(test)]
mod tests;
