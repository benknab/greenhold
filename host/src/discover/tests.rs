use super::*;
use crate::test_support::{accept, read_http};
use anyhow::anyhow;
use bitcoin::{Txid, block::Header, consensus::encode::serialize_hex, hashes::Hash};
use serde_json::{Value, json};
use std::{
    io::{BufRead as _, BufReader, Write},
    net::TcpListener,
    thread,
};
use url::Url;

#[test]
fn only_unspent_confirmed_candidates_reach_gettxout() -> Result<()> {
    check_discovery(false, 10, false)
}

#[test]
fn large_history_pages_preserve_pagination() -> Result<()> {
    check_discovery(true, 10, false)
}

#[test]
fn utreexod_rejects_confirmations_above_the_observed_height() -> Result<()> {
    check_discovery(false, u32::MAX, false)
}

#[test]
fn utreexod_rejects_a_tip_change_after_creation_hash_lookup() -> Result<()> {
    check_discovery(false, 10, true)
}

fn check_discovery(large_reply: bool, confirmations: u32, changed_tip: bool) -> Result<()> {
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
    // One full page of confirmed outputs; the next page spends all except two.
    let outpoints: Vec<_> = (0..PAGE_SIZE)
        .map(|index| OutPoint::new(Txid::hash(&index.to_le_bytes()), 0))
        .collect();
    let mut survivors = outpoints[..2].to_vec();
    survivors.sort_unstable();
    let creation_hash = BlockHash::from_byte_array([42; 32]);
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
        .skip(2)
        .map(|outpoint| json!({"txid":outpoint.txid,"vout":outpoint.vout}))
        .collect();
    let second = json!([
        {"txid":Txid::from_byte_array([1;32]),"confirmations":1,"vin":inputs,"vout":[]},
        // A pending spend of the survivor must not remove it or add its new output.
        {"txid":Txid::from_byte_array([2;32]),"vin":[{"txid":outpoints[0].txid,"vout":outpoints[0].vout}],
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
            json!([address.to_string(), 1, 0, PAGE_SIZE, 0, false]),
            json!(first),
        ),
        (
            "searchrawtransactions",
            json!([address.to_string(), 1, PAGE_SIZE, PAGE_SIZE, 0, false]),
            second,
        ),
        (
            "gettxout",
            json!(survivors.iter().map(|coin| json!([coin.txid, coin.vout, false])).collect::<Vec<_>>()),
            json!(survivors.iter().map(|_| json!({
                "bestblock":genesis,"confirmations":confirmations,"coinbase":true,"value":1.000_000_01,
                "scriptPubKey":{"hex":script}
            })).collect::<Vec<_>>()),
        ),
    ];
    if confirmations == 10 {
        requests.extend([
            ("getblockhash", json!([11]), json!(creation_hash)),
            (
                "getbestblock",
                json!([]),
                json!({"hash":if changed_tip {creation_hash} else {genesis},"height":20}),
            ),
        ]);
    }
    let task = thread::spawn(move || serve(&listener, requests, large_reply));
    let result = super::coins(&address, Network::Regtest, &config);
    task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    if confirmations != 10 || changed_tip {
        let error = result.err().context("inconsistent discovery accepted")?;
        assert_eq!(
            error.to_string(),
            if changed_tip {
                "chain tip changed during discovery; run discover again"
            } else {
                "RPC confirmation count exceeds observed height"
            }
        );
    } else {
        assert_eq!(
            result?,
            survivors
                .into_iter()
                .map(|outpoint| CoinSummary {
                    outpoint,
                    value_sats: 100_000_001,
                    creation_height: 11,
                    creation_hash,
                })
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

fn serve(
    listener: &TcpListener,
    requests: Vec<(&str, Value, Value)>,
    large_reply: bool,
) -> Result<()> {
    let mut id = 1;
    for (index, (method, params, result)) in requests.into_iter().enumerate() {
        let (socket, _) = accept(listener)?;
        let (_, request) = read_http(&mut BufReader::new(&socket))?;
        let (expected, body) = if method == "gettxout" {
            let params = params.as_array().context("batch params")?;
            let results = result.as_array().context("batch results")?;
            let requests: Vec<_> = params.iter().enumerate().map(|(offset, params)| json!({"jsonrpc":"2.0","id":id + offset,"method":method,"params":params})).collect();
            let replies: Vec<_> = results
                .iter()
                .enumerate()
                .map(|(offset, result)| json!({"id":id + offset,"result":result}))
                .collect();
            id += params.len();
            (json!(requests), json!(replies).to_string())
        } else {
            let body = if large_reply && index == 2 {
                json!({"id":id,"result":result,"future_field":"x".repeat(17 * 1024 * 1024)})
                    .to_string()
            } else {
                json!({"id":id,"result":result}).to_string()
            };
            let request = json!({"jsonrpc":"1.0","id":id,"method":method,"params":params});
            id += 1;
            (request, body)
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
            {"tx_hash":first,"tx_pos":1,"height":10,"value":1234,"future_field":true},
            {"tx_hash":first,"tx_pos":0,"height":0,"value":5678},
            {"tx_hash":second,"tx_pos":0,"height":-1,"value":5678},
            {"tx_hash":first,"tx_pos":1,"height":10,"value":1234}
        ]),
    )??;
    assert_eq!(
        result,
        [
            CoinSummary {
                outpoint: OutPoint::new(first, 1),
                value_sats: 1234,
                creation_height: 10,
                creation_hash: electrum_header(10).block_hash()
            },
            CoinSummary {
                outpoint: OutPoint::new(second, 3),
                value_sats: 1000,
                creation_height: 20,
                creation_hash: electrum_header(20).block_hash()
            },
        ]
    );
    assert!(electrum_response(Network::Regtest, json!([]))??.is_empty());
    Ok(())
}

#[test]
fn electrum_rejects_conflicting_details_for_one_outpoint() -> Result<()> {
    let txid = Txid::from_byte_array([1; 32]);
    let error = electrum_response(
        Network::Regtest,
        json!([
            {"tx_hash":txid,"tx_pos":0,"height":1,"value":1000},
            {"tx_hash":txid,"tx_pos":0,"height":1,"value":2000}
        ]),
    )?
    .err()
    .context("conflicting coin accepted")?;
    assert_eq!(
        error.to_string(),
        "Electrum returned conflicting details for the same outpoint"
    );
    Ok(())
}

#[test]
fn electrum_accepts_large_unspent_responses() -> Result<()> {
    let expected: Vec<_> = (0_u32..1_000)
        .map(|index| OutPoint::new(Txid::hash(&index.to_le_bytes()), 0))
        .collect();
    let mut coins: Vec<_> = expected
        .iter()
        .map(|coin| json!({"tx_hash":coin.txid,"tx_pos":coin.vout,"height":1,"value":1000}))
        .collect();
    coins[0]["future_field"] = json!("x".repeat(17 * 1024 * 1024));
    let actual = electrum_response(Network::Regtest, json!(coins))??;
    assert_eq!(
        actual
            .into_iter()
            .map(|coin| {
                assert_eq!(coin.value_sats, 1000);
                assert_eq!(coin.creation_height, 1);
                assert_eq!(coin.creation_hash, electrum_header(1).block_hash());
                coin.outpoint
            })
            .collect::<BTreeSet<_>>(),
        expected.into_iter().collect()
    );
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
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":0,"value":1000}),
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":-1,"height":1,"value":1000}),
        json!({"tx_hash":"invalid","tx_pos":0,"height":1,"value":1000}),
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":0,"height":1}),
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":0,"height":1,"value":-1}),
        json!({"tx_hash":Txid::from_byte_array([1;32]),"tx_pos":0,"height":1,"value":1.5}),
    ] {
        let error = electrum_response(Network::Regtest, json!([coin]))?
            .err()
            .context("malformed candidate accepted")?;
        assert_eq!(error.to_string(), "invalid JSON-RPC result");
    }
    Ok(())
}

// The outer result checks the fixture. The inner result comes from discovery.
fn electrum_response(server_network: Network, coins: Value) -> Result<Result<Vec<CoinSummary>>> {
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
        let parsed = serde_json::from_value::<Vec<electrum::Unspent>>(coins.clone());
        requests.push((
            "blockchain.scripthash.listunspent",
            json!(["5c4430ceb6dafeed32dd97588979e78cb6ceab6787cd38f1da96eb4cbd081a40"]),
            coins,
        ));
        if let Ok(coins) = parsed {
            let mut heights = BTreeSet::new();
            for coin in coins
                .into_iter()
                .filter(|coin| coin.height > 0 && heights.insert(coin.height))
            {
                requests.push((
                    "blockchain.block.header",
                    json!([coin.height]),
                    json!(serialize_hex(&electrum_header(u32::try_from(coin.height)?))),
                ));
            }
        }
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
    let result = electrum_coins(&address, Network::Regtest, &endpoint);
    task.join()
        .map_err(|_| anyhow!("Electrum mock panicked"))??;
    Ok(result)
}

fn electrum_header(height: u32) -> Header {
    let mut header = genesis_block(Network::Regtest).header;
    header.nonce = height;
    header
}
