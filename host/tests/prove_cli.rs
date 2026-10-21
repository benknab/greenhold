use anyhow::Result;
use greenhold_core::coin::CoinWitness;
use greenhold_signer::{Format, ScriptType};
use std::{fs, process::Command};

#[path = "../src/artifacts.rs"]
mod artifacts;
#[path = "fixtures/claim.rs"]
mod fixture;

#[test]
fn claim_and_prove_explain_the_prepared_signing_flow() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    for (command, required) in [
        (
            "claim",
            vec![
                "--outpoint <OUTPOINT>",
                "--unspent-since <HEIGHT>",
                "tip-only",
                "--minimum-sats <SATS>",
                "--nonce <TEXT>",
                "--witness <FILE>",
                "--output <FILE>",
            ],
        ),
        (
            "prove",
            vec![
                "--claim <FILE>",
                "--witness <FILE>",
                "--signature <SIGNATURE>",
                "--dry-run",
            ],
        ),
        ("verify", vec!["--receipt <FILE>", "--nonce <TEXT>"]),
        ("inspect", vec!["--receipt <FILE>"]),
    ] {
        let output = Command::new(binary).args([command, "--help"]).output()?;
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout)?;
        assert!(help.contains("--json"), "{help}");
        assert!(!help.contains("--unspent-at"), "{help}");
        assert!(!help.contains("--claim-file"), "{help}");
        if command == "verify" {
            assert!(!help.contains("--claim"), "{help}");
        }
        for text in required {
            assert!(help.contains(text), "{help}");
        }
    }
    Ok(())
}

#[test]
fn prove_rejects_a_separate_nonce_option() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args(["prove", "--claim", "coin.claim"])
        .args([
            "--nonce",
            &"cd".repeat(32),
            "--witness",
            "missing.witness",
            "--signature",
            "smpAA==",
            "--dry-run",
        ])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("unexpected argument '--nonce'"));
    Ok(())
}

#[test]
fn claim_rejects_an_explicit_unspent_at_option() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let witness = directory.path().join("coin.witness");
    let claim = directory.path().join("coin.claim");
    let output = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args([
            "claim",
            "--outpoint",
            "1111111111111111111111111111111111111111111111111111111111111111:0",
            "--unspent-at",
            "200",
            "--witness",
        ])
        .arg(&witness)
        .arg("--output")
        .arg(&claim)
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("unexpected argument '--unspent-at'"),
        "{stderr}"
    );
    assert!(!witness.exists());
    assert!(!claim.exists());
    Ok(())
}

#[test]
fn prove_checks_the_claim_signature_before_node_access() -> Result<()> {
    let (input, claim) = fixture::signed(ScriptType::P2wpkh, Format::Simple)?;
    let input = serde_json::to_value(input)?;
    let witness: CoinWitness = serde_json::from_value(input["witness"].clone())?;
    let directory = tempfile::tempdir()?;
    let witness_path = directory.path().join("coin.witness");
    let claim_path = directory.path().join("coin.claim");
    fs::write(&witness_path, artifacts::encode_witness(&witness)?)?;
    let signature = input["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("signature"))?;
    for (changed, signature, network, reason) in [
        (None, signature, "regtest", "RPC transport failed"),
        (
            Some(123),
            signature,
            "regtest",
            "BIP-322 signature check failed",
        ),
        (
            Some(163),
            signature,
            "regtest",
            "BIP-322 signature check failed",
        ),
        (None, "smpAA==", "regtest", "BIP-322 signature check failed"),
        (
            None,
            signature,
            "bitcoin",
            "claim names regtest, not bitcoin",
        ),
    ] {
        let mut bytes = claim.to_bytes();
        if let Some(offset) = changed {
            bytes[offset] ^= 1;
        } // Change only M or the nonce, preserving a valid claim format.
        fs::write(&claim_path, bytes)?;
        let output = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
            .arg("prove")
            .arg("--claim")
            .arg(&claim_path)
            .arg("--witness")
            .arg(&witness_path)
            .args([
                "--signature",
                signature,
                "--network",
                network,
                "--dry-run",
                "--utreexod-rpc",
                "http://127.0.0.1:9/",
                "--utreexod-rpc-user",
                "test",
                "--utreexod-rpc-password",
                "test",
            ])
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(reason), "{stderr}");
        assert!(!stderr.contains('\u{1b}'), "{stderr}");
        if reason == "BIP-322 signature check failed" {
            assert!(stderr.contains("Checking signature..."), "{stderr}");
            assert!(!stderr.contains("Checking signature: done"), "{stderr}");
            assert!(!stderr.contains("Checking chain state"), "{stderr}");
        }
        if changed.is_some() {
            assert!(!stderr.contains("RPC transport failed"));
        }
    }
    Ok(())
}

#[test]
fn artifacts_reject_invalid_versions_trailing_bytes_and_oversize_files() -> Result<()> {
    let (input, claim) = fixture::signed(ScriptType::P2tr, Format::Simple)?;
    let input = serde_json::to_value(input)?;
    let witness: CoinWitness = serde_json::from_value(input["witness"].clone())?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("artifact");
    let valid = artifacts::encode_witness(&witness)?;
    fs::write(&path, &valid)?;
    artifacts::read_witness(&path)?
        .verify_membership(&serde_json::from_value(input["context"].clone())?)?;
    for bytes in [
        b"greenhold/witness/v2\0".to_vec(),
        [&valid[..], &[0]].concat(),
        valid[..valid.len() - 1].to_vec(),
        vec![0; 16 * 1024 + 1],
    ] {
        fs::write(&path, bytes)?;
        assert!(artifacts::read_witness(&path).is_err());
    }
    let encoded = artifacts::encode_claim(&claim);
    assert_eq!(encoded.len(), 195);
    assert_eq!(encoded, claim.to_bytes());
    fs::write(&path, &encoded)?;
    assert_eq!(artifacts::read_claim(&path)?, claim);
    for bytes in [
        claim.to_bytes()[..194].to_vec(),
        [claim.to_bytes(), vec![0]].concat(),
        [claim.to_bytes(), vec![0; 32]].concat(),
        vec![0; 195],
        b"greenhold/claim/v1\0".to_vec(),
        serde_json::to_vec(&claim)?,
    ] {
        fs::write(&path, bytes)?;
        assert!(artifacts::read_claim(&path).is_err());
    }
    Ok(())
}

#[test]
fn prove_rejects_malformed_claim_files_before_witness_or_node_access() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let claim = directory.path().join("coin.claim");
    for (contents, reason) in [
        (None, "cannot read claim"),
        (
            Some(b"greenhold/claim/v1\0".as_slice()),
            "claim tag or version is not supported",
        ),
        (
            Some(b"greenhold/claim/v2\0".as_slice()),
            "claim is truncated",
        ),
    ] {
        if let Some(contents) = contents {
            fs::write(&claim, contents)?;
        }
        let output = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
            .args(["prove", "--claim"])
            .arg(&claim)
            .args([
                "--witness",
                "missing.witness",
                "--signature",
                "smpAA==",
                "--dry-run",
                "--utreexod-rpc",
                "http://127.0.0.1:9/",
                "--utreexod-rpc-user",
                "test",
                "--utreexod-rpc-password",
                "test",
            ])
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(reason), "{stderr}");
        assert!(!stderr.contains("cannot read witness"), "{stderr}");
        assert!(!stderr.contains("RPC transport failed"), "{stderr}");
    }
    Ok(())
}
