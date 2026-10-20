use anyhow::Result;
use bitcoin::{
    Amount, BlockHash, Network, OutPoint, ScriptBuf, TxOut, Txid,
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

#[test]
fn guest_commits_the_claim_only_for_an_eligible_coin() -> Result<()> {
    let coin = Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(VALUE),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
        },
    )?;
    let context = context(vec![coin.leaf_hash()])?;
    let witness = CoinWitness::new(coin, context.last().hash(), 0, vec![])?;
    let minimum = MinimumAmount::try_from(VALUE)?;

    let session = run(&ClaimInput::new(context.clone(), witness.clone(), minimum))?;
    assert_eq!(session.exit_code, ExitCode::Halted(0));
    assert_eq!(
        session.journal.bytes,
        Claim::new(&context, minimum).to_bytes()
    );

    // Exit code 1 is the guest's rejection, not a panic or a malformed input.
    for rejected in [
        ClaimInput::new(
            context.clone(),
            witness.clone(),
            MinimumAmount::try_from(VALUE + 1)?,
        ),
        ClaimInput::new(self::context(vec![[0; 32]])?, witness, minimum),
    ] {
        let session = run(&rejected)?;
        assert_eq!(session.exit_code, ExitCode::Halted(1));
        assert!(session.journal.bytes.is_empty());
    }
    Ok(())
}
