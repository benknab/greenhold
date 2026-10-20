use anyhow::{Context as _, Result, bail};
use greenhold_core::{claim::Claim, input::ClaimInput};
use greenhold_guest_builder::{GREENHOLD_GUEST_ELF, GREENHOLD_GUEST_ID};
use risc0_zkvm::{Executor, ExecutorEnv, ExitCode, LocalProver, sha::Digest};

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
    let claim = Claim::try_from(session.journal.bytes.as_slice()).context("guest journal")?;
    if &claim != expected {
        bail!("guest claim does not match the host claim");
    }
    Ok(session.cycles())
}
