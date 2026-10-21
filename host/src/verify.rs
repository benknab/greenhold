use crate::{connection::utreexod_rpc, context, guest};
use anyhow::{Result, bail};
use greenhold_core::{claim::Claim, network::SupportedNetwork};
use risc0_zkvm::Receipt;

/// Check the receipt's claim against this node and an optional independent expectation.
/// The claim's heights are lookup inputs. Its hashes and digest are only compared.
pub fn claim(
    receipt: &Receipt,
    network: SupportedNetwork,
    expected: Option<&Claim>,
    rpc: &utreexod_rpc::Config,
) -> Result<Claim> {
    let claim = guest::verify(receipt)?;
    if claim.network() != network {
        bail!("receipt claims {}, not {network}", claim.network());
    }
    if expected.is_some_and(|expected| &claim != expected) {
        bail!("receipt does not match the expected claim");
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
        Some(claim.first_height()),
        Some(claim.last_height()),
    )?;
    let context = context::fetch(input, rpc)?;
    if Claim::new(&context, claim.minimum_sats(), claim.nonce()) != *claim {
        bail!("claim does not match the node's chain state");
    }
    Ok(context)
}
