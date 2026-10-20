use super::*;
use crate::{
    context::BlockEndpoint,
    test_support::{VALUE, coin, proved},
};
use bitcoin::{Network, blockdata::constants::genesis_block};
use serde_json::json;

fn minimum(sats: u64) -> Result<MinimumAmount> {
    MinimumAmount::try_from(sats)
}

fn check(creation: u32, coinbase: bool, first: u32, last: u32, sats: u64) -> Result<EligibleCoin> {
    let (witness, context) = proved(coin(creation, coinbase)?, first, last)?;
    EligibleCoin::new(witness, &context, minimum(sats)?)
}

fn reject(result: Result<EligibleCoin>, reason: &str) -> Result<()> {
    let error = result.err().context("ineligible coin accepted")?;
    assert!(error.to_string().contains(reason), "{error:#}");
    Ok(())
}

#[test]
fn eligibility_requires_membership_creation_value_and_maturity() -> Result<()> {
    let eligible = check(10, false, 10, 10, VALUE)?;
    assert_eq!(eligible.witness().coin(), &coin(10, false)?);
    reject(check(10, false, 10, 10, VALUE + 1), "below the minimum")?;
    reject(check(11, false, 10, 20, 1), "first-height cutoff")?;

    // Spendable in block T + 1 needs T + 1 - creation >= 100.
    check(21, true, 21, 120, 1)?;
    reject(
        check(22, true, 22, 120, 1),
        "coinbase coin is not spendable",
    )?;
    check(120, false, 120, 120, 1)?;

    // Every rule runs on a membership-checked witness, never on raw coin data.
    let (witness, context) = proved(coin(10, false)?, 10, 10)?;
    let (_, other_state) = proved(coin(9, false)?, 10, 10)?;
    reject(
        EligibleCoin::new(witness.clone(), &other_state, minimum(1)?),
        "membership mismatch",
    )?;
    let later = ChainContext::new(
        context.first().clone(),
        BlockEndpoint::new(Network::Regtest, 11, genesis_block(Network::Bitcoin).header)?,
        context.state().clone(),
    )?;
    reject(
        EligibleCoin::new(witness, &later, minimum(1)?),
        "proof block",
    )?;
    Ok(())
}

#[test]
fn minimum_amount_is_one_satoshi_to_money_limit_in_every_input_form() -> Result<()> {
    let max = Amount::MAX_MONEY.to_sat();
    for sats in [1, max] {
        assert_eq!(u64::from(minimum(sats)?), sats);
        assert_eq!(sats.to_string().parse::<MinimumAmount>()?, minimum(sats)?);
        let value = serde_json::to_value(minimum(sats)?)?;
        assert_eq!(value, json!(sats));
        assert_eq!(
            serde_json::from_value::<MinimumAmount>(value)?,
            minimum(sats)?
        );
    }
    for sats in [0, max + 1, u64::MAX] {
        assert!(minimum(sats).is_err(), "{sats}");
        assert!(sats.to_string().parse::<MinimumAmount>().is_err(), "{sats}");
        assert!(serde_json::from_value::<MinimumAmount>(json!(sats)).is_err());
    }
    for text in ["", "-1", "1.5", "one", "18446744073709551616"] {
        assert!(text.parse::<MinimumAmount>().is_err(), "{text}");
    }
    assert!(serde_json::from_value::<MinimumAmount>(json!("1")).is_err());
    Ok(())
}
