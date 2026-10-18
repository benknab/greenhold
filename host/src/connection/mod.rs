mod electrum;
mod peer;
pub mod rpc;

use crate::Result;
use serde_json::{Value, json};
use std::{net::TcpStream, time::Duration};
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE: u64 = 64 * 1024;

pub fn status(electrum: &Url, peer: &Url, rpc: &rpc::RpcConfig) -> Result<Value> {
    let electrum_tip = electrum::tip(electrum)?;
    let mut client = rpc::Rpc::new(rpc)?;
    let tip = client.call("getbestblockhash", &json!([]))?;
    let hash = tip.as_str().ok_or("RPC returned a non-string block hash")?;
    let _: bitcoin::BlockHash = hash
        .parse()
        .map_err(|_| "RPC returned an invalid block hash")?;
    // Bind the roots lookup to this response, not to a second tip lookup.
    let roots = client.call("getutreexoroots", &json!([hash]))?;
    let peer = peer::handshake(peer)?;
    Ok(json!({
        "electrum": electrum_tip,
        "utreexod_rpc": {"block_hash": hash, "roots": roots},
        "utreexod": peer,
    }))
}

fn connect(endpoint: &Url) -> Result<TcpStream> {
    let addresses = endpoint
        .socket_addrs(|| None)
        .map_err(|_| "node DNS lookup failed")?;
    for address in addresses {
        if let Ok(socket) = TcpStream::connect_timeout(&address, TIMEOUT) {
            socket.set_read_timeout(Some(TIMEOUT))?;
            socket.set_write_timeout(Some(TIMEOUT))?;
            return Ok(socket);
        }
    }
    Err("node connection failed".into())
}

fn result(reply: &Value, id: u64) -> Result<Value> {
    if reply.get("id") != Some(&json!(id)) || !reply["error"].is_null() {
        return Err("node rejected request or returned a mismatched response ID".into());
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| "missing node result".into())
}
