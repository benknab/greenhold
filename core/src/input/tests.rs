use super::*;
use crate::{
    coin::Coin,
    test_support::{VALUE, proved, signatures::*},
};
use bitcoin::{Address, Amount, BlockHash, OutPoint, TxOut, Txid, hashes::Hash};

fn input(address: &str, message: &str, signature: &str) -> Result<ClaimInput> {
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: address
                .parse::<Address<_>>()?
                .assume_checked()
                .script_pubkey(),
        },
    )?;
    let (witness, context) = proved(coin, 10, 20)?;
    Ok(ClaimInput::new(
        context,
        witness,
        MinimumAmount::try_from(VALUE)?,
        message.into(),
        signature.parse()?,
    ))
}

#[test]
fn input_yields_the_claim_only_for_an_eligible_coin() -> Result<()> {
    let mut input = input(P2WPKH, MESSAGE, P2WPKH_SIMPLE)?;
    assert_eq!(
        input.clone().check()?,
        Claim::new(&input.context, input.minimum, MESSAGE)
    );
    input.minimum = MinimumAmount::try_from(VALUE + 1)?;
    let error = input
        .check()
        .err()
        .ok_or_else(|| anyhow::anyhow!("ineligible coin produced a claim"))?;
    assert!(error.to_string().contains("below the minimum"), "{error:#}");
    Ok(())
}

#[test]
fn private_inputs_verify_every_format_after_serialization() -> Result<()> {
    for (address, signature) in CASES {
        let input = input(address, MESSAGE, signature)?;
        let encoded = serde_json::to_value(&input)?;
        let decoded: ClaimInput = serde_json::from_value(encoded.clone())?;
        assert_eq!(
            decoded.check()?,
            Claim::new(&input.context, input.minimum, MESSAGE)
        );
        for replacement in ["", "not base64", "pofAAAA"] {
            let mut invalid = encoded.clone();
            invalid["signature"] = replacement.into();
            assert!(serde_json::from_value::<ClaimInput>(invalid).is_err());
        }
        assert!(
            self::input(address, "different message", signature)?
                .check()
                .is_err()
        );
        let other = if address == P2TR { P2WPKH } else { P2TR };
        assert!(self::input(other, MESSAGE, signature)?.check().is_err());
        assert!(self::input(address, MESSAGE, "smpAA==")?.check().is_err());
    }
    Ok(())
}
