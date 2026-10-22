//! Private input that the guest turns into the public claim.

use crate::{
    claim::{Claim, Nonce},
    coin::CoinWitness,
    context::ChainContext,
    eligibility::EligibleCoin,
    eligibility::MinimumAmount,
    verification::{self, Signature},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The chain context, private coin witness, M, nonce, and signature. Each part checks its own
/// structure when deserialized; `check` binds them together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInput {
    context: ChainContext,
    witness: CoinWitness,
    minimum: MinimumAmount,
    nonce: Nonce,
    signature: Signature,
}

impl ClaimInput {
    #[must_use]
    pub fn new(
        context: ChainContext,
        witness: CoinWitness,
        minimum: MinimumAmount,
        nonce: Nonce,
        signature: Signature,
    ) -> Self {
        Self {
            context,
            witness,
            minimum,
            nonce,
            signature,
        }
    }

    /// Build the claim only for an eligible coin whose script verifies its claim signature.
    /// The claim never names the coin or reveals the signature.
    ///
    /// # Errors
    /// Rejects ineligible coins and invalid message signatures for the coin's script.
    pub fn check(self) -> Result<Claim> {
        let coin = EligibleCoin::try_new(self.witness, &self.context, self.minimum)?;
        let claim = Claim::new(&self.context, self.minimum, self.nonce);
        verification::verify_signature(
            &coin.witness().coin().output().script_pubkey,
            &claim.signing_message(),
            &self.signature,
        )?;
        Ok(claim)
    }
}

#[cfg(test)]
mod tests;
