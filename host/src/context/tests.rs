use super::*;
use anyhow::anyhow;
use bitcoin::{BlockHash, consensus::serialize, hashes::Hash, hex::DisplayHex};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::Duration,
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

fn read_http(stream: &mut impl BufRead) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line)? == 0 {
            bail!("request ended before HTTP headers");
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length: ") {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let mut body = vec![0; length.context("missing content length")?];
    stream.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

fn serve_rpc(steps: Vec<Step>) -> Result<(utreexod_rpc::Config, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let config = utreexod_rpc::Config::new(
        Url::parse(&format!("http://{}/", listener.local_addr()?))?,
        None,
    );
    let task = thread::spawn(move || {
        for (index, expected) in steps.iter().enumerate() {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(Duration::from_secs(20)))?;
            let mut stream = BufReader::new(socket);
            let request = read_http(&mut stream)?;
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

fn serve_electrum(steps: Vec<Step>) -> Result<(electrum::Endpoint, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
    let task = thread::spawn(move || {
        let (socket, _) = listener.accept()?;
        socket.set_read_timeout(Some(Duration::from_secs(20)))?;
        let mut stream = BufReader::new(socket);
        for (index, expected) in steps.iter().enumerate() {
            let mut line = String::new();
            stream.read_line(&mut line)?;
            let request = serde_json::from_str(&line)?;
            let reply = respond(&request, expected, u64::try_from(index)? + 1);
            writeln!(stream.get_mut(), "{reply}")?;
        }
        Ok(())
    });
    Ok((endpoint, task))
}

fn run(request: Request, rpc: Vec<Step>, chain: Vec<Step>) -> Result<Result<ChainContext>> {
    let (config, rpc_task) = serve_rpc(rpc)?;
    let (endpoint, chain_task) = serve_electrum(chain)?;
    let result = fetch(request, &endpoint, &config);
    rpc_task
        .join()
        .map_err(|_| anyhow!("RPC mock panicked"))??;
    chain_task
        .join()
        .map_err(|_| anyhow!("Electrum mock panicked"))??;
    Ok(result)
}

fn version() -> Step {
    step(
        "server.version",
        json!([
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
            "1.4"
        ]),
        json!(["fixture", "1.4"]),
    )
}

fn hash(height: u32, block: BlockHash) -> Step {
    step("getblockhash", json!([height]), json!(block))
}

fn chain_network(block: BlockHash) -> Step {
    step("server.features", json!([]), json!({"genesis_hash": block}))
}

#[test]
fn rpc_tip_is_pinned_and_first_height_defaults_to_it() -> Result<()> {
    let cases = [Network::Bitcoin, Network::Signet, Network::Regtest]
        .into_iter()
        .flat_map(|network| [None, Some(0), Some(10), Some(11)].map(|first| (network, first)));
    for (network, first) in cases {
        // Synthetic height binding and roots, for retrieval behavior only.
        let header = genesis_block(network).header;
        let block = header.block_hash();
        let raw = serialize(&header).as_hex().to_string();
        let mut rpc = vec![
            hash(0, block),
            step("getbestblock", json!([]), json!({"hash":block,"height":10})),
            step("getblockheader", json!([block, false]), json!(raw)),
        ];
        let mut chain = vec![version(), chain_network(block)];
        if first == Some(0) {
            rpc.push(hash(0, block));
            rpc.push(step("getblockheader", json!([block, false]), json!(raw)));
        }
        if first != Some(11) {
            rpc.push(step(
                "getutreexoroots",
                json!([block]),
                json!({"numleaves":0,"roots":[]}),
            ));
            let heights = if first == Some(0) {
                vec![0, 10]
            } else {
                vec![10]
            };
            for height in heights {
                rpc.push(hash(height, block));
                chain.push(step("blockchain.block.header", json!([height]), json!(raw)));
            }
        }
        // The script permits exactly one tip query and no Electrum tip query.
        let result = run(Request::new(network, first, None)?, rpc, chain)?;
        if first == Some(11) {
            assert!(
                result
                    .err()
                    .context("expected height failure")?
                    .to_string()
                    .contains("first height")
            );
        } else {
            let context = result?;
            assert_eq!(context.last().height(), 10);
            assert_eq!(context.last().hash(), block);
            assert_eq!(context.first().height(), first.unwrap_or(10));
        }
    }
    Ok(())
}

#[test]
fn selected_tip_reorg_and_electrum_lag_fail() -> Result<()> {
    let header = genesis_block(Network::Bitcoin).header;
    let block = header.block_hash();
    let raw = serialize(&header).as_hex().to_string();
    for reorg in [false, true] {
        let rpc = vec![
            hash(0, block),
            step("getbestblock", json!([]), json!({"hash":block,"height":0})),
            step("getblockheader", json!([block, false]), json!(raw)),
            step(
                "getutreexoroots",
                json!([block]),
                json!({"numleaves":0,"roots":[]}),
            ),
            hash(0, if reorg { BlockHash::all_zeros() } else { block }),
        ];
        let mut chain = vec![version(), chain_network(block)];
        if !reorg {
            chain.push((
                "blockchain.block.header",
                json!([0]),
                json!({"error":{"code":-1,"message":"height unavailable"}}),
            ));
        }
        let error = run(Request::new(Network::Bitcoin, None, None)?, rpc, chain)?
            .err()
            .context("expected retrieval failure")?;
        assert!(error.to_string().contains(if reorg {
            "RPC chain changed"
        } else {
            "cannot retrieve Electrum header"
        }));
    }
    Ok(())
}

#[test]
fn fixed_historical_endpoints_and_single_block_context() -> Result<()> {
    // Real mainnet headers, synthetic roots. These establish component behavior only.
    let genesis = genesis_block(Network::Bitcoin).header;
    let historical: bitcoin::block::Header = bitcoin::consensus::deserialize(&[
        0, 96, 29, 52, 85, 187, 159, 189, 150, 107, 62, 162, 220, 66, 208, 194, 39, 34, 228, 192,
        193, 114, 159, 173, 23, 33, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 85, 8, 127, 171, 12, 143, 63,
        137, 248, 188, 253, 77, 242, 108, 80, 77, 129, 176, 168, 142, 4, 144, 113, 97, 131, 140,
        12, 83, 0, 26, 240, 145, 53, 237, 189, 100, 148, 56, 5, 23, 94, 149, 94, 6,
    ])?;
    for first in [0, 800_000] {
        let mut rpc = vec![hash(0, genesis.block_hash())];
        let mut endpoints = vec![(first, if first == 0 { genesis } else { historical })];
        if first != 800_000 {
            endpoints.push((800_000, historical));
        }
        for (height, header) in &endpoints {
            rpc.push(hash(*height, header.block_hash()));
            rpc.push(step(
                "getblockheader",
                json!([header.block_hash(), false]),
                json!(serialize(header).as_hex().to_string()),
            ));
        }
        rpc.push(step(
            "getutreexoroots",
            json!([historical.block_hash()]),
            json!({"numleaves":3,"roots":["11".repeat(32), "22".repeat(32)]}),
        ));
        let mut chain = vec![version(), chain_network(genesis.block_hash())];
        for (height, header) in endpoints {
            rpc.push(hash(height, header.block_hash()));
            chain.push(step(
                "blockchain.block.header",
                json!([height]),
                json!(serialize(&header).as_hex().to_string()),
            ));
        }
        let context = run(
            Request::new(Network::Bitcoin, Some(first), Some(800_000))?,
            rpc,
            chain,
        )??;
        assert_eq!(context.first().height(), first);
        assert_eq!(context.last().height(), 800_000);
        assert_eq!(context.last().hash(), historical.block_hash());
        assert_eq!(context.state().num_leaves(), 3);
        assert_eq!(context.state().roots(), &[[0x11; 32], [0x22; 32]]);
    }
    Ok(())
}

#[test]
fn rejects_wrong_networks_before_retrieving_state() -> Result<()> {
    for network in [Network::Bitcoin, Network::Signet, Network::Regtest] {
        let genesis = genesis_block(network).block_hash();
        let wrong = if network == Network::Bitcoin {
            genesis_block(Network::Testnet).block_hash()
        } else {
            genesis_block(Network::Bitcoin).block_hash()
        };
        for (rpc_hash, chain) in [
            (wrong, vec![version()]),
            (genesis, vec![version(), chain_network(wrong)]),
        ] {
            let error = run(
                Request::new(network, Some(0), Some(0))?,
                vec![hash(0, rpc_hash)],
                chain,
            )?
            .err()
            .context("expected network failure")?;
            assert!(error.to_string().contains("not on the requested network"));
        }
    }
    assert!(Request::new(Network::Bitcoin, Some(2), Some(1)).is_err());
    assert!(Request::new(Network::Testnet, None, None).is_err());
    assert!(Request::new(Network::Testnet4, None, None).is_err());
    Ok(())
}

#[test]
fn retrieves_context_for_each_requested_network() -> Result<()> {
    for network in [Network::Bitcoin, Network::Signet, Network::Regtest] {
        let header = genesis_block(network).header;
        let genesis = header.block_hash();
        let raw = serialize(&header).as_hex().to_string();
        let rpc = vec![
            hash(0, genesis),
            hash(0, genesis),
            step("getblockheader", json!([genesis, false]), json!(raw)),
            step(
                "getutreexoroots",
                json!([genesis]),
                json!({"numleaves":0,"roots":[]}),
            ),
            hash(0, genesis),
        ];
        let chain = vec![
            version(),
            chain_network(genesis),
            step("blockchain.block.header", json!([0]), json!(raw)),
        ];
        let context = run(Request::new(network, None, Some(0))?, rpc, chain)??;
        assert_eq!(context.network(), network);
        assert_eq!(context.first().hash(), genesis);
        assert_eq!(context.last().hash(), genesis);
    }
    Ok(())
}

#[test]
fn rejects_unavailable_state_malformed_headers_reorgs_and_disagreement() -> Result<()> {
    let header = genesis_block(Network::Bitcoin).header;
    let genesis = header.block_hash();
    let raw = serialize(&header).as_hex().to_string();
    for case in [
        "unavailable",
        "bad-roots",
        "large-state",
        "bad-header",
        "wrong-header",
        "reorg",
        "disagreement",
        "chain-behind",
    ] {
        let header_reply = match case {
            "bad-header" => json!("ab".repeat(81)),
            "wrong-header" => json!(
                serialize(&genesis_block(Network::Testnet).header)
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
        let mut chain = vec![version(), chain_network(genesis)];
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
        if matches!(case, "reorg" | "disagreement" | "chain-behind") {
            rpc.push(hash(
                0,
                if case == "reorg" {
                    BlockHash::all_zeros()
                } else {
                    genesis
                },
            ));
        }
        if matches!(case, "disagreement" | "chain-behind") {
            let reply = if case == "chain-behind" {
                json!({"error":{"code":-1,"message":"height unavailable"}})
            } else {
                json!({"result": serialize(&genesis_block(Network::Testnet).header).as_hex().to_string()})
            };
            chain.push(("blockchain.block.header", json!([0]), reply));
        }
        let result = run(
            Request::new(Network::Bitcoin, Some(0), Some(0))?,
            rpc,
            chain,
        )?;
        assert!(result.is_err(), "{case}");
        assert!(!format!("{result:?}").contains("secret"));
    }
    Ok(())
}
