pub mod electrum;
mod peer;
pub mod rpc;

use crate::Result;
use serde_json::{Value, json};
use std::{net::TcpStream, time::Duration};
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MESSAGE: u64 = 64 * 1024;

pub fn status(
    electrum: &electrum::ElectrumEndpoint,
    peer: &Url,
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
    let hash = tip.as_str().ok_or("RPC returned a non-string block hash")?;
    let _: bitcoin::BlockHash = hash
        .parse()
        .map_err(|_| "RPC returned an invalid block hash")?;
    // Bind the roots lookup to this response, not to a second tip lookup.
    let roots = client.call("getutreexoroots", &json!([hash]))?;
    let peer = peer::handshake(peer)?;
    Ok(json!({
        "electrum": {"server_version": server_version, "protocol_version": protocol_version},
        "utreexod_rpc": {"block_hash": hash, "roots": roots},
        "utreexod": peer,
    }))
}

pub(super) fn validate_socket_url(url: &Url) -> std::result::Result<(), &'static str> {
    if url.host().is_none()
        || !matches!(url.port(), Some(1..))
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "endpoint must use SCHEME://HOST:PORT with a nonzero port and no credentials, path, query, or fragment",
        );
    }
    Ok(())
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
