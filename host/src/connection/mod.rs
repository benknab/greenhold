pub mod electrum;
pub mod peer;
pub mod rpc;
pub mod socket;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE: u64 = 64 * 1024;

pub fn status(
    electrum: &electrum::ElectrumEndpoint,
    peer: &peer::P2pEndpoint,
    rpc: &rpc::RpcConfig,
) -> Result<Value> {
    let mut electrum = electrum::Electrum::connect(electrum)?;
    let [server_version, protocol_version]: [String; 2] = serde_json::from_value(electrum.call(
        "server.version",
        &[
            json!(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            )),
            json!("1.4"),
        ],
    )?)?;
    let mut client = rpc::Rpc::new(rpc)?;
    let tip = client.call("getbestblockhash", &json!([]))?;
    let hash = tip
        .as_str()
        .context("RPC returned a non-string block hash")?;
    let _: bitcoin::BlockHash = hash.parse().context("RPC returned an invalid block hash")?;
    // Bind the roots lookup to this response, not to a second tip lookup.
    let roots = client.call("getutreexoroots", &json!([hash]))?;
    let peer = peer::probe(peer)?;
    Ok(json!({
        "electrum": {"server_version": server_version, "protocol_version": protocol_version},
        "utreexod_rpc": {"block_hash": hash, "roots": roots},
        "utreexod": {"protocol_version": peer.version, "start_height": peer.start_height},
    }))
}

fn result(reply: &Value, id: u64) -> Result<Value> {
    if reply.get("id") != Some(&json!(id)) || !reply["error"].is_null() {
        bail!("node rejected request or returned a mismatched response ID");
    }
    reply.get("result").cloned().context("missing node result")
}
