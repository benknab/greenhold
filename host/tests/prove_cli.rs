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
                "<OUTPOINT> [LAST_HEIGHT]",
                "tip-only",
                "--minimum-sats <SATS>",
                "--output <FILE>",
                "--witness <FILE>",
            ],
        ),
        (
            "prove",
            vec![
                "<CLAIM>",
                "--witness <FILE>",
                "--signature <SIGNATURE>",
                "--dry-run",
            ],
        ),
        ("verify", vec!["--claim <FILE>"]),
    ] {
        let output = Command::new(binary).args([command, "--help"]).output()?;
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout)?;
        for text in required {
            assert!(help.contains(text), "{help}");
        }
    }
    Ok(())
}

#[test]
fn prove_checks_the_claim_signature_before_node_access() -> Result<()> {
    let (input, claim) = fixture::signed(ScriptType::P2wpkh, Format::Simple)?;
    let input = serde_json::to_value(input)?;
    let witness: CoinWitness = serde_json::from_value(input["witness"].clone())?;
    let directory = tempfile::tempdir()?;
    let claim_path = directory.path().join("claim.bin");
    let witness_path = directory.path().join("coin.witness");
    fs::write(&claim_path, claim.to_bytes())?;
    fs::write(&witness_path, artifacts::encode_witness(&witness)?)?;
    let signature = input["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("signature"))?;
    for (changed, signature, network, reason) in [
        (false, signature, "regtest", "RPC transport failed"),
        (true, signature, "regtest", "BIP-322 signature check failed"),
        (
            false,
            "smpAA==",
            "regtest",
            "BIP-322 signature check failed",
        ),
        (
            false,
            signature,
            "bitcoin",
            "claim names regtest, not bitcoin",
        ),
    ] {
        let mut bytes = claim.to_bytes();
        if changed {
            bytes[123] ^= 1;
        } // A different valid M.
        fs::write(&claim_path, bytes)?;
        let output = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
            .arg("prove")
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
        if changed {
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
    fs::write(&path, claim.to_bytes())?;
    assert_eq!(artifacts::read_claim(&path)?, claim);
    for bytes in [
        claim.to_bytes()[..162].to_vec(),
        [claim.to_bytes(), vec![0; 32]].concat(),
    ] {
        fs::write(&path, bytes)?;
        assert!(artifacts::read_claim(&path).is_err());
    }
    Ok(())
}
