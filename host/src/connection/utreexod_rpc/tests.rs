use super::*;
use crate::test_support::{accept, read_http};
use anyhow::{Context, Result, anyhow, bail};
use bitcoin::OutPoint;
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    net::TcpListener,
    thread,
};

const BLOCK: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn serve(
    status: &str,
    replies: Vec<(Value, String)>,
    auth: bool,
) -> Result<(Config, thread::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let config = Config::new(
        Url::parse(&format!("http://{}/", listener.local_addr()?))?,
        auth.then(|| ("user".into(), "pass".into())),
    );
    let status = status.to_owned();
    let task = thread::spawn(move || {
        for (expected, body) in replies {
            let (socket, _) = accept(&listener)?;
            let (headers, request) = read_http(&mut BufReader::new(&socket))?;
            assert!(headers.starts_with("post / http/1.1\r\n"));
            assert_eq!(headers.contains("authorization:"), auth);
            assert!(!auth || headers.contains("authorization: basic dxnlcjpwyxnz\r\n"));
            assert_eq!(request, expected);
            write!(
                &socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: /redirected\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
        }
        Ok(())
    });
    Ok((config, task))
}

fn best_request() -> Value {
    json!({"jsonrpc":"1.0","id":1,"method":"getbestblock","params":[]})
}

#[test]
fn output_lookup_and_proof_request_preserve_rpc_semantics() -> Result<()> {
    let first = OutPoint::new(BLOCK.parse()?, 2);
    let second = OutPoint::new("11".repeat(32).parse()?, 3);
    let replies = vec![
        (
            json!({"jsonrpc":"1.0","id":1,"method":"gettxout","params":[first.txid,2,true]}),
            json!({"id":1,"result":null}).to_string(),
        ),
        (
            json!({"jsonrpc":"1.0","id":2,"method":"proveutxochaintipinclusion",
            "params":[[first.txid,second.txid],[2,3],1]}),
            json!({"id":2,"result":{"provedathash":BLOCK,"prooftargets":[4,5],
                "hashesproven":[BLOCK,BLOCK],"proofhashes":[]}})
            .to_string(),
        ),
    ];
    let (config, task) = serve("200 OK", replies, false)?;
    let mut client = Client::new(&config);
    assert!(client.tx_out(&first.txid, first.vout, true)?.is_none());
    let proof = client.prove_utxo_chain_tip_inclusion(&[first.txid, second.txid], &[2, 3])?;
    assert_eq!(proof.proof_targets, [4, 5]);
    task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    Ok(())
}

#[test]
fn header_endpoint_parses_both_formats_without_verifying_chain_data() -> Result<()> {
    let header = bitcoin::blockdata::constants::genesis_block(bitcoin::Network::Regtest).header;
    let hash = header.block_hash();
    let raw = bitcoin::consensus::encode::serialize_hex(&header);
    for requested in [hash, BLOCK.parse()?] {
        let replies = vec![
            (
                json!({"jsonrpc":"1.0","id":1,"method":"getblockheader","params":[requested,false]}),
                json!({"id":1,"result":raw}).to_string(),
            ),
            (
                json!({"jsonrpc":"1.0","id":2,"method":"getblockheader","params":[requested,true]}),
                json!({"id":2,"result":{"hash":hash,"height":0}}).to_string(),
            ),
        ];
        let (config, task) = serve("200 OK", replies, false)?;
        let mut client = Client::new(&config);
        let fetched_header = client.block_header(&requested, false);
        let location = client.block_header(&requested, true);
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
        let BlockHeaderResponse::Raw(fetched_header) = fetched_header? else {
            bail!("raw header returned a verbose response");
        };
        assert_eq!(fetched_header, header);
        let BlockHeaderResponse::Verbose(location) = location? else {
            bail!("verbose header returned a raw response");
        };
        assert_eq!((location.hash, location.height), (hash, 0));
    }
    Ok(())
}

#[test]
fn authenticated_and_anonymous_lookup_preserves_block_and_root_bytes() -> Result<()> {
    for auth in [false, true] {
        let replies = vec![
            (best_request(), json!({"id":1,"result":{"hash":BLOCK,"height":123},"error":null,
                "future_field":"x".repeat(128 * 1024)}).to_string()),
            (json!({"jsonrpc":"1.0","id":2,"method":"getutreexoroots","params":[BLOCK]}),
             json!({"id":2,"result":{"numleaves":3,"roots":[BLOCK,"00".repeat(32)]},"error":null}).to_string()),
        ];
        let (config, task) = serve("200 OK", replies, auth)?;
        let mut client = Client::new(&config);
        let block = client.best_block()?;
        assert_eq!(block.hash.to_string(), BLOCK);
        assert_eq!(block.height, 123);
        let roots = client.utreexo_roots(&block.hash)?;
        assert_eq!(roots.num_leaves, 3);
        let mut expected = [0; 32];
        for (byte, value) in expected.iter_mut().zip(0_u8..32) {
            *byte = value;
        }
        assert_eq!(roots.roots, [expected, [0; 32]]);
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    }
    let config = Config::new(
        Url::parse("http://localhost/")?,
        Some(("secret-user".into(), "secret-password".into())),
    );
    assert!(!format!("{config:?}").contains("secret"));
    Ok(())
}

#[test]
fn transport_rejections_are_specific_and_redact_remote_values() -> Result<()> {
    for (status, body, expected) in [
        (
            "302 Found",
            "secret".into(),
            "RPC HTTP 302; check authentication and node availability",
        ),
        (
            "401 Unauthorized",
            "secret".into(),
            "RPC HTTP 401; check authentication and node availability",
        ),
        (
            "200 OK",
            json!({"id":2,"result":{"hash":BLOCK,"height":123}}).to_string(),
            "JSON-RPC response ID does not match request",
        ),
        (
            "200 OK",
            json!({"id":1,"error":{"code":-1,"message":"secret"}}).to_string(),
            "node rejected request",
        ),
        (
            "200 OK",
            json!({"id":1,"result":"secret"}).to_string(),
            "invalid JSON-RPC result",
        ),
        ("200 OK", "secret".into(), "invalid JSON-RPC response"),
    ] {
        let (config, task) = serve(status, vec![(best_request(), body)], false)?;
        let error = Client::new(&config)
            .best_block()
            .err()
            .context("invalid response accepted")?;
        assert_eq!(format!("{error:#}"), expected);
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    }
    Ok(())
}

#[test]
fn address_search_preserves_options_and_distinguishes_exhaustion_from_failures() -> Result<()> {
    for (vin_extra, reverse, code) in [
        (false, false, -5),
        (true, false, -5),
        (false, true, -5),
        (true, true, -5),
        (false, false, -1),
        (false, false, -32601),
    ] {
        let request = json!({"jsonrpc":"1.0","id":1,"method":"searchrawtransactions",
            "params":["address",1,100,100,u8::from(vin_extra),reverse]});
        let body = json!({"id":1,"error":{"code":code,"message":"private address"}});
        let (config, task) = serve("200 OK", vec![(request, body.to_string())], false)?;
        let result =
            Client::new(&config).search_raw_transactions("address", 100, 100, vin_extra, reverse);
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
        if code == -5 {
            assert!(result?.is_empty());
        } else {
            assert_eq!(
                result.err().context("RPC failure ignored")?.to_string(),
                "node rejected request"
            );
        }
    }
    Ok(())
}

#[test]
fn batched_coin_lookup_restores_order_and_preserves_request_ids() -> Result<()> {
    let outpoints = [
        OutPoint::new(BLOCK.parse()?, 2),
        OutPoint::new(BLOCK.parse()?, 3),
    ];
    let coin = json!({"bestblock":BLOCK,"confirmations":2,"coinbase":false,
        "value":0.000_000_01,"scriptPubKey":{"hex":"51"}});
    for include_mempool in [false, true] {
        let requests: Vec<_> = outpoints
            .iter()
            .enumerate()
            .map(|(index, outpoint)| {
                json!({
                    "jsonrpc":"2.0","id":index+2,"method":"gettxout",
                    "params":[outpoint.txid,outpoint.vout,include_mempool]
                })
            })
            .collect();
        let replies = vec![
            (
                best_request(),
                json!({"id":1,"result":{"hash":BLOCK,"height":123}}).to_string(),
            ),
            (
                json!(requests),
                json!([{"id":3,"result":null},
                    {"id":2,"result":coin,"future_field":"x".repeat(2 * 1024 * 1024)}])
                .to_string(),
            ),
            (
                json!({"jsonrpc":"1.0","id":4,"method":"getbestblock","params":[]}),
                json!({"id":4,"result":{"hash":BLOCK,"height":123}}).to_string(),
            ),
        ];
        let (config, task) = serve("200 OK", replies, true)?;
        let mut client = Client::new(&config);
        assert!(client.tx_outs(&[], include_mempool)?.is_empty());
        client.best_block()?;
        let outputs = client.tx_outs(&outpoints, include_mempool)?;
        assert_eq!(outputs.len(), 2);
        assert_eq!(
            outputs[0].as_ref().context("unspent coin")?.value.to_sat(),
            1
        );
        assert!(outputs[1].is_none());
        client.best_block()?;
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
    }
    Ok(())
}

#[test]
fn invalid_batch_responses_fail_without_partial_results() -> Result<()> {
    let outpoints = [
        OutPoint::new(BLOCK.parse()?, 2),
        OutPoint::new(BLOCK.parse()?, 3),
    ];
    let requests: Vec<_> = outpoints
        .iter()
        .enumerate()
        .map(|(index, outpoint)| {
            json!({
                "jsonrpc":"2.0","id":index+1,"method":"gettxout",
                "params":[outpoint.txid,outpoint.vout,false]
            })
        })
        .collect();
    let id_error = "JSON-RPC batch response IDs do not match requests";
    for (reply, expected) in [
        (json!([]), id_error),
        (json!([{"id":1,"result":null}]), id_error),
        (
            json!([{"id":1,"result":null},{"id":1,"result":null}]),
            id_error,
        ),
        (
            json!([{"id":1,"result":null},{"id":3,"result":null}]),
            id_error,
        ),
        (
            json!([{"id":0,"result":null},{"id":1,"result":null}]),
            id_error,
        ),
        (
            json!([{"id":1,"result":null},{"id":2,"result":null},{"id":3,"result":null}]),
            id_error,
        ),
        (
            json!({"id":1,"result":null}),
            "invalid JSON-RPC batch response",
        ),
        (
            json!([{"id":"secret","result":null}]),
            "invalid JSON-RPC batch response",
        ),
        (
            json!([{"id":1,"result":null},{"id":2,"error":{"code":-5,"message":"secret"}}]),
            "node rejected request",
        ),
        (
            json!([{"id":1,"result":null},{"id":2}]),
            "missing JSON-RPC result",
        ),
        (
            json!([{"id":1,"result":null},{"id":2,"result":"secret"}]),
            "invalid JSON-RPC result",
        ),
    ] {
        let (config, task) = serve("200 OK", vec![(json!(requests), reply.to_string())], false)?;
        let result = Client::new(&config).tx_outs(&outpoints, false);
        task.join().map_err(|_| anyhow!("RPC mock panicked"))??;
        assert_eq!(
            result.err().context("invalid batch accepted")?.to_string(),
            expected
        );
    }
    Ok(())
}

#[test]
fn best_block_requires_hash_and_unsigned_height() -> Result<()> {
    let valid = json!({"hash":BLOCK,"height":123,"extra":"ignored"});
    let block: BlockLocation = serde_json::from_value(valid)?;
    assert_eq!(block.height, 123);
    assert_eq!(block.hash.to_string(), BLOCK);
    for value in [
        json!({"height":123}),
        json!({"hash":BLOCK}),
        json!({"hash":"zz".repeat(32),"height":123}),
        json!({"hash":BLOCK,"height":-1}),
        json!({"hash":BLOCK,"height":4_294_967_296_u64}),
    ] {
        assert!(
            serde_json::from_value::<BlockLocation>(value.clone()).is_err(),
            "{value}"
        );
    }
    Ok(())
}

#[test]
fn roots_schema_checks_shape_and_preserves_full_leaf_counts() -> Result<()> {
    for roots in [json!([]), Value::Null] {
        let state: Roots =
            serde_json::from_value(json!({"numleaves":0,"roots":roots,"extra":true}))?;
        assert_eq!(state.num_leaves, 0);
        assert!(state.roots.is_empty());
    }
    // The wire schema accepts u64; core applies its narrower accumulator bound.
    for num_leaves in [1_u64 << 63, u64::MAX] {
        let state: Roots = serde_json::from_value(
            json!({"numleaves":num_leaves,"roots":vec![BLOCK; usize::try_from(num_leaves.count_ones())?]}),
        )?;
        assert_eq!(state.num_leaves, num_leaves);
    }
    // Root-count consistency belongs to the accumulator domain constructor.
    for roots in [Value::Null, json!([BLOCK])] {
        let state: Roots = serde_json::from_value(json!({"numleaves":3,"roots":roots}))?;
        assert_eq!(state.num_leaves, 3);
    }
    for value in [
        json!({"numleaves":0}),
        json!({"roots":[]}),
        json!({"numleaves":-1,"roots":[]}),
        json!({"numleaves":1.5,"roots":[BLOCK]}),
        json!({"numleaves":1,"roots":["ab".repeat(31)]}),
        json!({"numleaves":1,"roots":["zz".repeat(32)]}),
    ] {
        assert!(
            serde_json::from_value::<Roots>(value.clone()).is_err(),
            "{value}"
        );
    }
    Ok(())
}
