use super::*;
use bitcoin::Network;

fn parse(args: &[&str]) -> Result<ContextOptions> {
    let Command::Context(options) = Cli::try_parse_from(args)?.command;
    Ok(options)
}

#[test]
fn context_defaults_and_explicit_connections() -> Result<()> {
    let options = parse(&["host", "context"])?;
    assert_eq!(options.last_height, None);
    assert_eq!(options.first_height, None);
    assert_eq!(Network::from(options.network), Network::Bitcoin);
    assert!(options.connections.utreexod_rpc.is_none());
    options.connections.rpc_config()?;

    let options = parse(&[
        "host",
        "context",
        "200",
        "--first-height",
        "100",
        "--network",
        "regtest",
        "--utreexod-rpc",
        "http://localhost:8334/",
        "--utreexod-rpc-user",
        "user",
        "--utreexod-rpc-password",
        "secret-password",
    ])?;
    assert_eq!(options.last_height, Some(200));
    assert_eq!(options.first_height, Some(100));
    assert_eq!(Network::from(options.network), Network::Regtest);
    assert_eq!(
        options.connections.utreexod_rpc.as_ref().map(Url::as_str),
        Some("http://localhost:8334/")
    );
    options.connections.rpc_config()?;
    assert!(!format!("{options:?}").contains("secret-password"));

    let options = parse(&[
        "host",
        "context",
        "--first-height",
        "100",
        "--network",
        "signet",
    ])?;
    assert_eq!(options.last_height, None);
    assert_eq!(options.first_height, Some(100));
    assert_eq!(Network::from(options.network), Network::Signet);
    Ok(())
}

#[test]
fn incomplete_auth_and_invalid_values_fail_during_parsing() -> Result<()> {
    let flags = [
        ["--utreexod-rpc", "http://localhost/"],
        ["--utreexod-rpc-user", "user"],
        ["--utreexod-rpc-password", "pass"],
    ];
    for included in [1_u8, 2, 4, 3, 5, 6] {
        let mut args = vec!["host", "context"];
        for (bit, flag) in flags.iter().enumerate() {
            if included & (1 << bit) != 0 {
                args.extend(flag);
            }
        }
        let error = Cli::try_parse_from(args)
            .err()
            .context("expected missing arguments")?;
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
    for (flag, value) in [
        ("--network", "testnet"),
        ("--network", "testnet4"),
        ("--first-height", "4294967296"),
    ] {
        assert!(Cli::try_parse_from(["host", "context", flag, value]).is_err());
    }
    for (user, password) in [("", "pass"), ("user", ""), ("user:name", "pass")] {
        let error = Cli::try_parse_from([
            "host",
            "context",
            "--utreexod-rpc",
            "http://localhost/",
            "--utreexod-rpc-user",
            user,
            "--utreexod-rpc-password",
            password,
        ])
        .err()
        .context("expected invalid credentials")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
    Ok(())
}

#[test]
fn rpc_url_excludes_embedded_secrets_and_non_http_transports() {
    for text in [
        "ftp://host/",
        "http://user:password@host/",
        "http://host/?secret",
        "http://host/#fragment",
    ] {
        assert!(rpc_url(text).is_err(), "{text}");
    }
}
