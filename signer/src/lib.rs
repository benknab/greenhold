//! Test-wallet key derivation and single-key BIP-322 signing.
//!
//! Signs directly from secret keys, independently of Greenhold and the zkVM.

use anyhow::{Result, ensure};
use bip322::LockParams;
use bitcoin::base64::Engine as _;
use bitcoin::base64::engine::general_purpose::STANDARD;
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::hashes::Hash;
use bitcoin::key::TapTweak;
use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::{Address, CompressedPublicKey, Network, PrivateKey, Transaction, Witness, consensus};
use clap::ValueEnum;
use serde::Serialize;

/// Single-key scripts supported by the test wallet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptType {
    P2pkh,
    P2shP2wpkh,
    P2wpkh,
    P2tr,
}

/// BIP-322 signature encodings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Simple,
    Full,
    Legacy,
}

impl ScriptType {
    /// Allowed formats, with the default first.
    #[must_use]
    pub const fn formats(self) -> &'static [Format] {
        match self {
            Self::P2pkh => &[Format::Full, Format::Legacy],
            Self::P2shP2wpkh => &[Format::Full],
            Self::P2wpkh | Self::P2tr => &[Format::Simple, Format::Full],
        }
    }

    /// BIP-44/49/84/86 derivation path for this script and network.
    ///
    /// # Errors
    /// Rejects account, change, or index values at or above 2^31.
    pub fn default_path(
        self,
        network: Network,
        account: u32,
        change: u32,
        index: u32,
    ) -> Result<DerivationPath> {
        let purpose = match self {
            Self::P2pkh => 44,
            Self::P2shP2wpkh => 49,
            Self::P2wpkh => 84,
            Self::P2tr => 86,
        };
        Ok(vec![
            ChildNumber::from_hardened_idx(purpose)?,
            ChildNumber::from_hardened_idx(u32::from(network != Network::Bitcoin))?,
            ChildNumber::from_hardened_idx(account)?,
            ChildNumber::from_normal_idx(change)?,
            ChildNumber::from_normal_idx(index)?,
        ]
        .into())
    }
}

/// Decode an English BIP-39 mnemonic, normalizing whitespace and Unicode.
///
/// # Errors
/// Rejects invalid words, word counts, and checksums.
pub fn seed_from_mnemonic(mnemonic: &str, passphrase: &str) -> Result<[u8; 64]> {
    Ok(bip39::Mnemonic::parse_in(bip39::Language::English, mnemonic)?.to_seed(passphrase))
}

/// Derive a compressed wallet key from a BIP-32 seed and path.
///
/// # Errors
/// Rejects invalid seed lengths or failed BIP-32 derivations.
pub fn derive_key(seed: &[u8], path: &DerivationPath) -> Result<SecretKey> {
    ensure!(
        (16..=64).contains(&seed.len()),
        "seed must be 16 to 64 bytes"
    );
    // Network affects extended-key serialization, not the derived secret key.
    Ok(Xpriv::new_master(Network::Bitcoin, seed)?
        .derive_priv(&Secp256k1::new(), path)?
        .private_key)
}

/// Derive a single-key address; Taproot uses the BIP-86 empty script tree.
#[must_use]
pub fn address(script_type: ScriptType, key: &SecretKey, network: Network) -> Address {
    let secp = Secp256k1::new();
    let public = CompressedPublicKey(key.public_key(&secp));
    match script_type {
        ScriptType::P2pkh => Address::p2pkh(public, network),
        ScriptType::P2shP2wpkh => Address::p2shwpkh(&public, network),
        ScriptType::P2wpkh => Address::p2wpkh(&public, network),
        ScriptType::P2tr => Address::p2tr(&secp, public.0.x_only_public_key().0, None, network),
    }
}

/// An address and its encoded message signature.
#[derive(Clone, Debug, Serialize)]
pub struct SignedMessage {
    pub address: Address,
    pub signature: String,
}

/// Sign exact UTF-8 message bytes without time locks.
///
/// # Errors
/// Rejects formats unsupported by the script type, or failed signing.
pub fn sign(
    script_type: ScriptType,
    format: Format,
    key: &SecretKey,
    network: Network,
    message: &str,
) -> Result<SignedMessage> {
    ensure!(
        script_type.formats().contains(&format),
        "format is not supported for this script type"
    );
    let address = address(script_type, key, network);
    let private = PrivateKey::new(*key, network);
    let signature = if format == Format::Legacy {
        STANDARD.encode(bip322::sign_legacy(&address, message, &private)?.serialize())
    } else {
        let transaction = if script_type == ScriptType::P2tr {
            sign_taproot(&address, key, message)?
        } else {
            bip322::sign_full(&address, message, &[private], None, LockParams::default())?
        };
        let (prefix, bytes) = if format == Format::Simple {
            ("smp", consensus::serialize(&transaction.input[0].witness))
        } else {
            ("ful", consensus::serialize(&transaction))
        };
        format!("{prefix}{}", STANDARD.encode(bytes))
    };
    Ok(SignedMessage { address, signature })
}

/// bip322 0.0.12 signs Taproot with ALL; preserve the signer's DEFAULT policy.
fn sign_taproot(address: &Address, key: &SecretKey, message: &str) -> Result<Transaction> {
    let spend = bip322::create_to_spend(address, message)?;
    let mut transaction =
        bip322::create_to_sign(&spend, None, LockParams::default())?.extract_tx()?;
    let sighash = SighashCache::new(&transaction).taproot_key_spend_signature_hash(
        0,
        &Prevouts::All(&spend.output),
        TapSighashType::Default,
    )?;
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, key)
        .tap_tweak(&secp, None)
        .to_keypair();
    let signature =
        secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &keypair);
    transaction.input[0].witness = Witness::from_slice(&[signature.serialize()]);
    Ok(transaction)
}
