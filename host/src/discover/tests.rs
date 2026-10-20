use super::*;
use crate::test_support::{accept, read_http};
use anyhow::anyhow;
use bitcoin::{Txid, hashes::Hash};
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    net::TcpListener,
    thread,
};
use url::Url;

#[test]
fn only_unspent_confirmed_candidates_reach_gettxout() -> Result<()> {
    check_discovery(false)
}

#[test]
fn oversized_history_retries_the_same_offset_with_smaller_pages() -> Result<()> {
    check_discovery(true)
}

fn check_discovery(oversized: bool) -> Result<()> {
    let address = Address::p2wsh(
        &bitcoin::ScriptBuf::from_bytes(vec![0x51]),
        Network::Regtest,
    );
    let script = address.script_pubkey();
    let genesis = genesis_block(Network::Regtest).block_hash();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let config = utreexod_rpc::Config::new(
        Url::parse(&format!("http://{}/", listener.local_addr()?))?,
        None,
    );
    // One full page of confirmed outputs; the next page spends all except one.
    let page_size = if oversized { PAGE_SIZE / 2 } else { PAGE_SIZE };
    let outpoints: Vec<_> = (0..page_size)
        .map(|index| OutPoint::new(Txid::hash(&index.to_le_bytes()), 0))
        .collect();
    let survivor = outpoints[0];
    let first: Vec<_> = outpoints
        .iter()
        .map(|outpoint| {
            json!({
                "txid":outpoint.txid, "confirmations":10, "vin":[{"coinbase":"00"}],
                "vout":[{"n":outpoint.vout,"scriptPubKey":{"hex":script}}],
            })
        })
        .collect();
    let inputs: Vec<_> = outpoints
        .iter()
        .skip(1)
        .map(|outpoint| json!({"txid":outpoint.txid,"vout":outpoint.vout}))
        .collect();
    let second = json!([
        {"txid":Txid::from_byte_array([1;32]),"confirmations":1,"vin":inputs,"vout":[]},
        // A pending spend of the survivor must not remove it or add its new output.
        {"txid":Txid::from_byte_array([2;32]),"vin":[{"txid":survivor.txid,"vout":survivor.vout}],
         "vout":[{"n":0,"scriptPubKey":{"hex":script}}]}
    ]);
    let mut requests = vec![
        ("getblockhash", json!([0]), json!(genesis)),
        (
            "getbestblock",
            json!([]),
            json!({"hash":genesis,"height":20}),
        ),
        (
            "searchrawtransactions",
            json!([address.to_string(), 1, 0, page_size, 0, false]),
            json!(first),
        ),
        (
            "searchrawtransactions",
            json!([address.to_string(), 1, page_size, page_size, 0, false]),
            second,
        ),
        (
            "gettxout",
            json!([survivor.txid, survivor.vout, false]),
            json!({
                "bestblock":genesis,"confirmations":10,"coinbase":true,"value":1,
                "scriptPubKey":{"hex":script}
            }),
        ),
        (
            "getbestblock",
            json!([]),
            json!({"hash":genesis,"height":20}),
        ),
    ];
    if oversized {
        requests.insert(
            2,
            (
                "searchrawtransactions",
                json!([address.to_string(), 1, 0, PAGE_SIZE, 0, false]),
                serde_json::Value::Null,
            ),
        );
    }
    let task = thread::spawn(move || serve(&listener, requests, oversized));
    let result = super::outpoints(&address, Network::Regtest, &config);
    task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    assert_eq!(result?, [survivor]);
    Ok(())
}

fn serve(
    listener: &TcpListener,
    requests: Vec<(&str, Value, Value)>,
    oversized: bool,
) -> Result<()> {
    for (index, (method, params, result)) in requests.into_iter().enumerate() {
        let (socket, _) = accept(listener)?;
        let (_, request) = read_http(&mut BufReader::new(&socket))?;
        let id = index + 1;
        let (expected, body) = if method == "gettxout" {
            (
                json!([{"jsonrpc":"2.0","id":id,"method":method,"params":params}]),
                json!([{"id":id,"result":result}]).to_string(),
            )
        } else {
            let body = if oversized && index == 2 {
                " ".repeat(16 * 1024 * 1024 + 1)
            } else {
                json!({"id":id,"result":result}).to_string()
            };
            (
                json!({"jsonrpc":"1.0","id":id,"method":method,"params":params}),
                body,
            )
        };
        assert_eq!(request, expected);
        write!(
            &socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )?;
    }
    Ok(())
}
