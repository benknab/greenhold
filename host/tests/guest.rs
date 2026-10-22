use anyhow::Result;
use greenhold_core::{context::ChainContext, input::ClaimInput};
use greenhold_guest_builder::GREENHOLD_GUEST_ELF;
use greenhold_signer::{Format, ScriptType};
use risc0_zkvm::{Executor, ExecutorEnv, ExitCode, LocalProver, SessionInfo};
use serde_json::json;

#[path = "fixtures/claim.rs"]
mod fixture;

fn run(input: &ClaimInput) -> Result<SessionInfo> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    LocalProver::new("local").execute(env, GREENHOLD_GUEST_ELF)
}

#[test]
fn guest_verifies_each_format_over_its_own_claim() -> Result<()> {
    for script in [
        ScriptType::P2pkh,
        ScriptType::P2shP2wpkh,
        ScriptType::P2wpkh,
        ScriptType::P2tr,
    ] {
        for &format in script.formats() {
            let (input, claim) = fixture::signed(script, format)?;
            let session = run(&input)?;
            assert_eq!(
                session.exit_code,
                ExitCode::Halted(0),
                "{script:?}/{format:?}"
            );
            assert_eq!(session.journal.bytes, claim.to_bytes());
        }
    }
    Ok(())
}

#[test]
fn guest_rejects_changed_claims_and_witnesses_without_host_checks() -> Result<()> {
    let (input, _) = fixture::signed(ScriptType::P2wpkh, Format::Simple)?;
    let original = serde_json::to_value(&input)?;
    for (pointer, value) in [
        ("/minimum", json!(fixture::VALUE - 1)), // Still eligible: must fail signature binding.
        ("/minimum", json!(fixture::VALUE + 1)),
        ("/nonce", json!("cd".repeat(32))), // Still eligible: only the signed challenge changes.
        ("/context/unspent_since/height", json!(11)), // Still eligible, different X.
        ("/context/unspent_at/height", json!(21)), // Still eligible, different T.
        ("/context/state/roots", json!(vec![[0; 32]])),
        ("/witness/proved_at", json!("00".repeat(32))),
        ("/signature", json!("smpAA==")),
    ] {
        let mut changed = original.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or_else(|| anyhow::anyhow!("missing {pointer}"))? = value;
        let session = run(&serde_json::from_value(changed)?)?;
        assert_eq!(session.exit_code, ExitCode::Halted(1), "{pointer}");
        assert!(session.journal.bytes.is_empty());
    }
    // A self-consistent different script/root still cannot use the original signature.
    let (other, _) = fixture::signed(ScriptType::P2tr, Format::Simple)?;
    let mut changed = serde_json::to_value(other)?;
    changed["signature"] = original["signature"].clone();
    let session = run(&serde_json::from_value(changed)?)?;
    assert_eq!(session.exit_code, ExitCode::Halted(1));
    assert!(session.journal.bytes.is_empty());

    // A well-formed but forged Schnorr signature for a correctly authenticated P2TR coin.
    let (taproot, _) = fixture::signed(ScriptType::P2tr, Format::Simple)?;
    let mut forged = serde_json::to_value(taproot)?;
    forged["signature"] = json!(
        "smpAUAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
    let session = run(&serde_json::from_value(forged)?)?;
    assert_eq!(session.exit_code, ExitCode::Halted(1));
    assert!(session.journal.bytes.is_empty());

    // Change a valid PoW header at X while retaining the authenticated state at T.
    let mut changed = original.clone();
    changed["context"]["unspent_since"]["header"] = serde_json::to_value(
        bitcoin::blockdata::constants::genesis_block(bitcoin::Network::Bitcoin).header,
    )?;
    let _: ChainContext = serde_json::from_value(changed["context"].clone())?;
    let session = run(&serde_json::from_value(changed)?)?;
    assert_eq!(session.exit_code, ExitCode::Halted(1));
    assert!(session.journal.bytes.is_empty());
    Ok(())
}

#[test]
fn guest_rejects_rebinding_the_signature_to_a_different_valid_context() -> Result<()> {
    use bitcoin::{Network, blockdata::constants};
    use greenhold_core::{
        coin::CoinWitness,
        eligibility::{EligibleCoin, MinimumAmount},
    };
    let (input, _) = fixture::signed(ScriptType::P2wpkh, Format::Simple)?;
    let original = serde_json::to_value(input)?;
    for case in ["unspent_at block", "network", "accumulator"] {
        let mut changed = original.clone();
        match case {
            "unspent_at block" | "network" => {
                let header = constants::genesis_block(Network::Bitcoin).header;
                changed["context"]["unspent_at"]["header"] = serde_json::to_value(header)?;
                changed["witness"]["proved_at"] = json!(header.block_hash());
                if case == "network" {
                    changed["context"]["unspent_since"]["header"] = serde_json::to_value(header)?;
                    changed["context"]["unspent_since"]["network"] = json!("bitcoin");
                    changed["context"]["unspent_at"]["network"] = json!("bitcoin");
                }
            }
            _ => {
                // Preserve the coin's membership as a promoted leaf, changing only
                // the forest representation committed by the context digest.
                changed["context"]["state"]["num_leaves"] = json!(3);
                let root = changed["context"]["state"]["roots"][0].clone();
                changed["context"]["state"]["roots"] = json!([root, vec![1; 32]]);
                changed["witness"]["target"] = json!(1_u64 << 63);
            }
        }
        let context: ChainContext = serde_json::from_value(changed["context"].clone())?;
        let witness: CoinWitness = serde_json::from_value(changed["witness"].clone())?;
        EligibleCoin::new(witness, &context, MinimumAmount::try_from(fixture::VALUE)?)?;
        let session = run(&serde_json::from_value(changed)?)?;
        assert_eq!(session.exit_code, ExitCode::Halted(1), "{case}");
        assert!(session.journal.bytes.is_empty());
    }
    Ok(())
}
