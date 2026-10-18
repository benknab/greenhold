pub mod electrum;
mod json_rpc;
pub mod peer;
pub mod socket;
pub mod utreexod_rpc;

use anyhow::Result;
use bitcoin::hex::DisplayHex;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE: u64 = 64 * 1024;

pub fn status(
    electrum: &electrum::Endpoint,
    peer: &peer::Endpoint,
    rpc: &utreexod_rpc::Config,
) -> Result<Value> {
    let mut electrum = electrum::Client::connect(electrum)?;
    let version = electrum.server_version()?;
    let mut client = utreexod_rpc::Client::new(rpc);
    let hash = client.best_block_hash()?;
    // Bind the roots lookup to this response, not to a second tip lookup.
    let roots = client.utreexo_roots(&hash)?;
    let peer = peer::probe(peer, TIMEOUT)?;
    Ok(json!({
        "electrum": {
            "server_version": version.server_version(),
            "protocol_version": version.protocol_version()
        },
        "utreexod_rpc": {
            "block_hash": hash.to_string(),
            "roots": {
                "numleaves": roots.num_leaves(),
                "roots": roots.roots().iter().map(|root| root.as_hex().to_string()).collect::<Vec<_>>()
            }
        },
        "utreexod": {"protocol_version": peer.version, "start_height": peer.start_height},
    }))
}
