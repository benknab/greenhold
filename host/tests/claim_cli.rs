use anyhow::Result;
use std::process::Command;

#[test]
fn claim_cli_requires_an_outpoint_and_explains_tip_only_proofs() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let help = Command::new(binary).args(["claim", "--help"]).output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("<OUTPOINT> [LAST_HEIGHT]"));
    assert!(help.contains("tip-only"));
    assert!(help.contains("--minimum-sats <SATS>"));
    for args in [vec!["claim"], vec!["claim", "not-an-outpoint"]] {
        let output = Command::new(binary).args(args).output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("OUTPOINT"));
    }
    Ok(())
}

#[test]
fn claim_cli_rejects_minimum_outside_money_range_before_node_access() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let outpoint = format!("{}:0", "11".repeat(32));
    for value in ["0", "2100000000000001"] {
        // An unused local port: reaching node access would fail with exit code 1.
        let output = Command::new(binary)
            .args(["claim", &outpoint, "--minimum-sats", value])
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
        assert!(stderr.contains("--minimum-sats"), "{stderr}");
        assert!(stderr.contains("minimum amount must be 1 to 2100000000000000 satoshis"));
    }
    Ok(())
}
