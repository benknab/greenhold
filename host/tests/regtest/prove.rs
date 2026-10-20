use crate::support::{
    MINING_ADDRESS, Node, TAPROOT_WALLET, WALLET, WALLET_ADDRESS, command, command_with_timeout,
    context_digest, image, inspect, rejection, success,
    transactions::{broadcast, mined, spend_signed},
};
use anyhow::{Context, Result};
use bitcoin::{Address, Amount, Network, OutPoint, Transaction, hex::FromHex};
use greenhold_core::claim::Claim;
use greenhold_guest_builder::GREENHOLD_GUEST_ID;
use risc0_zkvm::{InnerReceipt, Receipt, sha::Digest};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

/// The receipt file tag, written independently of the production encoder.
const TAG: &[u8] = b"greenhold/receipt/v1\0";

#[test]
#[ignore = "requires Docker; runs Utreexod nodes"]
fn prove_and_verify_use_real_transactions_and_reject_ineligible_coins() -> Result<()> {
    let (node, address, funding) = funded_node()?;
    let input = OutPoint::new(funding.compute_txid(), 0).to_string();
    // Prove with the test wallet's signature.
    let prove = |args: &[&str], timeout| {
        let args = [args, &WALLET].concat();
        command_with_timeout(&node, "regtest", "prove", &args, timeout)
    };
    let claim = |args: &[&str]| prove(&[args, &["--dry-run"]].concat(), Duration::from_secs(30));
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("claim.receipt");
    let receipt = path.to_str().context("receipt path")?;
    // The only real proof in this scenario. The other runs stop before proving.
    let report = success(&prove(
        &[
            &input,
            "433",
            "--first-height",
            "433",
            "--minimum-sats",
            "2000000000",
            "--output",
            receipt,
        ],
        Duration::from_mins(10),
    )?)?;
    assert_claim(&node, &report, 433, 433, 2_000_000_000)?;
    assert_receipt(&report, &fs::read(&path)?)?;
    let accepted = json!({
        "claim": report["claim"],
        "encoded": report["encoded"],
        "image_id": report["image_id"],
    });
    let verify =
        |node: &Node, network: &str, receipt: &str| command(node, network, "verify", &[receipt]);
    assert_eq!(success(&verify(&node, "regtest", receipt)?)?, accepted);
    assert_eq!(success(&inspect(receipt)?)?, accepted);
    rejection(
        &verify(&node, "bitcoin", receipt)?,
        "receipt claims regtest, not bitcoin",
    );
    let rejected_path = directory.path().join("rejected.receipt");
    let rejected = rejected_path.to_str().context("receipt path")?;
    rejection(
        &prove(
            &[&input, "--minimum-sats", "2000000001", "--output", rejected],
            Duration::from_secs(30),
        )?,
        "below the minimum",
    );
    assert!(
        !rejected_path.exists(),
        "a rejected coin must not write a receipt"
    );
    // Created at 335: T - 335 = 98 at 433, so block 434 could not spend it.
    let young = mined(&node, 335, &address.script_pubkey())?.to_string();
    rejection(&claim(&[&young])?, "coinbase coin is not spendable");
    rejection(&claim(&[&input, "--first-height", "432"])?, "cutoff");
    let bad_index = OutPoint::new(funding.compute_txid(), 99).to_string();
    rejection(&claim(&[&bad_index])?, "absent from the confirmed UTXO set");
    // A valid signature from another script does not cover this coin.
    rejection(
        &command(
            &node,
            "regtest",
            "prove",
            &[&[input.as_str(), "--dry-run"][..], &TAPROOT_WALLET].concat(),
        )?,
        "--address does not lock the coin",
    );

    node.rpc("generate", &json!([1]))?;
    let mature = success(&claim(&[&young, "--first-height", "335"])?)?;
    assert_claim(&node, &mature, 335, 434, 1)?;
    rejection(
        &claim(&[&input, "433"])?,
        "proof block height does not match requested last height",
    );
    let report = success(&claim(&[&input, "--first-height", "433"])?)?;
    assert_claim(&node, &report, 433, 434, 1)?;
    let outpoint = OutPoint::new(funding.compute_txid(), 0);
    let spending = spend_signed(
        outpoint,
        Amount::from_sat(2_000_000_000),
        &[1_999_990_000],
        &address.script_pubkey(),
    )?;
    broadcast(&node, &spending)?;
    node.rpc("generate", &json!([1]))?;
    rejection(&claim(&[&input])?, "absent from the confirmed UTXO set");
    // Deleting the first funding output changes the survivor's position/path.
    let survivor = OutPoint::new(funding.compute_txid(), 1).to_string();
    let report = success(&claim(&[&survivor, "--minimum-sats", "2999990000"])?)?;
    assert_claim(&node, &report, 435, 435, 2_999_990_000)?;

    // T is now historical, and the proved coin is spent. The claim was about T.
    assert_eq!(success(&verify(&node, "regtest", receipt)?)?, accepted);
    assert_verify_fails_closed(&node, &path)
}

/// A node with active segregated witness and a confirmed test wallet transaction at 433.
/// Its two outputs pay the test wallet's P2WPKH address.
fn funded_node() -> Result<(Node, Address, Transaction)> {
    let address = WALLET_ADDRESS
        .parse::<Address<_>>()?
        .require_network(Network::Regtest)?;
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
    // A regtest coinbase at height 1 pays the full 50 BTC subsidy.
    let funding = spend_signed(
        mined(&node, 1, &address.script_pubkey())?,
        Amount::from_sat(5_000_000_000),
        &[2_000_000_000, 2_999_990_000],
        &address.script_pubkey(),
    )?;
    broadcast(&node, &funding)?;
    node.rpc("generate", &json!([1]))?;
    Ok((node, address, funding))
}

/// Reject a changed receipt, another chain, and a node without historical roots.
fn assert_verify_fails_closed(node: &Node, path: &Path) -> Result<()> {
    let receipt = path.to_str().context("receipt path")?;
    let verify = |node: &Node, receipt: &str| command(node, "regtest", "verify", &[receipt]);
    // A changed journal no longer matches the seal.
    let mut tampered: Receipt = bincode::deserialize(&fs::read(path)?[TAG.len()..])?;
    *tampered.journal.bytes.last_mut().context("empty journal")? ^= 1;
    let mut file = TAG.to_vec();
    file.extend(bincode::serialize(&tampered)?);
    let tampered_path = path.with_file_name("tampered.receipt");
    fs::write(&tampered_path, file)?;
    let tampered = tampered_path.to_str().context("receipt path")?;
    rejection(&verify(node, tampered)?, "receipt verification failed");
    rejection(&inspect(tampered)?, "receipt verification failed");
    // Same network and heights, but a different chain: its blocks rebuild another claim.
    let other = Node::start(
        image()?,
        &[
            "--utreexoproofindex".into(),
            "--utreexoproofindexmaxmemory=250".into(),
            format!("--miningaddr={MINING_ADDRESS}"),
        ],
        None,
    )?;
    other.rpc("generate", &json!([433]))?;
    rejection(
        &verify(&other, receipt)?,
        "receipt claim does not match the node's chain state",
    );
    // Same chain, but deliberately no accumulator index. Verification must fail.
    let unindexed = Node::start(image()?, &["--noutreexo".into()], Some(node))?;
    unindexed.wait_for_block(node, 435)?;
    rejection(
        &verify(&unindexed, receipt)?,
        "cannot retrieve Utreexo state",
    );
    Ok(())
}

/// Decode the receipt file independently and check that it proves the reported claim.
fn assert_receipt(report: &Value, file: &[u8]) -> Result<()> {
    assert_eq!(report["receipt_bytes"], file.len());
    let encoded = file.strip_prefix(TAG).context("receipt tag")?;
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
    assert_eq!(claim["context_digest"], context_digest(node, first, last)?);
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
