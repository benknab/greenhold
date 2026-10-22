use crate::progress;
use anyhow::{Context as _, Result, bail};
use greenhold_core::claim::Claim;
use greenhold_core::input::ClaimInput;
use greenhold_guest_builder::{GREENHOLD_GUEST_ELF, GREENHOLD_GUEST_ID};
use risc0_zkvm::sha::Digest;
use risc0_zkvm::{
    Executor, ExecutorEnv, ExitCode, InnerReceipt, LocalProver, Prover, ProverOpts, Receipt,
};

#[must_use]
pub fn image_id() -> Digest {
    Digest::from(GREENHOLD_GUEST_ID)
}

/// Execute the guest before proving. The journal must be exactly the expected claim.
/// Any other result is a bug, not a rejection.
pub fn execute(input: &ClaimInput, expected: &Claim) -> Result<()> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    let session = LocalProver::new("local")
        .execute(env, GREENHOLD_GUEST_ELF)
        .context("guest execution failed")?;
    if session.exit_code != ExitCode::Halted(0) {
        bail!("guest rejected the claim input ({:?})", session.exit_code);
    }
    check_journal(&session.journal.bytes, expected)
}

/// Prove the guest and verify the receipt against this guest's image ID. The receipt's
/// journal must be exactly the claim the host expects.
pub fn prove(input: &ClaimInput, expected: &Claim) -> Result<Receipt> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    // Unlike composite receipts, succinct receipts do not leak the execution length.
    let receipt = progress::run("Generating proof", || {
        Ok(LocalProver::new("local")
            .prove_with_opts(env, GREENHOLD_GUEST_ELF, &ProverOpts::succinct())
            .context("guest proving failed")?
            .receipt)
    })?;
    progress::run("Checking receipt", || {
        if &verify(&receipt)? != expected {
            bail!("guest claim does not match the host claim");
        }
        Ok(())
    })?;
    Ok(receipt)
}

/// Check a receipt without node access: a succinct proof that this guest halted
/// normally and committed a well-formed claim. Chain state still needs checking.
pub fn verify(receipt: &Receipt) -> Result<Claim> {
    if !matches!(receipt.inner, InnerReceipt::Succinct(_)) {
        bail!("receipt is not succinct");
    }
    // Verification requires exit code Halted(0), so a rejected input cannot pass.
    receipt
        .verify(image_id())
        .context("receipt verification failed")?;
    Claim::try_from(receipt.journal.bytes.as_slice()).context("receipt journal")
}

fn check_journal(journal: &[u8], expected: &Claim) -> Result<()> {
    let claim = Claim::try_from(journal).context("guest journal")?;
    if &claim != expected {
        bail!("guest claim does not match the host claim");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
