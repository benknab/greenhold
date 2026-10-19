use super::*;
use bitcoin::{Amount, ScriptBuf, TxOut, Txid};
use serde_json::json;

#[test]
fn amounts_preserve_satoshis_in_decimal_and_scientific_notation() -> Result<()> {
    for (text, sats) in [
        ("0", 0),
        ("0.00000001", 1),
        ("0.000000010", 1),
        ("1e-8", 1),
        ("1.2300E+0", 123_000_000),
        ("2.1e7", Amount::MAX_MONEY.to_sat()),
        ("1.23e-6", 123),
        ("0.01000000", 1_000_000),
    ] {
        assert_eq!(amount(&serde_json::from_str(text)?)?.to_sat(), sats);
    }
    for text in [
        "-1",
        "-0.1",
        "0.000000001",
        "1e-9",
        "21000000.00000001",
        "1e100",
        "1e-100",
    ] {
        assert!(amount(&serde_json::from_str(text)?).is_err(), "{text}");
    }
    Ok(())
}

#[test]
fn unspent_output_requires_confirmed_complete_bounded_coin_data() -> Result<()> {
    let valid = json!({"bestblock":BlockHash::all_zeros(),"confirmations":2,"value":0.000_000_01,
        "coinbase":false,"scriptPubKey":{"hex":"51","extra":true},"extra":true});
    let output: UnspentOutput = serde_json::from_value(valid.clone())?;
    assert_eq!(output.creation_height(10)?, 9);
    assert!(output.creation_height(0).is_err());
    assert_eq!(
        output
            .into_coin(
                OutPoint::new(Txid::from_byte_array([1; 32]), 0),
                BlockHash::all_zeros(),
                9
            )?
            .output()
            .value
            .to_sat(),
        1
    );
    for field in [
        "bestblock",
        "confirmations",
        "value",
        "coinbase",
        "scriptPubKey",
    ] {
        let mut value = valid.clone();
        value
            .as_object_mut()
            .context("output object")?
            .remove(field);
        assert!(serde_json::from_value::<UnspentOutput>(value).is_err());
    }
    for (field, replacement) in [
        ("confirmations", json!(0)),
        ("confirmations", json!(-1)),
        ("value", json!("1")),
        ("scriptPubKey", json!({"hex":"zz"})),
        ("scriptPubKey", json!({"hex":"51".repeat(10001)})),
    ] {
        let mut value = valid.clone();
        value[field] = replacement;
        assert!(serde_json::from_value::<UnspentOutput>(value).is_err());
    }
    Ok(())
}

#[test]
fn proof_import_reverses_display_hashes_and_checks_the_reconstructed_leaf() -> Result<()> {
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 0),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(1000),
            script_pubkey: ScriptBuf::new(),
        },
    )?;
    let leaf = bitcoin::hashes::sha256d::Hash::from_byte_array(coin.leaf_hash());
    let response = json!({"provedathash": BlockHash::all_zeros(), "prooftargets":[0],
        "hashesproven":[leaf], "proofhashes":[leaf], "extra":true});
    let proof: InclusionProof = serde_json::from_value(response.clone())?;
    assert_eq!(proof.hashes, vec![coin.leaf_hash()]);
    proof.bind(coin.clone())?;
    for (field, value) in [
        ("prooftargets", json!([])),
        ("prooftargets", json!([0, 1])),
        ("prooftargets", json!([u64::MAX])),
        ("hashesproven", json!([])),
        ("hashesproven", json!([leaf, leaf])),
        ("proofhashes", json!(["zz"])),
        ("proofhashes", json!(vec![leaf; 62])),
    ] {
        let mut invalid = response.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<InclusionProof>(invalid).is_err());
    }
    let mut wrong = response;
    wrong["hashesproven"] = json!([BlockHash::all_zeros()]);
    let proof: InclusionProof = serde_json::from_value(wrong)?;
    assert!(
        proof
            .bind(coin)
            .err()
            .context("wrong leaf accepted")?
            .to_string()
            .contains("reconstructed coin")
    );
    Ok(())
}
