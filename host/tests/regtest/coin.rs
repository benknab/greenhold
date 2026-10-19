use crate::support::{Node, command, context, image, rejection, success};
use anyhow::{Context, Result};
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Transaction, TxIn, TxOut, Witness,
    absolute::LockTime,
    consensus::{deserialize, serialize},
    hex::{DisplayHex, FromHex},
    transaction::Version,
};
use greenhold_core::{coin::CoinWitness, context::ChainContext};
use serde_json::{Value, json};

fn spend(outpoint: OutPoint, values: &[u64], script: &ScriptBuf) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            witness: Witness::from_slice(&[vec![0x51]]),
            ..TxIn::default()
        }],
        output: values
            .iter()
            .map(|value| TxOut {
                value: Amount::from_sat(*value),
                script_pubkey: script.clone(),
            })
            .collect(),
    }
}

fn broadcast(node: &Node, transaction: &Transaction) -> Result<()> {
    node.rpc(
        "sendrawtransaction",
        &json!([serialize(transaction).as_hex().to_string()]),
    )?;
    Ok(())
}

#[test]
#[ignore = "requires Docker; runs Utreexod nodes"]
fn single_coin_membership_uses_real_transactions_and_rejects_spent_coins() -> Result<()> {
    let script = ScriptBuf::from_bytes(vec![0x51]);
    let address = Address::p2wsh(&script, Network::Regtest);
    let node = Node::start(
        image()?,
        &[
            "--utreexoproofindex".into(),
            "--utreexoproofindexmaxmemory=250".into(),
            format!("--miningaddr={address}"),
        ],
        None,
    )?;
    // Three regtest versionbits windows activate SegWit and mature the coinbase.
    node.rpc("generate", &json!([432]))?;
    let hash = node.rpc("getblockhash", &json!([1]))?;
    let raw = node.rpc("getblock", &json!([hash, 0]))?;
    let block: bitcoin::Block =
        deserialize(&Vec::<u8>::from_hex(raw.as_str().context("raw block")?)?)?;
    let coinbase = &block.txdata[0];
    let txid = coinbase.compute_txid();
    let vout = coinbase
        .output
        .iter()
        .position(|output| output.script_pubkey == address.script_pubkey())
        .context("mining output")?;
    let funding = spend(
        OutPoint::new(txid, u32::try_from(vout)?),
        &[2_000_000_000, 2_999_990_000],
        &address.script_pubkey(),
    );
    broadcast(&node, &funding)?;
    node.rpc("generate", &json!([1]))?;
    let outpoint = OutPoint::new(funding.compute_txid(), 0);
    let input = outpoint.to_string();
    let report = success(&command(
        &node,
        "regtest",
        "coin",
        &[&input, "433", "--first-height", "433"],
    )?)?;
    assert_coin(&report, &funding, outpoint, 433)?;
    rejection(
        &command(&node, "regtest", "coin", &[&input, "--first-height", "432"])?,
        "cutoff",
    );
    let bad_index = OutPoint::new(funding.compute_txid(), 99).to_string();
    rejection(
        &command(&node, "regtest", "coin", &[&bad_index])?,
        "absent from the confirmed UTXO set",
    );

    node.rpc("generate", &json!([1]))?;
    verify_after_tip_advance(&node, &report, outpoint)?;
    rejection(
        &command(&node, "regtest", "coin", &[&input, "433"])?,
        "proof block height does not match requested last height",
    );
    let report = success(&command(
        &node,
        "regtest",
        "coin",
        &[&input, "--first-height", "433"],
    )?)?;
    assert_coin(&report, &funding, outpoint, 434)?;
    let spending = spend(outpoint, &[1_999_990_000], &address.script_pubkey());
    broadcast(&node, &spending)?;
    node.rpc("generate", &json!([1]))?;
    rejection(
        &command(&node, "regtest", "coin", &[&input])?,
        "absent from the confirmed UTXO set",
    );
    // Deleting the first funding output changes the survivor's position/path.
    let survivor = OutPoint::new(funding.compute_txid(), 1);
    let report = success(&command(
        &node,
        "regtest",
        "coin",
        &[&survivor.to_string()],
    )?)?;
    assert_coin(&report, &funding, survivor, 435)?;
    Ok(())
}

fn verify_after_tip_advance(node: &Node, report: &Value, outpoint: OutPoint) -> Result<()> {
    // Fetch fresh public state at the proof's block after the node has advanced.
    let historical = success(&context(node, "regtest", &["433"])?)?;
    let fetched: ChainContext = serde_json::from_value(historical["context"].clone())?;
    let witness: CoinWitness = serde_json::from_value(report["witness"].clone())?;
    witness.verify(&fetched, outpoint)?;
    assert_eq!(
        historical["last_block_hash"],
        report["witness"]["proved_at"]
    );
    Ok(())
}

fn assert_coin(
    report: &Value,
    transaction: &Transaction,
    outpoint: OutPoint,
    height: u32,
) -> Result<()> {
    assert_eq!(
        report["amount_sats"],
        transaction.output[usize::try_from(outpoint.vout)?]
            .value
            .to_sat()
    );
    assert_eq!(report["context"]["last"]["height"], height);
    assert_eq!(report["witness"]["coin"]["creation_height"], 433);
    assert_eq!(report["witness"]["coin"]["coinbase"], false);
    let context: ChainContext = serde_json::from_value(report["context"].clone())?;
    let witness: CoinWitness = serde_json::from_value(report["witness"].clone())?;
    witness.verify(&context, outpoint)?;
    let mut corrupt = report["witness"].clone();
    corrupt["coin"]["output"]["value"] = json!(123);
    let corrupt: CoinWitness = serde_json::from_value(corrupt)?;
    assert!(
        corrupt
            .verify(&context, outpoint)
            .err()
            .context("tampered amount accepted")?
            .to_string()
            .contains("membership mismatch")
    );
    Ok(())
}
