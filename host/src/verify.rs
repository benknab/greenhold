use crate::{connection::utreexod_rpc, context, guest};
use anyhow::{Result, bail};
use greenhold_core::{claim::Claim, network::SupportedNetwork};
use risc0_zkvm::Receipt;

/// Accept a receipt's claim only if this node's chain state rebuilds the same claim.
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
    let input = context::Input::new(
        network.into(),
        Some(claim.first_height()),
        Some(claim.last_height()),
    )?;
    let context = context::fetch(input, rpc)?;
    if Claim::new(&context, claim.minimum_sats()) != claim {
        bail!("receipt claim does not match the node's chain state");
    }
    Ok(claim)
}
