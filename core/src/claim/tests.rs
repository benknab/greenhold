use super::*;
use crate::context::AccumulatorState;
use bitcoin::{Amount, block::Header};
use serde_json::json;

const NETWORK: usize = TAG.len();
const FIRST: usize = NETWORK + 32;
const LAST: usize = FIRST + 36;
const MINIMUM: usize = LAST + 36;

fn header(network: Network) -> Header {
    genesis_block(network).header
}

/// Distinct X and T headers, so a swapped field changes the bytes.
fn context(first: u32, last: u32) -> Result<ChainContext> {
    let last_header = if first == last {
        header(Network::Regtest)
    } else {
        header(Network::Bitcoin)
    };
    ChainContext::new(
        BlockEndpoint::new(Network::Regtest, first, header(Network::Regtest))?,
        BlockEndpoint::new(Network::Regtest, last, last_header)?,
        AccumulatorState::new(3, vec![[1; 32], [2; 32]])?,
    )
}

fn claim() -> Result<Claim> {
    Ok(Claim::new(
        &context(10, 20)?,
        MinimumAmount::try_from(12_345)?,
    ))
}

#[test]
fn claim_bytes_and_json_follow_the_documented_layout() -> Result<()> {
    let digest = context(10, 20)?.digest();
    let claim = claim()?;
    let first = header(Network::Regtest).block_hash();
    let last = header(Network::Bitcoin).block_hash();
    let mut expected = b"greenhold/claim/v1\0".to_vec();
    expected.extend(genesis_block(Network::Regtest).block_hash().to_byte_array());
    expected.extend(10u32.to_le_bytes());
    expected.extend(first.to_byte_array());
    expected.extend(20u32.to_le_bytes());
    expected.extend(last.to_byte_array());
    expected.extend(12_345u64.to_le_bytes());
    expected.extend(digest.to_byte_array());
    assert_eq!(claim.to_bytes(), expected);
    assert_eq!(Claim::try_from(expected.as_slice())?, claim);
    assert_eq!(
        serde_json::to_value(&claim)?,
        json!({
            "network": "regtest",
            "first": {"height": 10, "hash": first},
            "last": {"height": 20, "hash": last},
            "minimum_sats": 12_345,
            "context_digest": digest,
        })
    );

    // X = T is one block, named twice.
    let single = Claim::new(&context(20, 20)?, MinimumAmount::try_from(1)?);
    assert_eq!(Claim::try_from(single.to_bytes().as_slice())?, single);
    Ok(())
}

#[test]
fn claim_parsing_rejects_malformed_bytes() -> Result<()> {
    let bytes = claim()?.to_bytes();
    let changed = |offset: usize, value: &[u8]| {
        let mut changed = bytes.clone();
        changed[offset..offset + value.len()].copy_from_slice(value);
        changed
    };
    let mut trailing = bytes.clone();
    trailing.push(0);
    let testnet = genesis_block(Network::Testnet).block_hash().to_byte_array();
    let over_limit = (Amount::MAX_MONEY.to_sat() + 1).to_le_bytes();
    for (invalid, reason) in [
        (changed(NETWORK - 2, b"2"), "tag or version"),
        (bytes[..bytes.len() - 1].to_vec(), "truncated"),
        (trailing, "trailing bytes"),
        (changed(NETWORK, &testnet), "not a supported network"),
        (changed(FIRST, &21u32.to_le_bytes()), "must not exceed"),
        (changed(FIRST, &20u32.to_le_bytes()), "two blocks"),
        (changed(MINIMUM, &0u64.to_le_bytes()), "minimum amount"),
        (changed(MINIMUM, &over_limit), "minimum amount"),
    ] {
        let error = Claim::try_from(invalid.as_slice())
            .err()
            .context("malformed claim accepted")?;
        assert!(error.to_string().contains(reason), "{error:#}");
    }
    Ok(())
}
