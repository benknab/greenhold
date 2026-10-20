//! Private input that the guest turns into the public claim.

use crate::{
    claim::Claim, coin::CoinWitness, context::ChainContext, eligibility::EligibleCoin,
    eligibility::MinimumAmount,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The chain context, the private coin witness, and M. Each part checks its own
/// structure when deserialized; `check` binds them together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInput {
    context: ChainContext,
    witness: CoinWitness,
    minimum: MinimumAmount,
}

impl ClaimInput {
    #[must_use]
    pub fn new(context: ChainContext, witness: CoinWitness, minimum: MinimumAmount) -> Self {
        Self {
            context,
            witness,
            minimum,
        }
    }

    /// Build the claim only for an eligible coin. The claim never names the coin.
    ///
    /// # Errors
    /// Rejects any coin that `EligibleCoin::new` rejects.
    pub fn check(self) -> Result<Claim> {
        EligibleCoin::new(self.witness, &self.context, self.minimum)?;
        Ok(Claim::new(&self.context, self.minimum))
    }
}

#[cfg(test)]
mod tests;
