use super::*;
use crate::test_support::{accept, read_http};
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

type Step = (&'static str, Value, Value);

fn fixture() -> Result<(OutPoint, Vec<Step>)> {
    let block = genesis_block(Network::Regtest);
    let hash = block.block_hash();
    let transaction = &block.txdata[0];
    let outpoint = OutPoint::new(transaction.compute_txid(), 0);
    let coin = Coin::new(outpoint, hash, 10, false, transaction.output[0].clone())?;
    let leaf = sha256d::Hash::from_byte_array(coin.leaf_hash());
    let mut proof_header = block.header;
    proof_header.prev_blockhash = hash;
    proof_header.time += 1;
    while proof_header.validate_pow(proof_header.target()).is_err() {
        proof_header.nonce = proof_header
            .nonce
            .checked_add(1)
            .context("nonce exhausted")?;
    }
    let proof_hash = proof_header.block_hash();
    // The output snapshot is at 20, but the proof selects 21. All context reads
    // use that proof's hash, with no later tip lookup or height-binding recheck.
    let steps = vec![
        (
            "gettxout",
            json!([outpoint.txid, 0, false]),
            json!({"bestblock":hash,"confirmations":11,"value":50,"coinbase":false,
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
            json!({"provedathash":proof_hash,"prooftargets":[0],"proofhashes":[],"hashesproven":[leaf]}),
        ),
        ("getblockhash", json!([0]), json!(hash)),
        (
            "getblockheader",
            json!([proof_hash, true]),
            json!({"hash":proof_hash,"height":21}),
        ),
        (
            "getblockheader",
            json!([proof_hash, false]),
            json!(serialize(&proof_header).as_hex().to_string()),
        ),
        (
            "getutreexoroots",
            json!([proof_hash]),
            json!({"numleaves":1,"roots":[coin.leaf_hash().as_hex().to_string()]}),
        ),
    ];
    Ok((outpoint, steps))
}

fn run(
    outpoint: OutPoint,
    minimum: u64,
    steps: Vec<Step>,
) -> Result<Result<(ChainContext, EligibleCoin)>> {
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
        Network::Regtest.try_into()?,
        None,
        MinimumAmount::try_from(minimum)?,
        &config,
    );
    task.join()
        .map_err(|_| anyhow::anyhow!("RPC mock panicked"))??;
    Ok(result)
}

#[test]
fn retrieval_checks_coin_and_proof_at_the_selected_block() -> Result<()> {
    let (outpoint, steps) = fixture()?;
    let (context, coin) = run(outpoint, 5_000_000_000, steps.clone())??;
    assert_eq!(context.unspent_since().height(), 21);
    assert_eq!(context.unspent_at().height(), 21);
    assert_eq!(
        json!(context.unspent_at().hash()),
        steps[3].2["provedathash"]
    );
    assert_eq!(coin.witness().coin().output().value.to_sat(), 5_000_000_000);
    let error = run(outpoint, 5_000_000_001, steps.clone())?
        .err()
        .context("coin below the minimum accepted")?;
    assert!(
        format!("{error:#}").contains("below the minimum"),
        "{error:#}"
    );
    for (index, field, value, length, reason) in [
        (
            5,
            "hash",
            json!(BlockHash::all_zeros()),
            6,
            "location does not match",
        ),
        (
            1,
            "hash",
            json!(BlockHash::all_zeros()),
            2,
            "location does not match",
        ),
        (
            3,
            "hashesproven",
            json!([BlockHash::all_zeros()]),
            4,
            "reconstructed coin",
        ),
        (3, "prooftargets", json!([1]), 8, "outside occupied forest"),
    ] {
        let mut changed = steps.clone();
        changed[index].2[field] = value;
        changed.truncate(length);
        let error = run(outpoint, 1, changed)?
            .err()
            .context("bad RPC data accepted")?;
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
    let mut changed = steps.clone();
    changed[7].2["roots"] = json!(["00".repeat(32)]);
    changed.truncate(8);
    let error = run(outpoint, 1, changed)?
        .err()
        .context("wrong roots accepted")?;
    assert!(format!("{error:#}").contains("membership mismatch"));
    for (replacement, reason, length) in [
        (Value::Null, "absent from the confirmed UTXO set", 1),
        (
            {
                let mut value = steps[0].2.clone();
                value["value"] = json!(21_000_001);
                value
            },
            "coin amount or script exceeds Bitcoin limits",
            3,
        ),
        (
            {
                let mut value = steps[0].2.clone();
                value["scriptPubKey"]["hex"] = json!("51".repeat(10001));
                value
            },
            "coin amount or script exceeds Bitcoin limits",
            3,
        ),
        (
            {
                let mut value = steps[0].2.clone();
                value["confirmations"] = json!(22);
                value
            },
            "confirmation count",
            2,
        ),
    ] {
        let mut changed = steps.clone();
        changed[0].2 = replacement;
        changed.truncate(length);
        let error = run(outpoint, 1, changed)?
            .err()
            .context("invalid UTXO accepted")?;
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
    Ok(())
}

#[test]
fn single_coin_binding_enforces_domain_rules_on_rpc_data() -> Result<()> {
    let block = genesis_block(Network::Regtest);
    let outpoint = OutPoint::new(block.txdata[0].compute_txid(), 0);
    let coin = Coin::new(
        outpoint,
        block.block_hash(),
        10,
        true,
        block.txdata[0].output[0].clone(),
    )?;
    let leaf = sha256d::Hash::from_byte_array(coin.leaf_hash());
    let response = json!({"provedathash":block.block_hash(),"prooftargets":[0],
        "hashesproven":[leaf],"proofhashes":[]});
    let witness = bind(coin.clone(), serde_json::from_value(response.clone())?)?;
    assert_eq!(witness.coin(), &coin);
    for (targets, leaves, hashes, reason) in [
        (json!([]), json!([]), json!([]), "expected one proof target"),
        (
            json!([]),
            json!([leaf]),
            json!([]),
            "expected one proof target",
        ),
        (json!([0]), json!([]), json!([]), "expected one proven leaf"),
        (
            json!([0]),
            json!([leaf, leaf]),
            json!([]),
            "expected one proven leaf",
        ),
        (
            json!([0, 1]),
            json!([leaf, leaf]),
            json!([]),
            "expected one proof target",
        ),
        (
            json!([0]),
            json!([BlockHash::all_zeros()]),
            json!([]),
            "reconstructed coin",
        ),
        (
            json!([u64::MAX]),
            json!([leaf]),
            json!([]),
            "supported forest range",
        ),
        (
            json!([0]),
            json!([leaf]),
            json!(vec![leaf; 62]),
            "supported forest range",
        ),
    ] {
        let mut invalid = response.clone();
        invalid["prooftargets"] = targets;
        invalid["hashesproven"] = leaves;
        invalid["proofhashes"] = hashes;
        let proof = serde_json::from_value(invalid)?;
        let error = bind(coin.clone(), proof)
            .err()
            .context("invalid proof binding accepted")?;
        assert!(error.to_string().contains(reason), "{error:#}");
    }
    assert_eq!(creation_height(1, 10)?, 10);
    assert_eq!(creation_height(2, 10)?, 9);
    assert_eq!(creation_height(1, u32::MAX)?, u32::MAX);
    assert!(
        creation_height(0, 10)
            .err()
            .context("unconfirmed coin accepted")?
            .to_string()
            .contains("not confirmed")
    );
    assert!(
        creation_height(12, 10)
            .err()
            .context("invalid age accepted")?
            .to_string()
            .contains("confirmation count")
    );
    Ok(())
}
