use anyhow::Result;
use std::process::Command;

#[test]
fn discover_checks_address_and_network_before_node_access() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold-host");
    let help = Command::new(binary).args(["discover", "--help"]).output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    assert!(help.contains("<ADDRESS>"));
    assert!(help.contains("one per line"));
    assert!(help.contains("--electrum [<URL>]"));
    assert!(help.contains("tls://electrum.hospitablealpaca.com:50002"));
    for (address, code, message) in [
        ("not-an-address", 2, "invalid value"),
        (
            "bcrt1qusgerygumpd0ztn735s5pypq6wsv2zzhuc4yak",
            1,
            "address does not match the requested network",
        ),
    ] {
        for connection in [
            vec!["--electrum"],
            vec!["--electrum", "tcp://127.0.0.1:9"],
            vec![
                "--utreexod-rpc",
                "http://127.0.0.1:9/",
                "--utreexod-rpc-user",
                "user",
                "--utreexod-rpc-password",
                "pass",
            ],
        ] {
            let output = Command::new(binary)
                .args(["discover", address])
                .args(connection)
                .output()?;
            assert_eq!(output.status.code(), Some(code));
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8(output.stderr)?;
            assert!(stderr.contains(message), "{stderr}");
        }
    }
    Ok(())
}
