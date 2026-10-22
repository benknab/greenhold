use anyhow::{Context as _, Result};
use bitcoin::base64::Engine as _;
use bitcoin::base64::engine::general_purpose::STANDARD;
use bitcoin::hex::DisplayHex as _;
use bitcoin::{Witness, consensus};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn run(directory: &Path, args: &str) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_greenhold-signer"))
        .current_dir(directory)
        .args(args.split_ascii_whitespace())
        .output()?)
}

fn json(directory: &Path, args: &str) -> Result<Value> {
    let output = run(directory, args)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn preserves_cli_addresses_defaults_and_signature_encodings() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("mnemonic"), format!("{MNEMONIC}\n"))?;
    for (script_type, purpose, address, format) in [
        ("p2pkh", 44, "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA", "full"),
        (
            "p2sh-p2wpkh",
            49,
            "37VucYSaXLCAsxYyAPfbSi9eh4iEcbShgf",
            "full",
        ),
        (
            "p2wpkh",
            84,
            "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu",
            "simple",
        ),
        (
            "p2tr",
            86,
            "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr",
            "simple",
        ),
    ] {
        let output = json(
            dir.path(),
            &format!("sign --type {script_type} --message= --mnemonic-file mnemonic"),
        )?;
        assert_eq!(output["type"], script_type);
        assert_eq!(output["network"], "bitcoin");
        assert_eq!(output["path"], format!("m/{purpose}'/0'/0'/0/0"));
        assert_eq!(output["address"], address);
        assert_eq!(output["format"], format);
        let signature = output["signature"].as_str().context("signature")?;
        assert!(signature.starts_with(if format == "full" { "ful" } else { "smp" }));
        if script_type == "p2tr" {
            let witness: Witness = consensus::deserialize(&STANDARD.decode(&signature[3..])?)?;
            assert_eq!(
                witness.to_vec().iter().map(Vec::len).collect::<Vec<_>>(),
                [64]
            );
        }
    }
    fs::write(
        dir.path().join("seed"),
        "000102030405060708090a0b0c0d0e0f\n",
    )?;
    let output = json(
        dir.path(),
        "address --type p2wpkh --network regtest --seed-file seed",
    )?;
    assert_eq!(
        output["address"],
        "bcrt1q7f0pjwhc3jzzv0w4uurm589506glv2dggfan4s"
    );
    assert_eq!(output["path"], "m/84'/1'/0'/0/0");
    assert!(output.get("signature").is_none());
    for seed in [
        "00".repeat(15),
        "00".repeat(65),
        "xy".repeat(16),
        "0".repeat(33),
    ] {
        fs::write(dir.path().join("seed"), seed)?;
        let output = run(dir.path(), "address --type p2wpkh --seed-file seed")?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn supports_passphrase_files_custom_paths_and_derivation_options() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("mnemonic"), MNEMONIC)?;
    fs::write(dir.path().join("passphrase"), "TREZOR\r\n")?;
    let seed = greenhold_signer::seed_from_mnemonic(MNEMONIC, "TREZOR")?;
    fs::write(dir.path().join("seed"), seed.as_hex().to_string())?;
    let output = json(
        dir.path(),
        "address --type p2tr --network signet --mnemonic-file mnemonic
        --passphrase-file passphrase --account 2 --change 1 --index 7",
    )?;
    let explicit = json(
        dir.path(),
        "address --type p2tr --network signet --seed-file seed
        --path m/86'/1'/2'/1/7",
    )?;
    assert_eq!(output, explicit);
    let root = json(dir.path(), "address --type p2pkh --seed-file seed --path m")?;
    assert_eq!(root["path"], "m");
    Ok(())
}

#[test]
fn rejects_invalid_requests_before_reading_secret_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for args in [
        "address --type p2wpkh",
        "address --type p2wpkh --seed-file missing --mnemonic-file missing",
        "address --type p2wpkh --seed-file missing --passphrase-file missing",
        "address --type p2wpkh --seed-file missing --path m --index 0",
        "address --type p2wpkh --seed-file missing --index 2147483648",
        "address --type p2wpkh --seed-file missing --network testnet",
        "address --type p2wpkh --seed-file missing --message test",
        "sign --type p2wpkh --seed-file missing",
        "sign --type p2pkh --seed-file missing --message test --format simple",
    ] {
        let output = run(dir.path(), args)?;
        assert!(!output.status.success(), "{args}");
        assert!(output.stdout.is_empty(), "{args}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("cannot read secret file"),
            "{args}"
        );
    }
    Ok(())
}
