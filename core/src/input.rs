//! Private input that the guest turns into the public claim.

use crate::{
    claim::Claim,
    coin::CoinWitness,
    context::ChainContext,
    eligibility::EligibleCoin,
    eligibility::MinimumAmount,
    verification::{Signature, verify_signature},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The chain context, private coin witness, M, and signed message. Each part checks its own
/// structure when deserialized; `check` binds them together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInput {
    context: ChainContext,
    witness: CoinWitness,
    minimum: MinimumAmount,
    message: String,
    signature: Signature,
}

impl ClaimInput {
    #[must_use]
    pub fn new(
        context: ChainContext,
        witness: CoinWitness,
        minimum: MinimumAmount,
        message: String,
        signature: Signature,
    ) -> Self {
        Self {
            context,
            witness,
            minimum,
            message,
            signature,
        }
    }

    /// Build the claim only for an eligible coin whose script verifies the message signature.
    /// The claim never names the coin or reveals the signature.
    ///
    /// # Errors
    /// Rejects ineligible coins and invalid message signatures for the coin's script.
    pub fn check(self) -> Result<Claim> {
        let coin = EligibleCoin::new(self.witness, &self.context, self.minimum)?;
        verify_signature(
            &coin.witness().coin().output().script_pubkey,
            &self.message,
            &self.signature,
        )?;
        Ok(Claim::new(&self.context, self.minimum, &self.message))
    }
}

#[cfg(test)]
mod tests;
