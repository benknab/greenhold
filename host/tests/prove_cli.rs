use anyhow::Result;
use std::process::Command;

#[test]
fn prove_cli_requires_an_outpoint_and_explains_tip_only_proofs() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let help = Command::new(binary).args(["prove", "--help"]).output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("<OUTPOINT> [LAST_HEIGHT]"));
    assert!(help.contains("tip-only"));
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

#[test]
fn prove_cli_rejects_invalid_arguments_before_node_access() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let outpoint = format!("{}:0", "11".repeat(32));
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
        // An unused local port: reaching node access would fail with exit code 1.
        let output = Command::new(binary)
            .args(["prove", &outpoint])
            .args(&args)
            .args(["--utreexod-rpc", "http://127.0.0.1:9/"])
            .args([
                "--utreexod-rpc-user",
                "user",
                "--utreexod-rpc-password",
                "pass",
            ])
            .output()?;
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(message), "{stderr}");
    }
    Ok(())
}
