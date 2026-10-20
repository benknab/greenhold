use super::*;
use bitcoin::Network;

const OUTPOINT: &str = "1111111111111111111111111111111111111111111111111111111111111111:0";

/// Parse prove arguments after `host prove <OUTPOINT> --dry-run`.
fn cli(args: &[&str]) -> clap::error::Result<Cli> {
    let args = ["host", "prove", OUTPOINT, "--dry-run"]
        .into_iter()
        .chain(args.iter().copied());
    Cli::try_parse_from(args)
}

fn parse(args: &[&str]) -> Result<ContextOptions> {
    let Command::Prove(options) = cli(args)?.command else {
        bail!("expected prove command");
    };
    Ok(options.context)
}

#[test]
fn prove_context_defaults_and_explicit_connections() -> Result<()> {
    let options = parse(&[])?;
    assert_eq!(options.last_height, None);
    assert_eq!(options.first_height, None);
    assert_eq!(Network::from(options.network), Network::Bitcoin);
    assert!(options.connections.utreexod_rpc.is_none());
    options.connections.rpc_config()?;

    let options = parse(&[
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

    let options = parse(&["--first-height", "100", "--network", "signet"])?;
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
        let mut args = vec![];
        for (bit, flag) in flags.iter().enumerate() {
            if included & (1 << bit) != 0 {
                args.extend(flag);
            }
        }
        let error = cli(&args).err().context("expected missing arguments")?;
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
        assert!(cli(&[flag, value]).is_err());
    }
    for (user, password) in [("", "pass"), ("user", ""), ("user:name", "pass")] {
        let error = cli(&[
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
fn prove_minimum_parses_into_the_domain_range() -> Result<()> {
    let parse_prove = |extra: &[&str]| -> Result<ProveOptions> {
        let args = ["host", "prove", OUTPOINT]
            .into_iter()
            .chain(extra.iter().copied());
        let Command::Prove(options) = Cli::try_parse_from(args)?.command else {
            bail!("expected prove command");
        };
        Ok(options)
    };
    assert_eq!(u64::from(parse_prove(&["--dry-run"])?.minimum_sats), 1);
    let max = "2100000000000000";
    assert_eq!(
        u64::from(parse_prove(&["--dry-run", "--minimum-sats", max])?.minimum_sats),
        2_100_000_000_000_000
    );
    for value in ["0", "2100000000000001", "-1", "1.5", "one"] {
        let flag = format!("--minimum-sats={value}");
        let error = Cli::try_parse_from(["host", "prove", OUTPOINT, "--dry-run", &flag])
            .err()
            .context("invalid minimum accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
    Ok(())
}

#[test]
fn prove_needs_exactly_one_of_output_and_dry_run() -> Result<()> {
    let parse = |extra: &[&str]| {
        let args = ["host", "prove", OUTPOINT]
            .into_iter()
            .chain(extra.iter().copied());
        Cli::try_parse_from(args)
    };
    let Command::Prove(options) = parse(&["--output", "claim.receipt"])?.command else {
        bail!("expected prove command");
    };
    assert_eq!(options.output, Some(PathBuf::from("claim.receipt")));
    let Command::Prove(options) = parse(&["--dry-run"])?.command else {
        bail!("expected prove command");
    };
    assert_eq!(options.output, None);
    for (extra, kind) in [
        (&[][..], clap::error::ErrorKind::MissingRequiredArgument),
        (
            &["--dry-run", "--output", "claim.receipt"][..],
            clap::error::ErrorKind::ArgumentConflict,
        ),
    ] {
        let error = parse(extra).err().context("invalid flags accepted")?;
        assert_eq!(error.kind(), kind);
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

#[test]
fn discover_selects_one_backend_and_validates_electrum_urls() -> Result<()> {
    let parse = |extra: &[&str]| {
        Cli::try_parse_from(
            ["host", "discover", "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"]
                .into_iter()
                .chain(extra.iter().copied()),
        )
    };
    for (args, expected) in [
        (vec![], None),
        (vec!["--electrum"], Some(DEFAULT_ELECTRUM)),
        (
            vec!["--electrum", "--network", "bitcoin"],
            Some(DEFAULT_ELECTRUM),
        ),
        (
            vec!["--electrum", "tcp://localhost:50001"],
            Some("tcp://localhost:50001"),
        ),
        (
            vec!["--electrum=ssl://localhost:50002"],
            Some("ssl://localhost:50002"),
        ),
    ] {
        let Command::Discover(options) = parse(&args)?.command else {
            bail!("expected discover command");
        };
        assert_eq!(
            options
                .electrum
                .as_ref()
                .map(|endpoint| endpoint.socket_url().url().as_str()),
            expected
        );
        if expected.is_none() {
            options.connections.rpc_config()?;
        }
    }
    for url in [
        "https://host:50002",
        "tls://host",
        "tcp://user:pass@host:50001",
    ] {
        let error = parse(&["--electrum", url])
            .err()
            .context("invalid URL accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
    for flag in [
        "--utreexod-rpc",
        "--utreexod-rpc-user",
        "--utreexod-rpc-password",
    ] {
        let value = if flag == "--utreexod-rpc" {
            "http://localhost/"
        } else {
            "test"
        };
        let error = parse(&["--electrum", flag, value])
            .err()
            .context("mixed backends accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    Ok(())
}
