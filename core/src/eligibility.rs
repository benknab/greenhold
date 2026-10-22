//! Coin eligibility for a claim. Authorization is checked separately.

use crate::{
    coin::{Coin, CoinWitness},
    context::ChainContext,
};
use anyhow::{Context as _, Result, bail};
use bitcoin::Amount;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Consensus rule: a coinbase output is spendable in a block at least 100 blocks later.
const COINBASE_MATURITY: u32 = 100;

/// The claim's minimum coin value M, from one satoshi to Bitcoin's money limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct MinimumAmount(Amount);

impl MinimumAmount {
    #[must_use]
    pub fn amount(self) -> Amount {
        self.0
    }
}

impl TryFrom<u64> for MinimumAmount {
    type Error = anyhow::Error;

    fn try_from(sats: u64) -> Result<Self> {
        let amount = Amount::from_sat(sats);
        if amount == Amount::ZERO || amount > Amount::MAX_MONEY {
            bail!(
                "minimum amount must be 1 to {} satoshis",
                Amount::MAX_MONEY.to_sat()
            );
        }
        Ok(Self(amount))
    }
}

impl From<MinimumAmount> for u64 {
    fn from(minimum: MinimumAmount) -> Self {
        minimum.0.to_sat()
    }
}

impl FromStr for MinimumAmount {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let sats = text
            .parse::<u64>()
            .context("minimum amount must be a whole number of satoshis")?;
        Self::try_from(sats)
    }
}

/// A coin proven unspent at T that meets the claim's creation and value rules.
/// This does not establish who controls the coin.
#[derive(Clone, Debug)]
pub struct EligibleCoin {
    witness: CoinWitness,
}

impl EligibleCoin {
    /// Check membership at T, creation at or before X, value of at least M, and
    /// coinbase maturity at T. Any locking script is accepted.
    ///
    /// # Errors
    /// Rejects failed membership, coins created after X, values below M, and coinbase
    /// coins that cannot be spent in the block after T.
    pub fn try_new(
        witness: CoinWitness,
        context: &ChainContext,
        minimum: MinimumAmount,
    ) -> Result<Self> {
        witness.verify_membership(context)?;
        check_rules(witness.coin(), context, minimum)?;
        Ok(Self { witness })
    }

    #[must_use]
    pub fn witness(&self) -> &CoinWitness {
        &self.witness
    }

    #[must_use]
    pub fn into_witness(self) -> CoinWitness {
        self.witness
    }
}

fn check_rules(coin: &Coin, context: &ChainContext, minimum: MinimumAmount) -> Result<()> {
    if coin.creation_height() > context.unspent_since().height() {
        bail!("coin was created after unspent_since");
    }
    if coin.output().value < minimum.amount() {
        bail!("coin amount is below the minimum");
    }
    // Creation at or before X, and X <= T, keep this subtraction in range.
    // Spending in block T + 1 needs T + 1 - creation >= 100.
    let age = context.unspent_at().height() - coin.creation_height();
    if coin.is_coinbase() && age < COINBASE_MATURITY - 1 {
        bail!("coinbase coin is not spendable after unspent_at");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
