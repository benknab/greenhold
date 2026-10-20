use super::*;
use crate::test_support::{accept, read_http};
use anyhow::anyhow;
use bitcoin::{Txid, hashes::Hash};
use serde_json::{Value, json};
use std::{
    io::{BufRead as _, BufReader, Write},
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

#[test]
fn electrum_candidates_are_confirmed_sorted_and_unique() -> Result<()> {
    let first = Txid::from_byte_array([1; 32]);
    let second = Txid::from_byte_array([2; 32]);
    let result = electrum_response(
        Network::Regtest,
        json!([
            {"tx_hash":second,"tx_pos":3,"height":20,"value":1000},
            {"tx_hash":first,"tx_pos":1,"height":10,"future_field":true},
            {"tx_hash":first,"tx_pos":0,"height":0},
            {"tx_hash":second,"tx_pos":0,"height":-1},
            {"tx_hash":first,"tx_pos":1,"height":10}
        ]),
    )??;
    assert_eq!(result, [OutPoint::new(first, 1), OutPoint::new(second, 3)]);
    assert!(electrum_response(Network::Regtest, json!([]))??.is_empty());
    Ok(())
}

#[test]
fn electrum_rejects_wrong_network_before_querying_the_address() -> Result<()> {
    let error = electrum_response(Network::Bitcoin, json!([]))?
        .err()
        .context("wrong network accepted")?;
    assert!(
        error
            .to_string()
            .contains("Electrum server is not on the requested network")
    );
    Ok(())
}

#[test]
fn electrum_rejects_malformed_candidate_data() -> Result<()> {
    for coin in [
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":0}),
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":-1,"height":1}),
        json!({"tx_hash":"invalid","tx_pos":0,"height":1}),
    ] {
        let error = electrum_response(Network::Regtest, json!([coin]))?
            .err()
            .context("malformed candidate accepted")?;
        assert_eq!(error.to_string(), "invalid JSON-RPC result");
    }
    Ok(())
}

// The outer result checks the fixture. The inner result comes from discovery.
fn electrum_response(server_network: Network, coins: Value) -> Result<Result<Vec<OutPoint>>> {
    let address = Address::p2wsh(
        &bitcoin::ScriptBuf::from_bytes(vec![0x51]),
        Network::Regtest,
    );
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
    let mut requests = vec![
        (
            "server.version",
            json!([
                concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
                "1.4"
            ]),
            json!(["test", "1.4"]),
        ),
        (
            "server.features",
            json!([]),
            json!({"genesis_hash":genesis_block(server_network).block_hash()}),
        ),
    ];
    if server_network == Network::Regtest {
        requests.push((
            "blockchain.scripthash.listunspent",
            json!(["5c4430ceb6dafeed32dd97588979e78cb6ceab6787cd38f1da96eb4cbd081a40"]),
            coins,
        ));
    }
    let task = thread::spawn(move || -> Result<()> {
        let (socket, _) = accept(&listener)?;
        let mut reader = BufReader::new(&socket);
        for (index, (method, params, result)) in requests.into_iter().enumerate() {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let request: Value = serde_json::from_str(&line)?;
            let id = index + 1;
            assert_eq!(request, json!({"id":id,"method":method,"params":params}));
            writeln!(&socket, "{}", json!({"id":id,"result":result}))?;
        }
        Ok(())
    });
    let result = electrum_outpoints(&address, Network::Regtest, &endpoint);
    task.join()
        .map_err(|_| anyhow!("Electrum mock panicked"))??;
    Ok(result)
}
