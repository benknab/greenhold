use crate::{connection::utreexod_rpc, context, guest};
use anyhow::{Result, bail};
use greenhold_core::{claim::Claim, network::SupportedNetwork};
use risc0_zkvm::Receipt;

/// Check the receipt's claim against the selected network and this node's chain state.
/// The claim's heights are lookup inputs. Its hashes and digest are only compared.
pub fn claim(
    receipt: &Receipt,
    network: SupportedNetwork,
    rpc: &utreexod_rpc::Config,
) -> Result<Claim> {
    let claim = guest::verify(receipt)?;
    if claim.network() != network {
        bail!("receipt claims {}, not {network}", claim.network());
    }
    context(&claim, rpc)?;
    Ok(claim)
}

/// Rebuild a prepared claim from independently retrieved historical node state.
pub fn context(
    claim: &Claim,
    rpc: &utreexod_rpc::Config,
) -> Result<greenhold_core::context::ChainContext> {
    let input = context::Input::new(
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
