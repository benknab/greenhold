use super::verify_signature;
use crate::test_support::signatures::*;
use anyhow::{Context as _, Result};
use bitcoin::{
    Address, Amount, ScriptBuf, Transaction, Witness,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    consensus::{deserialize, serialize},
    hashes::Hash,
    secp256k1::ecdsa,
};

fn script(address: &str) -> Result<ScriptBuf> {
    Ok(address
        .parse::<Address<_>>()?
        .assume_checked()
        .script_pubkey())
}

fn verify(address: &str, message: &str, signature: &str) -> Result<()> {
    verify_signature(&script(address)?, message, &signature.parse()?)
}

fn rejection(address: &str, message: &str, signature: &str, reason: &str) {
    let error = verify(address, message, signature);
    let text = format!(
        "{:#}",
        error.err().unwrap_or_else(|| anyhow::anyhow!("accepted"))
    );
    assert!(text.contains(reason), "{address} {signature}: {text}");
}

fn full(signature: &str) -> Result<Transaction> {
    let base64 = signature.strip_prefix("ful").context("full signature")?;
    Ok(deserialize(&STANDARD.decode(base64)?)?)
}

fn encode_full(transaction: &Transaction) -> String {
    format!("ful{}", STANDARD.encode(serialize(transaction)))
}

fn encode_simple(witness: &Witness) -> String {
    format!("smp{}", STANDARD.encode(serialize(witness)))
}

fn simple_witness(signature: &str) -> Result<Witness> {
    let base64 = signature.strip_prefix("smp").context("simple signature")?;
    Ok(deserialize(&STANDARD.decode(base64)?)?)
}

#[test]
fn accepts_bip322_vectors_and_each_allowed_format() -> Result<()> {
    let basic = "bc1q9vza2e8x573nczrlzms0wvx3gsqjx7vavgkx0l";
    for (address, message, signature) in [
        // BIP-322 basic and generated vectors.
        (
            basic,
            "",
            "smpAkcwRAIgM2gBAQqvZX15ZiysmKmQpDrG83avLIT492QBzLnQIxYCIBaTpOaD20qRlEylyxFSeEA2ba9YOixpX8z46TSDtS40ASECx/EgAxlkQpQ9hYjgGu6EBCPMVPwVIVJqO4XCsMvViHI=",
        ),
        (
            basic,
            "Hello World",
            "smpAkgwRQIhAOzyynlqt93lOKJr+wmmxIens//zPzl9tqIOua93wO6MAiBi5n5EyAcPScOjf1lAqIUIQtr3zKNeavYabHyR8eGhowEhAsfxIAMZZEKUPYWI4BruhAQjzFT8FSFSajuFwrDL1Yhy",
        ),
        (
            "bc1pss0zhytly75awhm6x2hhvd5lnzv3vssgrf9axfheq8ldyzn88ges79fler",
            "No prefix fallback",
            "AUCJYOwOjxYAvatTAGYaVlNXBVyFuc4MwNQkOuK2tl8xhfKDONd0NjfYyNSYcRqeCp8hsAnCEPHAVEkO9h6vbQ/R",
        ),
        (
            "bc1pcquvhrqv0q68t4m0hfq6tpn006qrskyc7yrqnp2uyrf2emg3wynsdjyk38",
            "PURVOQ544B6HUATVBJZN5EZJUU",
            "smpAUB6B2Rbupzua8LTQIF06516wzl+cwKy1be8RgoiW0riyXdKwe6GTz/5Hnb37m67pJwIKCh+D5jDueG6KpvYpmu8",
        ),
        // Signatures from `signer/`.
        (P2PKH, MESSAGE, P2PKH_FULL),
        (P2PKH, MESSAGE, P2PKH_LEGACY),
        (P2SH_P2WPKH, MESSAGE, P2SH_P2WPKH_FULL),
        (P2WPKH, MESSAGE, P2WPKH_SIMPLE),
        (P2WPKH, MESSAGE, &P2WPKH_SIMPLE[3..]),
        (P2WPKH, MESSAGE, P2WPKH_FULL),
        (P2TR, MESSAGE, P2TR_SIMPLE),
        (P2TR, MESSAGE, P2TR_FULL),
    ] {
        verify(address, message, signature).with_context(|| format!("{address} {signature}"))?;
    }
    Ok(())
}

#[test]
fn rejects_wrong_messages_signers_and_time_locks() {
    let segwit_simple = "smpAkgwRQIhALC6hdfxNy1n45d7UXSskRBdfZW0Al259E1kDMpipdYkAiAJPfZqb+WurZuf1apU5xeE6Igui9dvt5tihQLDvxlY1AEhAqbnruyo677ktQjio7XOchO3w51Dh9AbRVngha5jtNfT";
    let taproot_simple = "smpAUB6B2Rbupzua8LTQIF06516wzl+cwKy1be8RgoiW0riyXdKwe6GTz/5Hnb37m67pJwIKCh+D5jDueG6KpvYpmu8";
    let pubkey_hash_full = "fulAgAAAAGn3Z6t/gsHNyHdgZTOVro0Hej+qbd/ilU1ACalKoHX3gAAAABqRzBEAiB+8t/tm8Jm6zYv9JGZZVlAUjmqg7ZglIA39U+bim8EKQIgDv3E5cHOagN+xYgN3ZQjTYlAJp/WyslwJWuFP1TmM3IBIQJcPK2h9SY+Ki1oussvHnMdFAhJgsYBFPl+rNcMv9P1ROAHAAABAAAAAAAAAAABauAHAAA=";
    let nested_full = "fulAgAAAAABAe5xLNMlYQH4OGjJ3h4lqQaVp0Cic7mwxkvyWswqFMXeAAAAABcWABSy/hpDH/KLAi4x25Tmb2UaO1xtWeAHAAABAAAAAAAAAAABagJHMEQCIDEleqb0n1R5c21TGkWRXNFae98wbwI0QOyh/YmRuQX1AiAcv1MhyTzPOVgZ1VIwuu0tDxrVJUHK8lhOUOXpsZnGwwEhAsjeDEoWX8hvEC8A/692yGQsPh6JBO8Zf4aITEQsKAcJ4AcAAA==";
    let segwit_full = "fulAgAAAAABAUrfzHHOLAKmgCIFSTT3krp+cQxj1BDPBN4GBg3tRmFXAAAAAADgBwAAAQAAAAAAAAAAAWoCSDBFAiEAjYj85zyhQKa9DbMO0reDwdhkNwKJkF3q2qFcijXDgMUCIAaQ75s3fwqrCeYIUJugLvhxZFxQIVquGN90vIKCW3QLASEDMurnDzvc0zABUwVwCADfGXoDx/M3SQnYt7e3IHDoU3PgBwAA";
    let taproot_full = "fulAgAAAAABAROFPNY6Zt8hFK0YQq5Wb6wk/CnUYEPtQ0HTHDyzNROrAAAAAADgBwAAAQAAAAAAAAAAAWoBQNRdLOo5XZY0SBqAsLZNr/z3Bqrmo3OxVn7e4tD/OOD4H9U/L1unq5Nmdz+S1w7SHtt46bFwnd8xnRVan8BofFfgBwAA";
    let invalid = "not valid for this script and message";
    let key = "Public key does not match";
    for (address, message, signature, reason) in [
        // BIP-322 generated error vectors.
        (
            "bc1qqthe0hz8klx90e7stf6shclhsvqd5ly96pn53v",
            "EFGJ4AZYXDV7NDUDSUDB3NCDUC",
            segwit_simple,
            invalid,
        ),
        (
            "bc1qgg6lpr05az2l5kz402ddz5ez7fdu25kgmd40lf",
            "2V6TUTMSH4VQ3Z7WZWKYD7DFNH",
            segwit_simple,
            key,
        ),
        (
            "bc1pcquvhrqv0q68t4m0hfq6tpn006qrskyc7yrqnp2uyrf2emg3wynsdjyk38",
            "56VM6YK6Y76XTBXNPITF232EPX",
            taproot_simple,
            invalid,
        ),
        (
            "bc1pltvk000nd54v3hrrcn7lsffdra72hphpm40rhzf9hn8arqkgermq2p9029",
            "PURVOQ544B6HUATVBJZN5EZJUU",
            taproot_simple,
            invalid,
        ),
        (
            "13vU5PUSuArDXJdCWZvUFEbgJ2wcmtSJWn",
            "TCHG6CQ5E2T5S4S7DPLAEFVDY2",
            pubkey_hash_full,
            invalid,
        ),
        (
            "1BxMhvfWLnGLqVhJ3j39oDBk7qf5D86BFe",
            "MOISC5NCQ42ADH2SUXLELUJOWH",
            pubkey_hash_full,
            invalid,
        ),
        (
            "32Utb7Seg6EXq7UesMNJXhQ1gdohYNyzQ9",
            "CPVOBEXDTFAXS6N4YASD753CZV",
            nested_full,
            invalid,
        ),
        (
            "3QMEQj2LTUtKKR1UatUK44z1NwrWrcVSGh",
            "EMYGZHEY3LIANYKCR7XJF3NMFQ",
            nested_full,
            "push only the redeem script",
        ),
        // Valid BIP-322 generated signatures, valid only at time 2016 and age 2016.
        (
            "13vU5PUSuArDXJdCWZvUFEbgJ2wcmtSJWn",
            "MOISC5NCQ42ADH2SUXLELUJOWH",
            pubkey_hash_full,
            "time-locked",
        ),
        (
            "32Utb7Seg6EXq7UesMNJXhQ1gdohYNyzQ9",
            "EMYGZHEY3LIANYKCR7XJF3NMFQ",
            nested_full,
            "time-locked",
        ),
        (
            "bc1qrqtlzcq86850yzgsyq9sssawx2qxlx5yq3xpkd",
            "KLE5MMJBTNF4AVZXIO3GIL5UWF",
            segwit_full,
            "time-locked",
        ),
        (
            "bc1pve87s3l2levjmhetzr2f9xvep3y266xty0hnefmyv8tkxc3e4qssll2kdu",
            "XQMVC3YR6AOGZIHLSUQ2NSSBI2",
            taproot_full,
            "time-locked",
        ),
        // Signatures from `signer/`, checked against another message.
        (P2PKH, "other", P2PKH_LEGACY, invalid),
        (P2WPKH, "other", P2WPKH_SIMPLE, invalid),
        (P2TR, "other", P2TR_SIMPLE, invalid),
    ] {
        rejection(address, message, signature, reason);
    }
}

#[test]
fn rejects_formats_and_scripts_that_bip322_does_not_allow_here() {
    let simple_only = "only for P2WPKH and P2TR";
    let p2wsh = "bc1qw6g0rgrpuxvj4edkwtvzpmt3c5m08mhp8nuk3mrk4erufvlczp5ssdscjd";
    for (address, signature, reason) in [
        (P2PKH, P2WPKH_SIMPLE, simple_only),
        (P2SH_P2WPKH, P2WPKH_SIMPLE, simple_only),
        (P2WPKH, P2PKH_LEGACY, "legacy signatures only for P2PKH"),
        (
            P2SH_P2WPKH,
            P2PKH_LEGACY,
            "legacy signatures only for P2PKH",
        ),
        (p2wsh, P2WPKH_SIMPLE, "unsupported script"),
        (P2WPKH, "pofcHNidP8BAI8CAAAAA", "proof-of-funds"),
        (P2WPKH, "smp!!", "not base64"),
        (P2WPKH, "ful", "not a transaction"),
        (P2WPKH, "smpAA==", "Witness empty"),
        // Signatures signed with another script's key.
        (P2PKH, P2WPKH_FULL, "not valid for this script"),
        (P2SH_P2WPKH, P2PKH_FULL, "push only the redeem script"),
    ] {
        rejection(address, MESSAGE, signature, reason);
    }
    // A 65-byte BIP-137 SegWit header is not a legacy P2PKH signature.
    let mut segwit_header = STANDARD.decode(P2PKH_LEGACY).unwrap_or_default();
    segwit_header[0] += 8;
    rejection(
        P2PKH,
        MESSAGE,
        &STANDARD.encode(segwit_header),
        "P2PKH headers",
    );
    let mut trailing = STANDARD.decode(&P2WPKH_SIMPLE[3..]).unwrap_or_default();
    trailing.push(0);
    rejection(
        P2WPKH,
        MESSAGE,
        &format!("smp{}", STANDARD.encode(trailing)),
        "witness stack",
    );
}

#[test]
fn rejects_malformed_to_sign_transactions() -> Result<()> {
    let mut extra_output = full(P2WPKH_FULL)?;
    extra_output.output.push(extra_output.output[0].clone());
    let mut paying_output = full(P2WPKH_FULL)?;
    paying_output.output[0].value = Amount::from_sat(1);
    let mut no_input = full(P2WPKH_FULL)?;
    no_input.input.clear();
    let mut version_one = full(P2WPKH_FULL)?;
    version_one.version = bitcoin::transaction::Version::ONE;
    let mut nested_without_redeem = full(P2SH_P2WPKH_FULL)?;
    nested_without_redeem.input[0].script_sig = ScriptBuf::new();
    let mut native_with_script_sig = full(P2WPKH_FULL)?;
    native_with_script_sig.input[0].script_sig = ScriptBuf::from_bytes(vec![0x00]);
    let mut taproot_with_script_sig = full(P2TR_FULL)?;
    taproot_with_script_sig.input[0].script_sig = ScriptBuf::from_bytes(vec![0x00]);
    let mut p2pkh_with_witness = full(P2PKH_FULL)?;
    p2pkh_with_witness.input[0].witness = Witness::from_slice(&[[1u8]]);
    let mut p2pkh_extra_push = full(P2PKH_FULL)?;
    let mut script_sig = p2pkh_extra_push.input[0].script_sig.to_bytes();
    script_sig.push(0x00);
    p2pkh_extra_push.input[0].script_sig = ScriptBuf::from_bytes(script_sig);
    let mut p2pkh_opcode = full(P2PKH_FULL)?;
    let mut script_sig = p2pkh_opcode.input[0].script_sig.to_bytes();
    script_sig.push(0x51);
    p2pkh_opcode.input[0].script_sig = ScriptBuf::from_bytes(script_sig);
    for (address, transaction, reason) in [
        (P2WPKH, extra_output, "To sign transaction invalid"),
        (P2WPKH, paying_output, "To sign transaction invalid"),
        (P2WPKH, no_input, "exactly one input"),
        (P2WPKH, version_one, "inconclusive"),
        (
            P2SH_P2WPKH,
            nested_without_redeem,
            "push only the redeem script",
        ),
        (
            P2WPKH,
            native_with_script_sig,
            "To sign transaction invalid",
        ),
        (P2TR, taproot_with_script_sig, "To sign transaction invalid"),
        (P2PKH, p2pkh_with_witness, "Invalid witness"),
        (P2PKH, p2pkh_extra_push, "Invalid witness"),
        (P2PKH, p2pkh_opcode, "Invalid witness"),
    ] {
        rejection(address, MESSAGE, &encode_full(&transaction), reason);
    }
    Ok(())
}

#[test]
fn rejects_signature_encodings_outside_bip322_rules() -> Result<()> {
    let witness = simple_witness(P2WPKH_SIMPLE)?.to_vec();
    let [signature, key] = witness.as_slice() else {
        anyhow::bail!("P2WPKH witness");
    };
    let (sighash, der) = signature.split_last().context("signature")?;
    let mut sighash_none = der.to_vec();
    sighash_none.push(0x02);
    let high_s = high_s(der)?;
    let mut taproot = simple_witness(P2TR_SIMPLE)?.to_vec();
    let mut explicit_default = taproot[0].clone();
    explicit_default.push(0x00);
    let mut taproot_all = taproot[0].clone();
    taproot_all.push(0x01);
    let mut taproot_single = taproot[0].clone();
    taproot_single.push(0x03);
    taproot.push(vec![0x50]);
    for (address, items, reason) in [
        (
            P2WPKH,
            vec![sighash_none, key.clone()],
            "Unsupported sighash type",
        ),
        (
            P2WPKH,
            vec![[high_s, vec![*sighash]].concat(), key.clone()],
            "Invalid signature",
        ),
        (P2WPKH, vec![der.to_vec(), key.clone()], "Invalid signature"),
        (P2WPKH, vec![signature.clone()], "Invalid witness"),
        (
            P2TR,
            vec![explicit_default],
            "SIGHASH_DEFAULT must use a 64-byte signature",
        ),
        // SIGHASH_ALL changes the digest, so the default-type signature fails.
        (P2TR, vec![taproot_all], "not valid"),
        (P2TR, vec![taproot_single], "Unsupported sighash type"),
        (P2TR, taproot, "one key-path signature"),
    ] {
        rejection(
            address,
            MESSAGE,
            &encode_simple(&Witness::from_slice(&items)),
            reason,
        );
    }
    // The uncompressed encoding of the same key does not hash to the P2WPKH program.
    let uncompressed = bitcoin::PublicKey::from_slice(key)?
        .inner
        .serialize_uncompressed();
    rejection(
        P2WPKH,
        MESSAGE,
        &encode_simple(&Witness::from_slice(&[
            signature.clone(),
            uncompressed.to_vec(),
        ])),
        "compressed public key",
    );
    Ok(())
}

/// The same signature with S replaced by n - S.
fn high_s(der: &[u8]) -> Result<Vec<u8>> {
    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];
    let compact = ecdsa::Signature::from_der(der)?.serialize_compact();
    let mut high = compact;
    let mut borrow = 0u16;
    for index in (32..64).rev() {
        let difference = u16::from(ORDER[index - 32]) + 0x100 - u16::from(compact[index]) - borrow;
        high[index] = u8::try_from(difference & 0xff)?;
        borrow = u16::from(difference < 0x100);
    }
    Ok(ecdsa::Signature::from_compact(&high)?
        .serialize_der()
        .to_vec())
}

#[test]
fn rejects_a_lock_time_or_a_sequence_alone() -> Result<()> {
    // The BIP-322 basic vector key and its P2WPKH address.
    let key =
        bitcoin::PrivateKey::from_wif("L3VFeEujGtevx9w18HD1fhRbCH67Az2dpCymeRE1SoPK6XQtaN2k")?;
    let address = "bc1q9vza2e8x573nczrlzms0wvx3gsqjx7vavgkx0l";
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let public = bitcoin::CompressedPublicKey::from_private_key(&secp, &key)?;
    let script = script(address)?;
    for (lock_time, sequence) in [(1, 0), (0, 1)] {
        let address = address.parse::<Address<_>>()?.assume_checked();
        let spend = bip322::create_to_spend(&address, MESSAGE)?;
        let mut transaction =
            bip322::create_to_sign(&spend, None, bip322::LockParams::default())?.extract_tx()?;
        transaction.version = bitcoin::transaction::Version::TWO;
        transaction.lock_time = bitcoin::absolute::LockTime::from_consensus(lock_time);
        transaction.input[0].sequence = bitcoin::Sequence(sequence);
        let sighash = bitcoin::sighash::SighashCache::new(&transaction).p2wpkh_signature_hash(
            0,
            &script,
            Amount::ZERO,
            bitcoin::EcdsaSighashType::All,
        )?;
        let signature = secp.sign_ecdsa(
            &bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array()),
            &key.inner,
        );
        transaction.input[0].witness = Witness::p2wpkh(
            &bitcoin::ecdsa::Signature::sighash_all(signature),
            &public.0,
        );
        rejection(
            &address.to_string(),
            MESSAGE,
            &encode_full(&transaction),
            "time-locked",
        );
    }
    Ok(())
}
