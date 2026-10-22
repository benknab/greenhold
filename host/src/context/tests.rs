use super::*;
use crate::test_support;
use anyhow::anyhow;
use bitcoin::{BlockHash, consensus, hashes::Hash, hex::DisplayHex};
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
};
use url::Url;

type Step = (&'static str, Value, Value);

fn step(method: &'static str, params: Value, result: Value) -> Step {
    let mut reply = json!({});
    reply["result"] = result;
    (method, params, reply)
}

fn respond(request: &Value, expected: &Step, id: u64) -> Value {
    assert_eq!(request["id"], id);
    assert_eq!(request["method"], expected.0);
    assert_eq!(request["params"], expected.1);
    let mut reply = expected.2.clone();
    reply["id"] = json!(id);
    reply
}

fn serve_rpc(steps: Vec<Step>) -> Result<(utreexod_rpc::Config, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let config = utreexod_rpc::Config::new(
        Url::parse(&format!("http://{}/", listener.local_addr()?))?,
        None,
    );
    let task = thread::spawn(move || {
        for (index, expected) in steps.iter().enumerate() {
            let (socket, _) = test_support::accept(&listener)?;
            let mut stream = BufReader::new(socket);
            let (_, request) = test_support::read_http(&mut stream)?;
            assert_eq!(request["jsonrpc"], "1.0");
            let reply = respond(&request, expected, u64::try_from(index)? + 1).to_string();
            write!(
                stream.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )?;
        }
        Ok(())
    });
    Ok((config, task))
}

fn run(input: Input, rpc: Vec<Step>) -> Result<Result<ChainContext>> {
    let (config, rpc_task) = serve_rpc(rpc)?;
    let result = fetch(input, &config);
    rpc_task
        .join()
        .map_err(|_| anyhow!("RPC mock panicked"))??;
    Ok(result)
}

fn hash(height: u32, block: BlockHash) -> Step {
    step("getblockhash", json!([height]), json!(block))
}

#[test]
fn header_lookup_binds_the_parsed_header_to_the_requested_hash() -> Result<()> {
    let header = constants::genesis_block(Network::Regtest).header;
    let hash = header.block_hash();
    for requested in [hash, BlockHash::all_zeros()] {
        let (config, task) = serve_rpc(vec![step(
            "getblockheader",
            json!([requested, false]),
            json!(consensus::serialize(&header).as_hex().to_string()),
        )])?;
        let result = block_header(&mut utreexod_rpc::Client::new(&config), &requested)
            .map_err(|error| error.to_string());
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
        let expected = if requested == hash {
            Ok(header)
        } else {
            Err("RPC header does not match requested block hash".to_owned())
        };
        assert_eq!(result, expected);
    }
    Ok(())
}

#[test]
fn rpc_tip_is_resolved_once_on_each_requested_network() -> Result<()> {
    for network in [Network::Bitcoin, Network::Signet, Network::Regtest] {
        // Synthetic height binding and roots, for retrieval behavior only.
        let header = constants::genesis_block(network).header;
        let block = header.block_hash();
        let raw = consensus::serialize(&header).as_hex().to_string();
        let rpc = vec![
            hash(0, block),
            step("getbestblock", json!([]), json!({"hash":block,"height":10})),
            step("getblockheader", json!([block, false]), json!(raw)),
            hash(0, block),
            step("getblockheader", json!([block, false]), json!(raw)),
            step(
                "getutreexoroots",
                json!([block]),
                json!({"numleaves":0,"roots":[]}),
            ),
        ];
        // The script permits exactly one tip query.
        let context = run(Input::new(network, Some(0), None)?, rpc)??;
        assert_eq!(context.network(), network);
        assert_eq!(context.unspent_at().height(), 10);
        assert_eq!(context.unspent_at().hash(), block);
    }
    Ok(())
}

#[test]
fn rejects_wrong_networks_before_retrieving_state() -> Result<()> {
    for network in [Network::Bitcoin, Network::Signet, Network::Regtest] {
        let wrong = if network == Network::Bitcoin {
            constants::genesis_block(Network::Testnet).block_hash()
        } else {
            constants::genesis_block(Network::Bitcoin).block_hash()
        };
        let error = run(Input::new(network, Some(0), Some(0))?, vec![hash(0, wrong)])?
            .err()
            .context("expected network failure")?;
        assert!(error.to_string().contains("not on the requested network"));
    }
    assert!(Input::new(Network::Bitcoin, Some(2), Some(1)).is_err());
    assert!(Input::new(Network::Testnet, None, None).is_err());
    assert!(Input::new(Network::Testnet4, None, None).is_err());
    Ok(())
}

#[test]
fn rejects_unavailable_state_and_malformed_headers() -> Result<()> {
    let header = constants::genesis_block(Network::Bitcoin).header;
    let genesis = header.block_hash();
    let raw = consensus::serialize(&header).as_hex().to_string();
    for case in [
        "unavailable",
        "bad-roots",
        "large-state",
        "bad-header",
        "wrong-header",
    ] {
        let header_reply = match case {
            "bad-header" => json!("ab".repeat(81)),
            "wrong-header" => json!(
                consensus::serialize(&constants::genesis_block(Network::Testnet).header)
                    .as_hex()
                    .to_string()
            ),
            _ => json!(raw),
        };
        let mut rpc = vec![
            hash(0, genesis),
            hash(0, genesis),
            step("getblockheader", json!([genesis, false]), header_reply),
        ];
        if !matches!(case, "bad-header" | "wrong-header") {
            let reply = match case {
                "unavailable" => json!({"error":{"code":-1,"message":"secret node details"}}),
                "bad-roots" => json!({"result":{"numleaves":1,"roots":[]}}),
                "large-state" => {
                    json!({"result":{"numleaves":1_u64 << 62,"roots":["00".repeat(32)]}})
                }
                _ => json!({"result":{"numleaves":0,"roots":null}}),
            };
            rpc.push(("getutreexoroots", json!([genesis]), reply));
        }
        let result = run(Input::new(Network::Bitcoin, Some(0), Some(0))?, rpc)?;
        let error = result
            .err()
            .with_context(|| format!("{case} was accepted"))?;
        let expected = match case {
            "unavailable" => "cannot retrieve Utreexo state",
            "bad-roots" => "Utreexo root count does not match leaf count",
            "large-state" => "Utreexo leaf count exceeds supported position range",
            "bad-header" | "wrong-header" => "cannot retrieve RPC header",
            _ => bail!("unknown fixture case"),
        };
        assert!(error.to_string().contains(expected), "{case}: {error:#}");
        assert!(!format!("{error:#}").contains("secret"));
    }
    Ok(())
}
