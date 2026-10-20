use super::*;
use bitcoin::Network;

const OUTPOINT: &str = "1111111111111111111111111111111111111111111111111111111111111111:0";

/// Parsing checks only the signature encoding, not the signature.
const SIGNED: [&str; 6] = [
    "--address",
    "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu",
    "--message",
    "message",
    "--signature",
    "smpAkgwRQIhAKhSHOe4EaSt0a4mn+BQdQGuirn2vIn/a2Z4R6YplK0AAiBG+iFd4tbWCVyT0q8daaBXVfss7lHhpOVsr+KWLgF1CgEhAzDVT9DdQgpuX402JPXzSCyuNQ951fB1O/W+75wtka88",
];

/// Parse prove arguments after `host prove <OUTPOINT>` and the signature flags.
fn prove(args: &[&str]) -> clap::error::Result<Cli> {
    let args = ["host", "prove", OUTPOINT]
        .into_iter()
        .chain(SIGNED)
        .chain(args.iter().copied());
    Cli::try_parse_from(args)
}

/// Parse prove arguments after `host prove <OUTPOINT> --dry-run` and the signature flags.
fn cli(args: &[&str]) -> clap::error::Result<Cli> {
    prove(&[&["--dry-run"], args].concat())
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
        let Command::Prove(options) = prove(extra)?.command else {
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
fn prove_needs_an_address_message_and_well_formed_signature() -> Result<()> {
    // Replace or drop one flag of SIGNED, then parse a dry run.
    let parse = |flag: &str, value: Option<&str>| -> Result<clap::error::ErrorKind> {
        let mut args = vec!["host", "prove", OUTPOINT, "--dry-run"];
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
    for flag in ["--address", "--message", "--signature"] {
        assert_eq!(
            parse(flag, None)?,
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
    for (flag, value) in [
        ("--signature", "smp!"),
        ("--signature", "pofAAAA"),
        ("--address", "not-an-address"),
    ] {
        assert_eq!(
            parse(flag, Some(value))?,
            clap::error::ErrorKind::ValueValidation
        );
    }
    Ok(())
}

#[test]
fn verify_requires_an_explicit_message_including_empty_text() -> Result<()> {
    let error = Cli::try_parse_from(["host", "verify", "claim.receipt"])
        .err()
        .context("missing message accepted")?;
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    for message in ["", "a message\nwith whitespace", "árvíz 🦀"] {
        let Command::Verify(options) =
            Cli::try_parse_from(["host", "verify", "claim.receipt", "--message", message])?.command
        else {
            bail!("expected verify command");
        };
        assert_eq!(options.message, message);
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
