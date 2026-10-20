use super::*;
use crate::test_support::{VALUE, coin, proved};

#[test]
fn input_yields_the_claim_only_for_an_eligible_coin() -> Result<()> {
    let (witness, context) = proved(coin(10, false)?, 10, 20)?;
    let minimum = MinimumAmount::try_from(VALUE)?;
    let input = ClaimInput::new(context.clone(), witness.clone(), minimum);
    assert_eq!(input.check()?, Claim::new(&context, minimum));

    let above = MinimumAmount::try_from(VALUE + 1)?;
    let error = ClaimInput::new(context, witness, above)
        .check()
        .err()
        .ok_or_else(|| anyhow::anyhow!("ineligible coin produced a claim"))?;
    assert!(error.to_string().contains("below the minimum"), "{error:#}");
    Ok(())
}
