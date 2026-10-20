use super::*;
use crate::{
    coin::Coin,
    test_support::{VALUE, proved},
};
use bitcoin::{
    Amount, BlockHash, Network, OutPoint, TxOut, Txid, hashes::Hash, secp256k1::SecretKey,
};
use greenhold_signer::{Format, ScriptType, address, sign};

fn input(script: ScriptType, format: Format) -> Result<ClaimInput> {
    let key = SecretKey::from_slice(&[1; 32])?;
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: address(script, &key, Network::Regtest).script_pubkey(),
        },
    )?;
    let (witness, context) = proved(coin, 10, 20)?;
    let minimum = MinimumAmount::try_from(VALUE)?;
    let message = Claim::new(&context, minimum).signing_message();
    let signature = sign(script, format, &key, Network::Regtest, &message)?
        .signature
        .parse()?;
    Ok(ClaimInput::new(context, witness, minimum, signature))
}

#[test]
fn input_requires_eligibility_and_a_signature_over_the_exact_claim() -> Result<()> {
    let original = input(ScriptType::P2wpkh, Format::Simple)?;
    assert_eq!(
        original.clone().check()?,
        Claim::new(&original.context, original.minimum)
    );
    for minimum in [VALUE - 1, VALUE + 1] {
        let mut changed = original.clone();
        changed.minimum = MinimumAmount::try_from(minimum)?;
        assert!(changed.check().is_err());
    }
    Ok(())
}

#[test]
fn private_inputs_verify_every_format_after_serialization() -> Result<()> {
    for script in [
        ScriptType::P2pkh,
        ScriptType::P2shP2wpkh,
        ScriptType::P2wpkh,
        ScriptType::P2tr,
    ] {
        for &format in script.formats() {
            let input = input(script, format)?;
            let encoded = serde_json::to_value(&input)?;
            let decoded: ClaimInput = serde_json::from_value(encoded.clone())?;
            assert_eq!(decoded.check()?, Claim::new(&input.context, input.minimum));
            for replacement in ["", "not base64", "pofAAAA"] {
                let mut invalid = encoded.clone();
                invalid["signature"] = replacement.into();
                assert!(serde_json::from_value::<ClaimInput>(invalid).is_err());
            }
            let mut invalid = input;
            invalid.signature = "smpAA==".parse()?;
            assert!(invalid.check().is_err());
        }
    }
    Ok(())
}
