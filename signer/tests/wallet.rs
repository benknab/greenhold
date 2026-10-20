use anyhow::Result;
use bitcoin::{Network, hex::FromHex};
use greenhold_signer::{Format, ScriptType, derive_key, seed_from_mnemonic, sign};

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn bip39_seed_matches_published_vector_and_normalizes_unicode() -> Result<()> {
    // BIP-39 English vector: zero entropy, passphrase TREZOR.
    let expected = Vec::<u8>::from_hex(
        "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04",
    )?;
    assert_eq!(seed_from_mnemonic(MNEMONIC, "TREZOR")?.as_slice(), expected);
    assert_eq!(
        seed_from_mnemonic(&format!("\n{}\t", MNEMONIC.replace(' ', "  \n")), "é")?,
        seed_from_mnemonic(MNEMONIC, "e\u{301}")?,
    );
    assert_ne!(
        seed_from_mnemonic(MNEMONIC, "")?,
        seed_from_mnemonic(MNEMONIC, " ")?
    );
    Ok(())
}

#[test]
fn rejects_invalid_seeds_paths_and_signing_combinations() -> Result<()> {
    assert!(seed_from_mnemonic(&"abandon ".repeat(12), "").is_err());
    assert!(seed_from_mnemonic("not a mnemonic", "").is_err());
    let root = "m".parse()?;
    for length in [15, 65] {
        assert!(derive_key(&vec![0; length], &root).is_err());
    }
    for length in [16, 32, 64] {
        derive_key(&vec![0; length], &root)?;
    }
    let script_type = ScriptType::P2wpkh;
    for (account, change, index) in [(1 << 31, 0, 0), (0, 1 << 31, 0), (0, 0, 1 << 31)] {
        assert!(
            script_type
                .default_path(Network::Bitcoin, account, change, index)
                .is_err()
        );
    }
    let key = derive_key(&[0; 16], &root)?;
    for (script_type, format) in [
        (ScriptType::P2pkh, Format::Simple),
        (ScriptType::P2shP2wpkh, Format::Simple),
        (ScriptType::P2shP2wpkh, Format::Legacy),
        (ScriptType::P2wpkh, Format::Legacy),
        (ScriptType::P2tr, Format::Legacy),
    ] {
        assert!(sign(script_type, format, &key, Network::Bitcoin, "test").is_err());
    }
    Ok(())
}
