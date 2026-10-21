use crate::support::{
    MINING_ADDRESS, Node, WALLET_ADDRESS, command, command_with_timeout, context_digest, image,
    inspect, rejection, sign_claim, success,
    transactions::{broadcast, mined, spend_signed},
};
use anyhow::{Context, Result};
use bitcoin::{Address, Amount, Network, OutPoint, Transaction};
use greenhold_core::claim::Claim;
use greenhold_guest_builder::GREENHOLD_GUEST_ID;
use risc0_zkvm::{InnerReceipt, Receipt, sha::Digest};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

const TAG: &[u8] = b"greenhold/receipt/v1\0";

#[test]
#[ignore = "requires Docker; runs Utreexod nodes"]
fn prepared_claim_survives_signing_delay_and_spending_but_not_a_reorg() -> Result<()> {
    let (node, address, funding) = funded_node()?;
    let input = OutPoint::new(funding.compute_txid(), 0).to_string();
    let directory = tempfile::tempdir()?;
    let witness_path = directory.path().join("coin.witness");
    let claim_path = directory.path().join("coin.claim");
    let receipt_path = directory.path().join("claim.receipt");
    let witness = witness_path.to_str().context("witness path")?;
    let claim = claim_path.to_str().context("claim path")?;
    let receipt = receipt_path.to_str().context("receipt path")?;
    let report = success(&command(
        &node,
        "regtest",
        "claim",
        &[
            "--json",
            "--output",
            claim,
            "--outpoint",
            &input,
            "--first-height",
            "433",
            "--minimum-sats",
            "2000000000",
            "--witness",
            witness,
        ],
    )?)?;
    assert_claim(&node, &report, 433, 433, 2_000_000_000)?;
    assert_claim_file(&claim_path, &report)?;
    assert_preparation_rejects_ineligible(&node, &address, &funding, directory.path())?;

    // Sign after advancing the tip. No fresh proof for the frozen T can be fetched.
    node.rpc("generate", &json!([1]))?;
    let signature = sign_claim(&report, &claim_path)?;
    let dry_run = || prove_dry(&node, claim, witness, &signature);
    let executed_output = dry_run()?;
    assert_progress(&executed_output, false)?;
    let executed = success(&executed_output)?;
    assert_eq!(executed["claim"], report["claim"]);
    assert_new_claim_uses_tip(&node, &input, directory.path())?;
    assert_human_dry_run(&node, claim, witness, &signature, &report)?;

    // Spend after T, then prove from the saved witness after the spend is confirmed.
    broadcast(
        &node,
        &spend_signed(
            OutPoint::new(funding.compute_txid(), 0),
            Amount::from_sat(2_000_000_000),
            &[1_999_990_000],
            &address.script_pubkey(),
        )?,
    )?;
    node.rpc("generate", &json!([1]))?;
    assert!(
        node.rpc("gettxout", &json!([funding.compute_txid(), 0, false]))?
            .is_null()
    );
    let proved_output = command_with_timeout(
        &node,
        "regtest",
        "prove",
        &[
            "--json",
            "--claim",
            claim,
            "--witness",
            witness,
            "--signature",
            &signature,
            "--output",
            receipt,
        ],
        Duration::from_mins(10),
    )?;
    assert_progress(&proved_output, true)?;
    let proved = success(&proved_output)?;
    assert_receipt(&proved, &fs::read(receipt)?, &fs::read(claim)?)?;
    assert!(proved.get("cycles").is_none());
    let accepted = json!({"claim": report["claim"],
        "message": report["message"], "image_id": report["image_id"]});
    assert_verified_reports(&node, receipt, claim, &accepted)?;
    assert_verify_fails_closed(&node, &receipt_path, claim)?;

    // A completed reorg across T invalidates chain acceptance, not the proof seal.
    node.rpc("invalidateblock", &json!([report["claim"]["last"]["hash"]]))?;
    node.rpc("generate", &json!([4]))?;
    rejection(
        &command(&node, "regtest", "verify", &["--receipt", receipt])?,
        "claim does not match the node's chain state",
    );
    rejection(&dry_run()?, "claim does not match the node's chain state");
    assert_public_report(&success(&inspect(receipt)?)?, &accepted);
    Ok(())
}

fn prove_dry(
    node: &Node,
    claim: &str,
    witness: &str,
    signature: &str,
) -> Result<std::process::Output> {
    command(
        node,
        "regtest",
        "prove",
        &[
            "--json",
            "--claim",
            claim,
            "--witness",
            witness,
            "--signature",
            signature,
            "--dry-run",
        ],
    )
}

fn assert_new_claim_uses_tip(node: &Node, input: &str, directory: &Path) -> Result<()> {
    let witness = directory.join("later.witness");
    let claim = directory.join("later.claim");
    let base = [
        "--outpoint",
        input,
        "--witness",
        witness.to_str().context("path")?,
        "--output",
        claim.to_str().context("path")?,
    ];
    for (extra, first) in [
        (vec!["--json"], 434),
        (vec!["--json", "--first-height", "433"], 433),
    ] {
        let report = success(&command(
            node,
            "regtest",
            "claim",
            &[base.as_slice(), extra.as_slice()].concat(),
        )?)?;
        assert_claim(node, &report, first, 434, 1)?;
        assert_claim_file(&claim, &report)?;
        assert!(witness.exists());
    }
    let output = command(node, "regtest", "claim", &base)?;
    let bytes = fs::read(&claim)?;
    let parsed = Claim::try_from(bytes.as_slice())?;
    assert_eq!(parsed.first_height(), 434);
    assert_human_report(&output, &json!({"claim": parsed}))?;
    assert!(std::str::from_utf8(&output.stdout)?.contains(&parsed.signing_message()));
    Ok(())
}

fn assert_human_report(output: &std::process::Output, report: &Value) -> Result<()> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::str::from_utf8(&output.stdout)?;
    assert!(serde_json::from_str::<Value>(text).is_err(), "{text}");
    assert!(text.contains("regtest"), "{text}");
    for (label, value) in [
        ("Minimum amount", &report["claim"]["minimum_sats"]),
        ("First height", &report["claim"]["first"]["height"]),
        ("Last height", &report["claim"]["last"]["height"]),
    ] {
        assert!(text.contains(&format!("{label}: {value}")), "{text}");
    }
    Ok(())
}

fn assert_human_dry_run(
    node: &Node,
    claim: &str,
    witness: &str,
    signature: &str,
    report: &Value,
) -> Result<()> {
    let output = command(
        node,
        "regtest",
        "prove",
        &[
            "--claim",
            claim,
            "--witness",
            witness,
            "--signature",
            signature,
            "--dry-run",
        ],
    )?;
    assert_human_report(&output, report)
}

fn assert_verified_reports(
    node: &Node,
    receipt: &str,
    claim: &str,
    expected: &Value,
) -> Result<()> {
    for extra in [vec!["--claim", claim], vec![]] {
        let report = success(&command(
            node,
            "regtest",
            "verify",
            &[&["--json", "--receipt", receipt], extra.as_slice()].concat(),
        )?)?;
        assert_public_report(&report, expected);
        assert_eq!(report["expected_claim_checked"], !extra.is_empty());
    }
    assert_public_report(&success(&inspect(receipt)?)?, expected);
    assert_human_report(
        &command(node, "regtest", "verify", &["--receipt", receipt])?,
        expected,
    )?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args(["inspect", "--receipt", receipt])
        .output()?;
    assert_human_report(&output, expected)
}

fn assert_public_report(actual: &Value, expected: &Value) {
    for field in ["claim", "message", "image_id"] {
        assert_eq!(actual[field], expected[field], "{field}");
    }
    assert!(actual.get("encoded").is_none());
}

/// Captured progress stays on stderr and contains no terminal control sequences.
fn assert_progress(output: &std::process::Output, proved: bool) -> Result<()> {
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert!(!stderr.contains(['\u{1b}', '\r']), "{stderr}");
    let stages = [
        "Checking signature",
        "Checking chain state",
        "Executing guest",
        "Generating proof",
        "Checking receipt",
        "Saving receipt",
    ];
    let count = if proved { stages.len() } else { 3 };
    let mut remaining = stderr;
    for stage in &stages[..count] {
        let (_, rest) = remaining
            .split_once(&format!("{stage}...\n"))
            .context("missing stage start")?;
        let (_, rest) = rest
            .split_once(&format!("{stage}: done ("))
            .context("missing stage completion")?;
        remaining = rest;
    }
    if !proved {
        assert!(!stderr.contains("Generating proof"), "{stderr}");
    }
    Ok(())
}

fn assert_claim_file(path: &Path, report: &Value) -> Result<()> {
    let bytes = fs::read(path)?;
    assert_eq!(bytes.len(), 163);
    assert!(bytes.starts_with(b"greenhold/claim/v1\0"));
    let claim = Claim::try_from(bytes.as_slice())?;
    assert_eq!(serde_json::to_value(&claim)?, report["claim"]);
    assert_eq!(claim.signing_message(), report["message"]);
    assert!(report.get("encoded").is_none());
    Ok(())
}

fn assert_preparation_rejects_ineligible(
    node: &Node,
    address: &Address,
    funding: &Transaction,
    directory: &Path,
) -> Result<()> {
    let witness = directory.join("rejected.witness");
    let claim = directory.join("rejected.claim");
    let outpoint = OutPoint::new(funding.compute_txid(), 0).to_string();
    let young = mined(node, 335, &address.script_pubkey())?.to_string();
    let missing = OutPoint::new(funding.compute_txid(), 99).to_string();
    for (args, reason) in [
        (
            vec![
                "--outpoint",
                outpoint.as_str(),
                "--minimum-sats",
                "2000000001",
            ],
            "below the minimum",
        ),
        (
            vec!["--outpoint", outpoint.as_str(), "--first-height", "432"],
            "cutoff",
        ),
        (
            vec!["--outpoint", outpoint.as_str(), "--first-height", "434"],
            "first height must not exceed last height",
        ),
        (
            vec!["--outpoint", young.as_str()],
            "coinbase coin is not spendable",
        ),
        (
            vec!["--outpoint", missing.as_str()],
            "absent from the confirmed UTXO set",
        ),
    ] {
        rejection(
            &command(
                node,
                "regtest",
                "claim",
                &[
                    args.as_slice(),
                    &[
                        "--witness",
                        witness.to_str().context("path")?,
                        "--output",
                        claim.to_str().context("path")?,
                    ],
                ]
                .concat(),
            )?,
            reason,
        );
        assert!(!witness.exists());
        assert!(!claim.exists());
    }
    Ok(())
}

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
    node.rpc("generate", &json!([432]))?;
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

fn assert_verify_fails_closed(node: &Node, path: &Path, claim: &str) -> Result<()> {
    let receipt = path.to_str().context("receipt path")?;
    // A real proof for C must not satisfy even a weaker but otherwise valid C-prime.
    let mut changed = fs::read(claim)?;
    changed[123..131].copy_from_slice(&1_u64.to_le_bytes());
    let expected = path.with_file_name("different.claim");
    fs::write(&expected, changed)?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args([
            "verify",
            "--receipt",
            receipt,
            "--claim",
            expected.to_str().context("expected claim path")?,
            "--network",
            "regtest",
            "--utreexod-rpc",
            "http://127.0.0.1:9/",
            "--utreexod-rpc-user",
            "test",
            "--utreexod-rpc-password",
            "test",
        ])
        .output()?;
    rejection(&output, "receipt does not match the expected claim");
    rejection(
        &command(
            node,
            "bitcoin",
            "verify",
            &["--receipt", receipt, "--claim", claim],
        )?,
        "receipt claims regtest, not bitcoin",
    );
    let mut tampered: Receipt = bincode::deserialize(&fs::read(path)?[TAG.len()..])?;
    *tampered.journal.bytes.last_mut().context("empty journal")? ^= 1;
    let mut file = TAG.to_vec();
    file.extend(bincode::serialize(&tampered)?);
    let tampered_path = path.with_file_name("tampered.receipt");
    fs::write(&tampered_path, file)?;
    let tampered = tampered_path.to_str().context("receipt path")?;
    rejection(
        &command(
            node,
            "regtest",
            "verify",
            &["--receipt", tampered, "--claim", claim],
        )?,
        "receipt verification failed",
    );
    rejection(&inspect(tampered)?, "receipt verification failed");
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
        &command(
            &other,
            "regtest",
            "verify",
            &["--receipt", receipt, "--claim", claim],
        )?,
        "claim does not match the node's chain state",
    );
    let unindexed = Node::start(image()?, &["--noutreexo".into()], Some(node))?;
    unindexed.wait_for_block(node, 435)?;
    rejection(
        &command(
            &unindexed,
            "regtest",
            "verify",
            &["--receipt", receipt, "--claim", claim],
        )?,
        "cannot retrieve Utreexo state",
    );
    Ok(())
}

fn assert_receipt(report: &Value, file: &[u8], claim: &[u8]) -> Result<()> {
    assert_eq!(report["receipt_bytes"], file.len());
    let receipt: Receipt = bincode::deserialize(file.strip_prefix(TAG).context("receipt tag")?)?;
    assert!(matches!(receipt.inner, InnerReceipt::Succinct(_)));
    receipt.verify(GREENHOLD_GUEST_ID)?;
    assert_eq!(receipt.journal.bytes, claim);
    Ok(())
}

fn assert_claim(node: &Node, report: &Value, first: u32, last: u32, minimum: u64) -> Result<()> {
    let claim = &report["claim"];
    assert_eq!(claim["network"], "regtest");
    for (block, height) in [(&claim["first"], first), (&claim["last"], last)] {
        assert_eq!(block["height"], height);
        assert_eq!(block["hash"], node.rpc("getblockhash", &json!([height]))?);
    }
    assert_eq!(claim["minimum_sats"], minimum);
    assert_eq!(claim["context_digest"], context_digest(node, first, last)?);
    assert!(claim.get("message_digest").is_none());
    assert_eq!(
        report["image_id"],
        Digest::from(GREENHOLD_GUEST_ID).to_string()
    );
    assert!(report.get("encoded").is_none());
    Ok(())
}
