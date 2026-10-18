pub mod electrum;
pub mod peer;
pub mod socket;
pub mod utreexod_rpc;

use anyhow::{Context, Result, bail};
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

fn result(reply: &Value, id: u64) -> Result<Value> {
    if reply.get("id") != Some(&json!(id)) || !reply["error"].is_null() {
        bail!("node rejected request or returned a mismatched response ID");
    }
    reply.get("result").cloned().context("missing node result")
}
