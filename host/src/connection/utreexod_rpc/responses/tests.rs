use super::*;
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
fn tx_out_response_preserves_mempool_outputs_and_requires_complete_data() -> Result<()> {
    let valid = json!({"bestblock":BlockHash::all_zeros(),"confirmations":2,"value":0.000_000_01,
        "coinbase":false,"scriptPubKey":{"hex":"51","extra":true},"extra":true});
    for confirmations in [0, 2, u32::MAX] {
        let mut value = valid.clone();
        value["confirmations"] = json!(confirmations);
        let output: TxOutResponse = serde_json::from_value(value)?;
        assert_eq!(output.confirmations, confirmations);
        assert_eq!(output.output.value.to_sat(), 1);
        assert_eq!(output.output.script_pubkey.as_bytes(), &[0x51]);
    }
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
        assert!(serde_json::from_value::<TxOutResponse>(value).is_err());
    }
    for (field, replacement) in [
        ("confirmations", json!(-1)),
        ("value", json!("1")),
        ("scriptPubKey", json!({"hex":"zz"})),
        ("scriptPubKey", json!({"hex":"51".repeat(10001)})),
    ] {
        let mut value = valid.clone();
        value[field] = replacement;
        assert!(serde_json::from_value::<TxOutResponse>(value).is_err());
    }
    Ok(())
}

#[test]
fn proof_response_preserves_multiple_targets_and_decodes_display_hashes() -> Result<()> {
    let display = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let mut bytes = <[u8; 32]>::from_hex(display)?;
    bytes.reverse();
    let response = json!({"provedathash": BlockHash::all_zeros(), "prooftargets":[0, 1],
        "hashesproven":[display, display], "proofhashes":[display], "extra":true});
    let proof: ChainTipProof = serde_json::from_value(response.clone())?;
    assert_eq!(proof.targets, [0, 1]);
    assert_eq!(proof.leaves, [bytes, bytes]);
    assert_eq!(proof.hashes, [bytes]);
    for (field, value) in [
        ("prooftargets", json!([])),
        ("prooftargets", json!([-1, 0])),
        ("hashesproven", json!([])),
        ("hashesproven", json!(["zz", display])),
        ("proofhashes", json!(["zz"])),
    ] {
        let mut invalid = response.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<ChainTipProof>(invalid).is_err());
    }
    // The RPC schema does not impose our single-coin forest/path bounds.
    let _: ChainTipProof = serde_json::from_value(json!({"provedathash":BlockHash::all_zeros(),
        "prooftargets":[u64::MAX],"hashesproven":[display],"proofhashes":vec![display; 62]}))?;
    let _: ChainTipProof = serde_json::from_value(json!({"provedathash":BlockHash::all_zeros(),
        "prooftargets":[],"hashesproven":[],"proofhashes":[]}))?;
    Ok(())
}
