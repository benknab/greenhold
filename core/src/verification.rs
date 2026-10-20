//! Single-key script and format policy around the BIP-322 verifier.

use anyhow::{Context as _, Result, bail};
use bip322::Verification;
use bitcoin::{
    Address, Network, Script, Sequence, Transaction, Witness,
    absolute::LockTime,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    consensus::{deserialize, serialize},
    sign_message::MessageSignature,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// A decoded BIP-322 signature. Parsing checks only the encoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Signature(Encoding);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Encoding {
    Legacy(MessageSignature),
    Simple(Witness),
    Full(Transaction),
}

impl TryFrom<String> for Signature {
    type Error = anyhow::Error;

    fn try_from(text: String) -> Result<Self> {
        text.parse()
    }
}

impl From<Signature> for String {
    fn from(signature: Signature) -> Self {
        match signature.0 {
            Encoding::Legacy(signature) => STANDARD.encode(signature.serialize()),
            Encoding::Simple(witness) => format!("smp{}", STANDARD.encode(serialize(&witness))),
            Encoding::Full(transaction) => {
                format!("ful{}", STANDARD.encode(serialize(&transaction)))
            }
        }
    }
}

impl FromStr for Signature {
    type Err = anyhow::Error;

    /// Read prefixed simple/full signatures, unprefixed simple signatures, or
    /// legacy signatures with a P2PKH header (27 to 34).
    fn from_str(text: &str) -> Result<Self> {
        let decode = |text: &str| STANDARD.decode(text).context("signature is not base64");
        let encoding = if let Some(rest) = text.strip_prefix("smp") {
            Encoding::Simple(simple(&decode(rest)?)?)
        } else if let Some(rest) = text.strip_prefix("ful") {
            Encoding::Full(
                deserialize(&decode(rest)?).context("full signature is not a transaction")?,
            )
        } else if text.starts_with("pof") {
            bail!("BIP-322 proof-of-funds signatures are not supported");
        } else {
            let bytes = decode(text)?;
            match (bytes.len(), bytes.first()) {
                (65, Some(27..=34)) => Encoding::Legacy(
                    MessageSignature::from_slice(&bytes).context("legacy signature is invalid")?,
                ),
                (65, Some(35..=42)) => {
                    bail!("BIP-322 allows legacy signatures only with P2PKH headers (27 to 34)")
                }
                _ => Encoding::Simple(simple(&bytes)?),
            }
        };
        Ok(Self(encoding))
    }
}

fn simple(bytes: &[u8]) -> Result<Witness> {
    deserialize(bytes).context("simple signature is not a witness stack")
}

/// Verify a message signature for P2PKH, P2SH-P2WPKH, P2WPKH, or P2TR key-path scripts.
/// Simple signatures require native witness scripts; legacy signatures require P2PKH.
///
/// # Errors
/// Rejects unsupported scripts or formats, invalid signatures, time locks, and
/// inconclusive verification results.
pub fn verify_signature(
    script_pubkey: &Script,
    message: &str,
    signature: &Signature,
) -> Result<()> {
    if !(script_pubkey.is_p2pkh()
        || script_pubkey.is_p2sh()
        || script_pubkey.is_p2wpkh()
        || script_pubkey.is_p2tr())
    {
        bail!("unsupported script; expected P2PKH, P2SH-P2WPKH, P2WPKH, or P2TR");
    }
    // The library takes an address. Its network only selects the address encoding;
    // BIP-322 signs the script, and the host checks the supplied address's network.
    let address = Address::from_script(script_pubkey, Network::Bitcoin)?;
    let result = match &signature.0 {
        Encoding::Legacy(signature) => {
            if !script_pubkey.is_p2pkh() {
                bail!("BIP-322 allows legacy signatures only for P2PKH scripts");
            }
            return bip322::verify_legacy(&address, message, *signature)
                .context("signature is not valid for this script and message");
        }
        Encoding::Simple(witness) => {
            if !script_pubkey.is_p2wpkh() && !script_pubkey.is_p2tr() {
                bail!("BIP-322 allows simple signatures only for P2WPKH and P2TR scripts");
            }
            check_stack(script_pubkey, witness, Script::new())?;
            bip322::verify_simple(&address, message, witness.clone())
        }
        Encoding::Full(transaction) => {
            let [input] = transaction.input.as_slice() else {
                bail!("to_sign must have exactly one input");
            };
            check_stack(script_pubkey, &input.witness, &input.script_sig)?;
            bip322::verify_full(&address, message, transaction.clone())
        }
    }
    .context("signature is not valid for this script and message")?;
    match result {
        Verification::Valid {
            time: LockTime::ZERO,
            age: Sequence::ZERO,
        } => Ok(()),
        Verification::Valid { .. } => bail!("time-locked BIP-322 signatures are not supported"),
        Verification::Inconclusive => bail!("BIP-322 verification is inconclusive"),
    }
}

/// Restrict the library's broader script support to our single-key policy.
fn check_stack(script: &Script, witness: &Witness, script_sig: &Script) -> Result<()> {
    if script.is_p2sh() {
        // Exactly one minimal push of a P2WPKH redeem script. The library also
        // accepts an empty nested scriptSig, which a full signature must not use.
        let bytes = script_sig.as_bytes();
        if bytes.len() != 23
            || bytes[0] != 22
            || !Script::from_bytes(&bytes[1..]).is_p2wpkh()
            || Script::from_bytes(&bytes[1..]).to_p2sh().as_script() != script
            || witness.len() != 2
        {
            bail!(
                "P2SH-P2WPKH signature must push only the redeem script and have two witness items"
            );
        }
    } else if script.is_p2tr() {
        let mut items = witness.iter();
        let signature = items
            .next()
            .context("P2TR signature must be one key-path signature")?;
        if items.next().is_some() {
            bail!("P2TR signature must be one key-path signature");
        }
        // bip322 0.0.12 accepts explicit SIGHASH_DEFAULT, but BIP-341 requires
        // the 64-byte form for DEFAULT. Leave other encoding checks to the library.
        if signature.len() == 65 && signature[64] == 0 {
            bail!("SIGHASH_DEFAULT must use a 64-byte signature");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
