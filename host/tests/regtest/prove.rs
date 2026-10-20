use crate::support::{Node, command, command_with_timeout, context, image, rejection, success};
use anyhow::{Context, Result};
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Transaction, TxIn, TxOut, Witness,
    absolute::LockTime,
    consensus::{deserialize, serialize},
    hex::{DisplayHex, FromHex},
    transaction::Version,
};
use greenhold_core::claim::Claim;
use greenhold_guest_builder::GREENHOLD_GUEST_ID;
use risc0_zkvm::{InnerReceipt, Receipt, sha::Digest};
use serde_json::{Value, json};
use std::{fs, time::Duration};

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

fn mined(node: &Node, height: u32, script: &ScriptBuf) -> Result<OutPoint> {
    let hash = node.rpc("getblockhash", &json!([height]))?;
    let raw = node.rpc("getblock", &json!([hash, 0]))?;
    let block: bitcoin::Block =
        deserialize(&Vec::<u8>::from_hex(raw.as_str().context("raw block")?)?)?;
    let coinbase = &block.txdata[0];
    let vout = coinbase
        .output
        .iter()
        .position(|output| output.script_pubkey == *script)
        .context("mining output")?;
    Ok(OutPoint::new(coinbase.compute_txid(), u32::try_from(vout)?))
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
fn prove_uses_real_transactions_and_rejects_ineligible_coins() -> Result<()> {
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
    let funding = spend(
        mined(&node, 1, &address.script_pubkey())?,
        &[2_000_000_000, 2_999_990_000],
        &address.script_pubkey(),
    );
    broadcast(&node, &funding)?;
    node.rpc("generate", &json!([1]))?;
    let input = OutPoint::new(funding.compute_txid(), 0).to_string();
    let claim =
        |args: &[&str]| command(&node, "regtest", "prove", &[args, &["--dry-run"]].concat());
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("claim.receipt");
    let output = path.to_str().context("receipt path")?;
    // The only real proof in this scenario. The other runs stop before proving.
    let report = success(&command_with_timeout(
        &node,
        "regtest",
        "prove",
        &[
            &input,
            "433",
            "--first-height",
            "433",
            "--minimum-sats",
            "2000000000",
            "--output",
            output,
        ],
        Duration::from_mins(10),
    )?)?;
    assert_claim(&node, &report, 433, 433, 2_000_000_000)?;
    assert_receipt(&report, &fs::read(&path)?)?;
    fs::remove_file(&path)?;
    rejection(
        &command(
            &node,
            "regtest",
            "prove",
            &[&input, "--minimum-sats", "2000000001", "--output", output],
        )?,
        "below the minimum",
    );
    assert!(!path.exists(), "a rejected coin must not write a receipt");
    // Created at 335: T - 335 = 98 at 433, so block 434 could not spend it.
    let young = mined(&node, 335, &address.script_pubkey())?.to_string();
    rejection(&claim(&[&young])?, "coinbase coin is not spendable");
    rejection(&claim(&[&input, "--first-height", "432"])?, "cutoff");
    let bad_index = OutPoint::new(funding.compute_txid(), 99).to_string();
    rejection(&claim(&[&bad_index])?, "absent from the confirmed UTXO set");

    node.rpc("generate", &json!([1]))?;
    // A verifier rebuilds the claim's state from its node after the tip moves on.
    assert_claim(&node, &report, 433, 433, 2_000_000_000)?;
    let mature = success(&claim(&[&young, "--first-height", "335"])?)?;
    assert_claim(&node, &mature, 335, 434, 1)?;
    rejection(
        &claim(&[&input, "433"])?,
        "proof block height does not match requested last height",
    );
    let report = success(&claim(&[&input, "--first-height", "433"])?)?;
    assert_claim(&node, &report, 433, 434, 1)?;
    let outpoint = OutPoint::new(funding.compute_txid(), 0);
    let spending = spend(outpoint, &[1_999_990_000], &address.script_pubkey());
    broadcast(&node, &spending)?;
    node.rpc("generate", &json!([1]))?;
    rejection(&claim(&[&input])?, "absent from the confirmed UTXO set");
    // Deleting the first funding output changes the survivor's position/path.
    let survivor = OutPoint::new(funding.compute_txid(), 1).to_string();
    let report = success(&claim(&[&survivor, "--minimum-sats", "2999990000"])?)?;
    assert_claim(&node, &report, 435, 435, 2_999_990_000)?;
    Ok(())
}

/// Decode the receipt file independently and check that it proves the reported claim.
fn assert_receipt(report: &Value, file: &[u8]) -> Result<()> {
    assert_eq!(report["receipt_bytes"], file.len());
    let encoded = file
        .strip_prefix(b"greenhold/receipt/v1\0")
        .context("receipt tag")?;
    let receipt: Receipt = bincode::deserialize(encoded)?;
    // Composite receipts leak the execution length, which depends on the coin.
    assert!(matches!(receipt.inner, InnerReceipt::Succinct(_)));
    receipt.verify(GREENHOLD_GUEST_ID)?;
    let claim = Vec::<u8>::from_hex(report["encoded"].as_str().context("encoded claim")?)?;
    assert_eq!(receipt.journal.bytes, claim);
    Ok(())
}

/// Check the public claim against the node, and its two output forms against each other.
fn assert_claim(node: &Node, report: &Value, first: u32, last: u32, minimum: u64) -> Result<()> {
    let claim = &report["claim"];
    assert_eq!(claim["network"], "regtest");
    for (block, height) in [(&claim["first"], first), (&claim["last"], last)] {
        assert_eq!(block["height"], height);
        assert_eq!(block["hash"], node.rpc("getblockhash", &json!([height]))?);
    }
    assert_eq!(claim["minimum_sats"], minimum);
    let fetched = success(&context(
        node,
        "regtest",
        &[&last.to_string(), "--first-height", &first.to_string()],
    )?)?;
    assert_eq!(claim["context_digest"], fetched["context_digest"]);
    // The claim came from the guest's journal, so the run must name this guest build.
    let image_id = Digest::from(GREENHOLD_GUEST_ID).to_string();
    assert_eq!(report["image_id"], image_id);
    // The cycle count depends on the coin, so a report beside a shareable receipt omits it.
    if report.get("receipt").is_some() {
        assert!(report.get("cycles").is_none());
    } else {
        assert!(report["cycles"].as_u64().context("cycle count")? > 0);
    }
    let encoded = Vec::<u8>::from_hex(report["encoded"].as_str().context("encoded claim")?)?;
    assert_eq!(
        &serde_json::to_value(Claim::try_from(encoded.as_slice())?)?,
        claim
    );
    Ok(())
}
