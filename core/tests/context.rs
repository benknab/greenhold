use anyhow::Result;
use bitcoin::{Network, blockdata::constants::genesis_block};
use serde_json::json;
use walledcity_core::context::{AccumulatorState, BlockEndpoint, ChainContext};

fn block(height: u32) -> Result<BlockEndpoint> {
    // Synthetic height binding for component tests, never accepted chain state.
    BlockEndpoint::new(
        Network::Bitcoin,
        height,
        genesis_block(Network::Bitcoin).header,
    )
}

#[test]
fn digest_vector_and_round_trip() -> Result<()> {
    let context = ChainContext::new(
        block(0)?,
        block(0)?,
        AccumulatorState::new(3, vec![[0x11; 32], [0x22; 32]])?,
    )?;
    assert_eq!(
        context.digest().to_string(),
        "57c57dffa8aaa3a7033349c053a56df38e788e806cba903153d0c19deac0f595"
    );
    let decoded: ChainContext = serde_json::from_slice(&serde_json::to_vec(&context)?)?;
    assert_eq!(decoded, context);
    assert_eq!(decoded.digest(), context.digest());
    assert_eq!(decoded.state().num_leaves(), 3);
    assert_eq!(decoded.state().roots(), &[[0x11; 32], [0x22; 32]]);
    Ok(())
}

#[test]
fn digest_binds_endpoints_leaf_count_and_ordered_roots() -> Result<()> {
    let state = AccumulatorState::new(3, vec![[1; 32], [2; 32]])?;
    let context = ChainContext::new(block(1)?, block(2)?, state.clone())?;
    for changed in [
        ChainContext::new(block(0)?, block(2)?, state.clone())?,
        ChainContext::new(block(1)?, block(3)?, state.clone())?,
        ChainContext::new(
            block(1)?,
            block(2)?,
            AccumulatorState::new(5, vec![[1; 32], [2; 32]])?,
        )?,
        ChainContext::new(
            block(1)?,
            block(2)?,
            AccumulatorState::new(3, vec![[2; 32], [1; 32]])?,
        )?,
        ChainContext::new(
            block(1)?,
            block(2)?,
            AccumulatorState::new(3, vec![[1; 32], [3; 32]])?,
        )?,
        ChainContext::new(
            block(1)?,
            BlockEndpoint::new(Network::Bitcoin, 2, genesis_block(Network::Testnet).header)?,
            state,
        )?,
    ] {
        assert_ne!(context.digest(), changed.digest());
    }
    Ok(())
}

#[test]
fn constructors_reject_invalid_structure_and_work() -> Result<()> {
    assert!(ChainContext::new(block(2)?, block(1)?, AccumulatorState::new(0, vec![])?).is_err());
    assert!(
        ChainContext::new(
            block(1)?,
            BlockEndpoint::new(Network::Bitcoin, 1, genesis_block(Network::Testnet).header)?,
            AccumulatorState::new(0, vec![])?
        )
        .is_err()
    );
    assert!(
        BlockEndpoint::new(Network::Bitcoin, 0, genesis_block(Network::Testnet).header).is_err()
    );
    assert!(
        BlockEndpoint::new(Network::Bitcoin, 1, genesis_block(Network::Regtest).header).is_err()
    );
    let mut invalid = genesis_block(Network::Bitcoin).header;
    invalid.nonce = 0;
    assert!(BlockEndpoint::new(Network::Bitcoin, 0, invalid).is_err());
    assert!(AccumulatorState::new(0, vec![]).is_ok());
    assert!(AccumulatorState::new(0, vec![[0; 32]]).is_err());
    assert!(AccumulatorState::new(3, vec![[0; 32]]).is_err());
    assert!(AccumulatorState::new(1 << 62, vec![[0; 32]]).is_err());
    assert!(AccumulatorState::new((1 << 62) - 1, vec![[0; 32]; 62]).is_ok());
    Ok(())
}

#[test]
fn deserialization_cannot_bypass_invariants() -> Result<()> {
    let valid = serde_json::to_value(ChainContext::new(
        block(1)?,
        block(2)?,
        AccumulatorState::new(1, vec![[0; 32]])?,
    )?)?;
    for (pointer, replacement) in [
        ("/first/height", json!(3)),
        ("/last/network", json!("regtest")),
        ("/state/num_leaves", json!(0)),
        ("/state/num_leaves", json!(1_u64 << 62)),
        ("/state/roots", json!([])),
        ("/last/header/nonce", json!(0)),
    ] {
        let mut changed = valid.clone();
        let field = changed
            .pointer_mut(pointer)
            .ok_or_else(|| anyhow::anyhow!("missing field {pointer}"))?;
        *field = replacement;
        assert!(serde_json::from_value::<ChainContext>(changed).is_err());
    }
    Ok(())
}

#[test]
fn networks_select_genesis_and_work_limits_and_bind_the_digest() -> Result<()> {
    let networks = [Network::Bitcoin, Network::Signet, Network::Regtest];
    let mut digests = Vec::new();
    for network in networks {
        let genesis = BlockEndpoint::new(network, 0, genesis_block(network).header)?;
        let context =
            ChainContext::new(genesis.clone(), genesis, AccumulatorState::new(0, vec![])?)?;
        assert_eq!(context.network(), network);
        assert_eq!(
            serde_json::from_slice::<ChainContext>(&serde_json::to_vec(&context)?)?,
            context
        );

        // The same non-genesis header and state must hash differently on each network.
        // A header's work alone cannot identify its chain; nodes establish that binding.
        let endpoint = BlockEndpoint::new(network, 1, genesis_block(Network::Bitcoin).header)?;
        let digest = ChainContext::new(
            endpoint.clone(),
            endpoint,
            AccumulatorState::new(0, vec![])?,
        )?
        .digest();
        assert!(!digests.contains(&digest));
        digests.push(digest);

        for other in networks {
            if other != network {
                assert!(BlockEndpoint::new(network, 0, genesis_block(other).header).is_err());
            }
        }
    }
    assert!(
        BlockEndpoint::new(Network::Signet, 1, genesis_block(Network::Regtest).header).is_err()
    );
    assert!(
        BlockEndpoint::new(Network::Regtest, 1, genesis_block(Network::Regtest).header).is_ok()
    );
    let mainnet = BlockEndpoint::new(Network::Bitcoin, 1, genesis_block(Network::Bitcoin).header)?;
    let signet = BlockEndpoint::new(Network::Signet, 2, genesis_block(Network::Bitcoin).header)?;
    assert!(ChainContext::new(mainnet, signet, AccumulatorState::new(0, vec![])?).is_err());
    Ok(())
}

#[test]
fn rejects_testnets_in_constructors_and_serialized_contexts() -> Result<()> {
    let context = ChainContext::new(block(1)?, block(2)?, AccumulatorState::new(0, vec![])?)?;
    for network in [Network::Testnet, Network::Testnet4] {
        assert!(BlockEndpoint::new(network, 0, genesis_block(network).header).is_err());
        let mut encoded = serde_json::to_value(&context)?;
        encoded["first"]["network"] = json!(network);
        encoded["last"]["network"] = json!(network);
        assert!(serde_json::from_value::<ChainContext>(encoded).is_err());
    }
    Ok(())
}
