use anyhow::Result;
use bitcoin::{
    Address, Amount, BlockHash, Network, OutPoint, TxOut, Txid,
    blockdata::constants::genesis_block, hashes::Hash,
};
use greenhold_core::{
    claim::Claim,
    coin::{Coin, CoinWitness},
    context::{AccumulatorState, BlockEndpoint, ChainContext},
    eligibility::MinimumAmount,
    input::ClaimInput,
};
use greenhold_guest_builder::GREENHOLD_GUEST_ELF;
use risc0_zkvm::{Executor, ExecutorEnv, ExitCode, LocalProver, SessionInfo};

const VALUE: u64 = 12_345;

#[path = "../../core/tests/fixtures/signatures.rs"]
mod signatures;
use signatures::{CASES, MESSAGE, P2TR, P2TR_SIMPLE, P2WPKH, P2WPKH_SIMPLE};

fn run(input: &ClaimInput) -> Result<SessionInfo> {
    let env = ExecutorEnv::builder().write(input)?.build()?;
    LocalProver::new("local").execute(env, GREENHOLD_GUEST_ELF)
}

/// Synthetic chain data: this checks the guest program, not accepted chain state.
fn context(roots: Vec<[u8; 32]>) -> Result<ChainContext> {
    let header = genesis_block(Network::Regtest).header;
    ChainContext::new(
        BlockEndpoint::new(Network::Regtest, 10, header)?,
        BlockEndpoint::new(Network::Regtest, 20, header)?,
        AccumulatorState::new(1, roots)?,
    )
}

fn input(
    address: &str,
    message: &str,
    signature: &str,
    value: u64,
    valid_root: bool,
) -> Result<(ClaimInput, Claim)> {
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: address
                .parse::<Address<_>>()?
                .assume_checked()
                .script_pubkey(),
        },
    )?;
    let context = context(vec![if valid_root {
        coin.leaf_hash()
    } else {
        [0; 32]
    }])?;
    let witness = CoinWitness::new(coin, context.last().hash(), 0, vec![])?;
    let minimum = MinimumAmount::try_from(value)?;
    let claim = Claim::new(&context, minimum, message);
    Ok((
        ClaimInput::new(
            context,
            witness,
            minimum,
            message.into(),
            signature.parse()?,
        ),
        claim,
    ))
}

#[test]
fn guest_verifies_each_signature_format_and_commits_the_message_digest() -> Result<()> {
    for (address, signature) in CASES.into_iter().chain([(P2WPKH, &P2WPKH_SIMPLE[3..])]) {
        let (input, claim) = input(address, MESSAGE, signature, VALUE, true)?;
        let session = run(&input)?;
        assert_eq!(
            session.exit_code,
            ExitCode::Halted(0),
            "{address} {signature}"
        );
        assert_eq!(session.journal.bytes, claim.to_bytes());
    }
    // An empty arbitrary message is valid too (official BIP-322 vector).
    let (input, claim) = input(
        "bc1q9vza2e8x573nczrlzms0wvx3gsqjx7vavgkx0l",
        "",
        "smpAkcwRAIgM2gBAQqvZX15ZiysmKmQpDrG83avLIT492QBzLnQIxYCIBaTpOaD20qRlEylyxFSeEA2ba9YOixpX8z46TSDtS40ASECx/EgAxlkQpQ9hYjgGu6EBCPMVPwVIVJqO4XCsMvViHI=",
        VALUE,
        true,
    )?;
    let session = run(&input)?;
    assert_eq!(session.exit_code, ExitCode::Halted(0));
    assert_eq!(session.journal.bytes, claim.to_bytes());
    Ok(())
}

#[test]
fn guest_rejects_invalid_signatures_messages_scripts_and_coins_without_the_host() -> Result<()> {
    // Exit code 1 is the guest's rejection, not a panic or a malformed input.
    for (address, message, signature, minimum, root) in [
        (P2WPKH, MESSAGE, P2WPKH_SIMPLE, VALUE + 1, true),
        (P2WPKH, MESSAGE, P2WPKH_SIMPLE, VALUE, false),
        (P2WPKH, "another message", P2WPKH_SIMPLE, VALUE, true),
        (P2TR, "another message", P2TR_SIMPLE, VALUE, true),
        (P2TR, MESSAGE, P2WPKH_SIMPLE, VALUE, true),
        (P2WPKH, MESSAGE, P2TR_SIMPLE, VALUE, true),
        (P2WPKH, MESSAGE, "smpAA==", VALUE, true),
        // A correctly encoded but forged Schnorr signature.
        (
            P2TR,
            MESSAGE,
            "smpAUAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            VALUE,
            true,
        ),
    ] {
        let (rejected, _) = input(address, message, signature, minimum, root)?;
        let session = run(&rejected)?;
        assert_eq!(session.exit_code, ExitCode::Halted(1));
        assert!(session.journal.bytes.is_empty());
    }
    Ok(())
}
