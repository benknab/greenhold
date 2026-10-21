use super::*;
use bitcoin::Network;

const OUTPOINT: &str = "1111111111111111111111111111111111111111111111111111111111111111:0";

/// Parsing checks only the signature encoding, not the signature.
const SIGNED: [&str; 4] = [
    "--witness",
    "coin.witness",
    "--signature",
    "smpAkgwRQIhAKhSHOe4EaSt0a4mn+BQdQGuirn2vIn/a2Z4R6YplK0AAiBG+iFd4tbWCVyT0q8daaBXVfss7lHhpOVsr+KWLgF1CgEhAzDVT9DdQgpuX402JPXzSCyuNQ951fB1O/W+75wtka88",
];

/// Parse prove arguments after its claim and witness/signature flags.
fn prove(args: &[&str]) -> clap::error::Result<Cli> {
    let args = ["host", "prove", "--claim", "BASE64"]
        .into_iter()
        .chain(SIGNED)
        .chain(args.iter().copied());
    Cli::try_parse_from(args)
}

/// Parse claim arguments after its required witness path.
fn cli(args: &[&str]) -> clap::error::Result<Cli> {
    Cli::try_parse_from(
        [
            "host",
            "claim",
            "--outpoint",
            OUTPOINT,
            "--witness",
            "coin.witness",
        ]
        .into_iter()
        .chain(args.iter().copied()),
    )
}

fn parse(args: &[&str]) -> Result<ContextOptions> {
    let Command::Claim(options) = cli(args)?.command else {
        bail!("expected claim command");
    };
    Ok(options.context)
}

#[test]
fn claim_context_defaults_and_explicit_connections() -> Result<()> {
    let options = parse(&[])?;
    assert_eq!(options.last_height, None);
    assert_eq!(options.first_height, None);
    assert_eq!(Network::from(options.network), Network::Bitcoin);
    assert!(options.connections.utreexod_rpc.is_none());
    options.connections.rpc_config()?;

    let options = parse(&[
        "--last-height",
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
        ("--last-height", "4294967296"),
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
fn claim_minimum_parses_into_the_domain_range() -> Result<()> {
    let parse_claim = |extra: &[&str]| -> Result<ClaimOptions> {
        let Command::Claim(options) = cli(extra)?.command else {
            bail!("expected claim command");
        };
        Ok(options)
    };
    assert_eq!(u64::from(parse_claim(&[])?.minimum_sats), 1);
    let max = "2100000000000000";
    assert_eq!(
        u64::from(parse_claim(&["--minimum-sats", max])?.minimum_sats),
        2_100_000_000_000_000
    );
    for value in ["0", "2100000000000001", "-1", "1.5", "one"] {
        let flag = format!("--minimum-sats={value}");
        let error = cli(&[&flag]).err().context("invalid minimum accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
    Ok(())
}

#[test]
fn prove_needs_exactly_one_of_output_and_dry_run() -> Result<()> {
    let Command::Prove(options) = prove(&["--output", "claim.receipt"])?.command else {
        bail!("expected prove command");
    };
    assert_eq!(options.output, Some(PathBuf::from("claim.receipt")));
    let Command::Prove(options) = prove(&["--dry-run"])?.command else {
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
        let error = prove(extra).err().context("invalid flags accepted")?;
        assert_eq!(error.kind(), kind);
    }
    Ok(())
}

#[test]
fn prove_needs_a_witness_and_well_formed_signature() -> Result<()> {
    // Replace or drop one flag of SIGNED, then parse a dry run.
    let parse = |flag: &str, value: Option<&str>| -> Result<clap::error::ErrorKind> {
        let mut args = vec!["host", "prove", "--claim", "BASE64", "--dry-run"];
        for pair in SIGNED.chunks(2) {
            match (pair, value) {
                ([name, _], Some(value)) if *name == flag => args.extend([flag, value]),
                ([name, _], None) if *name == flag => {}
                _ => args.extend(pair),
            }
        }
        let error = Cli::try_parse_from(args).err().context("flags accepted")?;
        Ok(error.kind())
    };
    for flag in ["--witness", "--signature"] {
        assert_eq!(
            parse(flag, None)?,
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
    for (flag, value) in [("--signature", "smp!"), ("--signature", "pofAAAA")] {
        assert_eq!(
            parse(flag, Some(value))?,
            clap::error::ErrorKind::ValueValidation
        );
    }
    Ok(())
}

#[test]
fn verify_requires_an_explicit_expected_claim() -> Result<()> {
    let error = Cli::try_parse_from(["host", "verify", "--receipt", "claim.receipt"])
        .err()
        .context("missing claim accepted")?;
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    let Command::Verify(options) = Cli::try_parse_from([
        "host",
        "verify",
        "--receipt",
        "claim.receipt",
        "--claim",
        "BASE64",
    ])?
    .command
    else {
        bail!("expected verify command");
    };
    assert_eq!(options.claim, "BASE64");
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

fn discover_cli(extra: &[&str]) -> clap::error::Result<Cli> {
    Cli::try_parse_from(
        [
            "host",
            "discover",
            "--address",
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
        ]
        .into_iter()
        .chain(extra.iter().copied()),
    )
}

#[test]
fn discover_defaults_to_electrum_and_selects_explicit_connections() -> Result<()> {
    for (args, expected) in [
        (vec![], Some(DEFAULT_ELECTRUM)),
        (vec!["--utreexod"], None),
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
        let Command::Discover(options) = discover_cli(&args)?.command else {
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
    for select in [vec![], vec!["--utreexod"]] {
        let args: Vec<_> = select
            .into_iter()
            .chain([
                "--utreexod-rpc",
                "http://localhost:8334/",
                "--utreexod-rpc-user",
                "user",
                "--utreexod-rpc-password",
                "secret-password",
            ])
            .collect();
        let Command::Discover(options) = discover_cli(&args)?.command else {
            bail!("expected discover command");
        };
        assert!(options.electrum.is_none());
        assert_eq!(
            options.connections.utreexod_rpc.as_ref().map(Url::as_str),
            Some("http://localhost:8334/")
        );
        options.connections.rpc_config()?;
        assert!(!format!("{options:?}").contains("secret-password"));
    }
    Ok(())
}

#[test]
fn discover_rejects_invalid_urls_and_mixed_backends() -> Result<()> {
    for url in [
        "https://host:50002",
        "tls://host",
        "tcp://user:pass@host:50001",
    ] {
        let error = discover_cli(&["--electrum", url])
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
        let error = discover_cli(&["--electrum", flag, value])
            .err()
            .context("mixed backends accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    for args in [
        vec!["--electrum", "--utreexod"],
        vec!["--utreexod", "--electrum", "tcp://localhost:50001"],
        vec![
            "--electrum",
            "--utreexod-rpc",
            "http://localhost/",
            "--utreexod-rpc-user",
            "user",
            "--utreexod-rpc-password",
            "pass",
        ],
    ] {
        let error = discover_cli(&args)
            .err()
            .context("mixed backends accepted")?;
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    Ok(())
}
