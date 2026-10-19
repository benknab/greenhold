use super::*;
use crate::context::{AccumulatorState, BlockEndpoint};
use anyhow::Context;
use bitcoin::{Network, ScriptBuf, Txid, blockdata::constants::genesis_block, hex::FromHex};
use serde_json::json;

fn coin() -> Result<Coin> {
    Coin::new(
        OutPoint::new(Txid::from_byte_array([1; 32]), 2),
        BlockHash::from_byte_array([2; 32]),
        10,
        false,
        TxOut {
            value: Amount::from_sat(12345),
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
        },
    )
}

fn context(leaves: u64, roots: Vec<[u8; 32]>) -> Result<ChainContext> {
    let header = genesis_block(Network::Regtest).header;
    ChainContext::new(
        BlockEndpoint::new(Network::Regtest, 10, header)?,
        BlockEndpoint::new(Network::Regtest, 20, header)?,
        AccumulatorState::new(leaves, roots)?,
    )
}

fn reject(result: Result<()>, reason: &str) -> Result<()> {
    let error = result.err().context("invalid witness accepted")?;
    assert!(error.to_string().contains(reason), "{error:#}");
    Ok(())
}

#[test]
fn single_coin_paths_bind_all_leaf_fields_and_proof_block() -> Result<()> {
    let coin = coin()?;
    let leaf = coin.leaf_hash();
    // Independent vector from Python hashlib over the explicit wire preimage.
    assert_eq!(
        leaf,
        <[u8; 32]>::from_hex("e172438aee336b32238371d70c398e05432d8786375918d752a78ea2cbe2c377")?
    );
    let sibling = [42; 32];
    let root: [u8; 32] = Sha512_256::digest([leaf, sibling].concat()).into();
    let state = context(3, vec![root, [43; 32]])?;
    let witness = CoinWitness::new(coin.clone(), state.last().hash(), 0, vec![sibling])?;
    witness.verify_membership(&state)?;
    let json = serde_json::to_value(&witness)?;
    for (field, replacement) in [
        (
            "creation_block_hash",
            json!(BlockHash::from_byte_array([3; 32])),
        ),
        ("creation_height", json!(9)),
        ("coinbase", json!(true)),
        (
            "output",
            json!(TxOut {
                value: Amount::from_sat(12346),
                ..coin.output.clone()
            }),
        ),
        (
            "output",
            json!(TxOut {
                script_pubkey: ScriptBuf::new(),
                ..coin.output.clone()
            }),
        ),
        ("outpoint", json!(OutPoint::new(coin.outpoint.txid, 3))),
        (
            "outpoint",
            json!(OutPoint::new(Txid::from_byte_array([9; 32]), 2)),
        ),
    ] {
        let mut altered = json.clone();
        altered["coin"][field] = replacement;
        let altered: CoinWitness = serde_json::from_value(altered)?;
        reject(altered.verify_membership(&state), "membership mismatch")?;
    }
    let mut wrong = witness.clone();
    wrong.proved_at = BlockHash::all_zeros();
    reject(wrong.verify_membership(&state), "proof block")?;
    wrong = witness.clone();
    wrong.hashes[0][0] ^= 1;
    reject(wrong.verify_membership(&state), "membership mismatch")?;
    wrong = witness.clone();
    wrong.hashes.clear();
    reject(wrong.verify_membership(&state), "path length")?;
    wrong = witness.clone();
    wrong.hashes.push(sibling);
    reject(wrong.verify_membership(&state), "path length")?;
    wrong = witness;
    wrong.target = 1;
    reject(wrong.verify_membership(&state), "membership mismatch")?;
    Ok(())
}

#[test]
fn sparse_forests_right_children_and_promoted_leaves() -> Result<()> {
    let coin = coin()?;
    let leaf = coin.leaf_hash();
    let sibling = [7; 32];
    let root: [u8; 32] = Sha512_256::digest([sibling, leaf].concat()).into();
    let deep_root: [u8; 32] = Sha512_256::digest([sibling, root].concat()).into();
    for (leaves, roots, target, hashes) in [
        (4, vec![deep_root], 3, vec![sibling, sibling]),
        (3, vec![root, sibling], 1, vec![sibling]),
        (3, vec![sibling, leaf], 2, vec![]),
        (2, vec![leaf], 1 << 63, vec![]), // survivor promoted after deleting its sibling
        (4, vec![root], (1 << 63) + 1, vec![sibling]),
        (4, vec![leaf], 3 << 62, vec![]), // promoted twice to a row-two root
        (8, vec![root], (3 << 62) + 1, vec![sibling]),
        (1, vec![leaf], 0, vec![]),
        ((1 << 61) + 1, vec![sibling, leaf], 1 << 61, vec![]),
    ] {
        let state = context(leaves, roots)?;
        CoinWitness::new(coin.clone(), state.last().hash(), target, hashes)?
            .verify_membership(&state)?;
    }
    for (leaves, roots, target) in [
        (0, vec![], 0),
        (3, vec![leaf, leaf], 3),
        (3, vec![leaf, leaf], (1 << 63) + 1),
        (1, vec![leaf], 1 << 63),
        (3, vec![leaf, leaf], (1 << 63) - 1),
    ] {
        let state = context(leaves, roots)?;
        reject(
            CoinWitness::new(coin.clone(), state.last().hash(), target, vec![])?
                .verify_membership(&state),
            "outside occupied forest",
        )?;
    }
    Ok(())
}

#[test]
fn deserialization_preserves_coin_and_path_bounds() -> Result<()> {
    let coin = coin()?;
    let witness = CoinWitness::new(coin, BlockHash::all_zeros(), 0, vec![])?;
    let mut value = serde_json::to_value(witness)?;
    value["extra"] = json!(true);
    value["coin"]["extra"] = json!(true);
    let _: CoinWitness = serde_json::from_value(value.clone())?;
    for (field, replacement) in [
        ("creation_height", json!(0)),
        ("creation_height", json!(1u64 << 31)),
        ("outpoint", json!(OutPoint::null())),
        (
            "output",
            json!(TxOut {
                value: Amount::from_sat(Amount::MAX_MONEY.to_sat() + 1),
                script_pubkey: ScriptBuf::new()
            }),
        ),
        (
            "output",
            json!(TxOut {
                value: Amount::ZERO,
                script_pubkey: ScriptBuf::from_bytes(vec![0; 10001])
            }),
        ),
    ] {
        let mut invalid = value.clone();
        invalid["coin"][field] = replacement;
        assert!(serde_json::from_value::<CoinWitness>(invalid).is_err());
    }
    for (field, replacement) in [
        ("target", json!(u64::MAX)),
        ("hashes", json!(vec![[0u8; 32]; 62])),
    ] {
        let mut invalid = value.clone();
        invalid[field] = replacement;
        assert!(serde_json::from_value::<CoinWitness>(invalid).is_err());
    }
    value["coin"]
        .as_object_mut()
        .context("coin object")?
        .remove("coinbase");
    assert!(serde_json::from_value::<CoinWitness>(value).is_err());
    Ok(())
}
