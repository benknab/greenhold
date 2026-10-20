use anyhow::{Context as _, Result, bail};
use greenhold_core::{claim::Claim, input::ClaimInput};
use greenhold_guest_builder::{GREENHOLD_GUEST_ELF, GREENHOLD_GUEST_ID};
use risc0_zkvm::{Executor, ExecutorEnv, ExitCode, LocalProver, Prover, Receipt, sha::Digest};

#[must_use]
pub fn image_id() -> Digest {
    Digest::from(GREENHOLD_GUEST_ID)
}

/// Execute the guest without proving and return its cycle count. The journal must be
/// exactly the claim the host expects; any other result is a bug, not a rejection.
pub fn execute(input: &ClaimInput, expected: &Claim) -> Result<u64> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    let session = LocalProver::new("local")
        .execute(env, GREENHOLD_GUEST_ELF)
        .context("guest execution failed")?;
    if session.exit_code != ExitCode::Halted(0) {
        bail!("guest rejected the claim input ({:?})", session.exit_code);
    }
    check_journal(&session.journal.bytes, expected)?;
    Ok(session.cycles())
}

/// Prove the guest and verify the receipt against this guest's image ID. The receipt's
/// journal must be exactly the claim the host expects.
pub fn prove(input: &ClaimInput, expected: &Claim) -> Result<Receipt> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    let receipt = LocalProver::new("local")
        .prove(env, GREENHOLD_GUEST_ELF)
        .context("guest proving failed")?
        .receipt;
    // Verification requires exit code Halted(0), so a rejected input cannot pass.
    receipt
        .verify(image_id())
        .context("receipt verification failed")?;
    check_journal(&receipt.journal.bytes, expected)?;
    Ok(receipt)
}

fn check_journal(journal: &[u8], expected: &Claim) -> Result<()> {
    let claim = Claim::try_from(journal).context("guest journal")?;
    if &claim != expected {
        bail!("guest claim does not match the host claim");
    }
    Ok(())
}
