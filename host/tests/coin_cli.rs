use anyhow::Result;
use std::process::Command;

#[test]
fn coin_cli_requires_an_outpoint_and_explains_tip_only_proofs() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let help = Command::new(binary).args(["coin", "--help"]).output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("<OUTPOINT> [LAST_HEIGHT]"));
    assert!(help.contains("tip-only"));
    for args in [vec!["coin"], vec!["coin", "not-an-outpoint"]] {
        let output = Command::new(binary).args(args).output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)?.contains("OUTPOINT"));
    }
    Ok(())
}
