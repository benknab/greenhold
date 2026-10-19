use super::*;
use crate::test_support::{accept, read_http};
use anyhow::Context;
use bitcoin::{
    BlockHash, Network,
    blockdata::constants::genesis_block,
    consensus::serialize,
    hashes::{Hash, sha256d},
    hex::DisplayHex,
};
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    net::TcpListener,
    thread,
};
use url::Url;
use walledcity_core::coin::Coin;

type Step = (&'static str, Value, Value);

fn fixture() -> Result<(OutPoint, Vec<Step>)> {
    let block = genesis_block(Network::Regtest);
    let hash = block.block_hash();
    let transaction = &block.txdata[0];
    let outpoint = OutPoint::new(transaction.compute_txid(), 0);
    let coin = Coin::new(outpoint, hash, 10, true, transaction.output[0].clone())?;
    let leaf = sha256d::Hash::from_byte_array(coin.leaf_hash());
    let steps = vec![
        ("getblockhash", json!([0]), json!(hash)),
        ("getbestblock", json!([]), json!({"hash":hash,"height":20})),
        (
            "getblockheader",
            json!([hash, false]),
            json!(serialize(&block.header).as_hex().to_string()),
        ),
        (
            "getutreexoroots",
            json!([hash]),
            json!({"numleaves":1,"roots":[coin.leaf_hash().as_hex().to_string()]}),
        ),
        ("getblockhash", json!([20]), json!(hash)),
        (
            "gettxout",
            json!([outpoint.txid, 0, false]),
            json!({"bestblock":hash,"confirmations":11,"value":50,"coinbase":true,
                "scriptPubKey":{"hex":transaction.output[0].script_pubkey.as_bytes().as_hex().to_string()}}),
        ),
        (
            "getblockheader",
            json!([hash, true]),
            json!({"hash":hash,"height":20}),
        ),
        ("getblockhash", json!([10]), json!(hash)),
        (
            "proveutxochaintipinclusion",
            json!([[outpoint.txid], [0], 1]),
            json!({"provedathash":hash,"prooftargets":[0],"proofhashes":[],"hashesproven":[leaf]}),
        ),
        ("getblockhash", json!([10]), json!(hash)),
        ("getblockhash", json!([20]), json!(hash)),
        ("getblockhash", json!([20]), json!(hash)),
    ];
    Ok((outpoint, steps))
}

fn run(outpoint: OutPoint, steps: Vec<Step>) -> Result<Result<(ChainContext, CoinWitness)>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let config = utreexod_rpc::Config::new(
        Url::parse(&format!("http://{}/", listener.local_addr()?))?,
        None,
    );
    let task = thread::spawn(move || -> Result<()> {
        for (method, params, result) in steps {
            let (socket, _) = accept(&listener)?;
            let (_, request) = read_http(&mut BufReader::new(&socket))?;
            assert_eq!(request["method"], method);
            assert_eq!(request["params"], params);
            let body = json!({"id":request["id"],"result":result}).to_string();
            write!(
                &socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
        }
        Ok(())
    });
    let result = fetch(
        outpoint,
        context::Request::new(Network::Regtest, None, None)?,
        &config,
    );
    task.join()
        .map_err(|_| anyhow::anyhow!("RPC mock panicked"))??;
    Ok(result)
}

#[test]
fn retrieval_checks_coin_proof_and_final_chain_bindings() -> Result<()> {
    let (outpoint, steps) = fixture()?;
    let (context, witness) = run(outpoint, steps.clone())??;
    assert_eq!(witness.coin().output().value.to_sat(), 5_000_000_000);
    witness.verify(&context, outpoint)?;
    for index in [9, 10, 11] {
        let mut changed = steps.clone();
        changed[index].2 = json!(BlockHash::all_zeros());
        changed.truncate(index + 1);
        let error = run(outpoint, changed)?.err().context("reorg accepted")?;
        assert!(format!("{error:#}").contains("chain changed during coin retrieval"));
    }
    for (index, field, value, reason) in [
        (
            6,
            "hash",
            json!(BlockHash::all_zeros()),
            "location does not match",
        ),
        (
            8,
            "provedathash",
            json!(BlockHash::all_zeros()),
            "proof block does not match",
        ),
        (
            8,
            "hashesproven",
            json!([BlockHash::all_zeros()]),
            "reconstructed coin",
        ),
        (8, "prooftargets", json!([1]), "outside occupied forest"),
    ] {
        let mut changed = steps.clone();
        changed[index].2[field] = value;
        changed.truncate(index + 1);
        let error = run(outpoint, changed)?
            .err()
            .context("bad RPC data accepted")?;
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
    let mut changed = steps.clone();
    changed[3].2["roots"] = json!(["00".repeat(32)]);
    changed.truncate(9);
    let error = run(outpoint, changed)?
        .err()
        .context("wrong roots accepted")?;
    assert!(format!("{error:#}").contains("membership mismatch"));
    for (replacement, reason, length) in [
        (Value::Null, "absent from the confirmed UTXO set", 6),
        (
            {
                let mut value = steps[5].2.clone();
                value["confirmations"] = json!(22);
                value
            },
            "confirmation count",
            7,
        ),
    ] {
        let mut changed = steps.clone();
        changed[5].2 = replacement;
        changed.truncate(length);
        let error = run(outpoint, changed)?
            .err()
            .context("invalid UTXO accepted")?;
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
    Ok(())
}
