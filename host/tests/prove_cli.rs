use anyhow::Result;
use std::process::{Command, Output};

/// A BIP-322 simple signature from `signer/` for this address and message.
const ADDRESS: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";
const MESSAGE: &str = "Greenhold BIP-322 test";
const SIGNATURE: &str = "smpAkgwRQIhAKhSHOe4EaSt0a4mn+BQdQGuirn2vIn/a2Z4R6YplK0AAiBG+iFd4tbWCVyT0q8daaBXVfss7lHhpOVsr+KWLgF1CgEhAzDVT9DdQgpuX402JPXzSCyuNQ951fB1O/W+75wtka88";

#[test]
fn prove_cli_requires_an_outpoint_and_explains_tip_only_proofs() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let help = Command::new(binary).args(["prove", "--help"]).output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("<OUTPOINT> [LAST_HEIGHT]"));
    assert!(help.contains("tip-only"));
    assert!(help.contains("--address <ADDRESS>"));
    assert!(help.contains("--message <TEXT>"));
    assert!(help.contains("--signature <SIGNATURE>"));
    assert!(help.contains("--minimum-sats <SATS>"));
    assert!(help.contains("--output <FILE>"));
    assert!(help.contains("--dry-run"));
    for args in [
        vec!["prove", "--dry-run"],
        vec!["prove", "not-an-outpoint", "--dry-run"],
    ] {
        let output = Command::new(binary).args(args).output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("OUTPOINT"));
    }
    Ok(())
}

/// Run `prove` against an unused local port: reaching node access fails with
/// "RPC transport failed".
fn prove(address: &str, message: &str, signature: &str, args: &[&str]) -> Result<Output> {
    let outpoint = format!("{}:0", "11".repeat(32));
    Ok(Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args([
            "prove",
            &outpoint,
            "--address",
            address,
            "--message",
            message,
        ])
        .args(["--signature", signature])
        .args(args)
        .args(["--utreexod-rpc", "http://127.0.0.1:9/"])
        .args([
            "--utreexod-rpc-user",
            "user",
            "--utreexod-rpc-password",
            "pass",
        ])
        .output()?)
}

#[test]
fn prove_cli_rejects_invalid_arguments_before_node_access() -> Result<()> {
    for (args, message) in [
        (
            vec!["--dry-run", "--minimum-sats", "0"],
            "minimum amount must be 1 to 2100000000000000 satoshis",
        ),
        (
            vec!["--dry-run", "--minimum-sats", "2100000000000001"],
            "minimum amount must be 1 to 2100000000000000 satoshis",
        ),
        (vec![], "--output <FILE>"),
    ] {
        let output = prove(ADDRESS, MESSAGE, SIGNATURE, &args)?;
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(message), "{stderr}");
    }
    let output = prove(ADDRESS, MESSAGE, "smp!", &["--dry-run"])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("signature is not base64"));
    Ok(())
}

#[test]
fn prove_cli_checks_the_signature_before_node_access() -> Result<()> {
    let p2pkh = "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA";
    for (address, message, args, reason) in [
        (
            ADDRESS,
            "another message",
            vec![],
            "not valid for this script and message",
        ),
        (
            p2pkh,
            MESSAGE,
            vec![],
            "simple signatures only for P2WPKH and P2TR",
        ),
        (
            ADDRESS,
            MESSAGE,
            vec!["--network", "regtest"],
            "address does not match the requested network",
        ),
    ] {
        let output = prove(
            address,
            message,
            SIGNATURE,
            &[&args[..], &["--dry-run"]].concat(),
        )?;
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(reason), "{stderr}");
        assert!(!stderr.contains("RPC transport failed"), "{stderr}");
    }
    // A valid signature passes the check, so the run reaches node access.
    let output = prove(ADDRESS, MESSAGE, SIGNATURE, &["--dry-run"])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)?.contains("RPC transport failed"));
    Ok(())
}
