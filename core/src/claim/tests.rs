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
fn context(unspent_since: u32, unspent_at: u32) -> Result<ChainContext> {
    let at_header = if unspent_since == unspent_at {
        header(Network::Regtest)
    } else {
        header(Network::Bitcoin)
    };
    ChainContext::new(
        BlockEndpoint::new(Network::Regtest, unspent_since, header(Network::Regtest))?,
        BlockEndpoint::new(Network::Regtest, unspent_at, at_header)?,
        AccumulatorState::new(3, vec![[1; 32], [2; 32]])?,
    )
}

fn claim() -> Result<Claim> {
    Ok(Claim::new(
        &context(10, 20)?,
        MinimumAmount::try_from(12_345)?,
        Nonce([0xab; 32]),
    ))
}

#[test]
fn claim_bytes_and_json_follow_the_documented_layout() -> Result<()> {
    let digest = context(10, 20)?.digest();
    let claim = claim()?;
    let unspent_since = header(Network::Regtest).block_hash();
    let unspent_at = header(Network::Bitcoin).block_hash();
    let mut expected = b"greenhold/claim/v2\0".to_vec();
    expected.extend(genesis_block(Network::Regtest).block_hash().to_byte_array());
    expected.extend(10u32.to_le_bytes());
    expected.extend(unspent_since.to_byte_array());
    expected.extend(20u32.to_le_bytes());
    expected.extend(unspent_at.to_byte_array());
    expected.extend(12_345u64.to_le_bytes());
    expected.extend(digest.to_byte_array());
    expected.extend([0xab; 32]);
    assert_eq!(claim.to_bytes(), expected);
    let parsed = Claim::try_from(expected.as_slice())?;
    assert_eq!(parsed, claim);
    // A verifier rebuilds the claim from the network, heights, and minimum it names.
    assert_eq!(Network::from(parsed.network()), Network::Regtest);
    let rebuilt = Claim::new(
        &context(parsed.unspent_since(), parsed.unspent_at())?,
        parsed.minimum_sats(),
        parsed.nonce(),
    );
    assert_eq!(rebuilt, parsed);
    assert_eq!(
        serde_json::to_value(&claim)?,
        json!({
            "network": "regtest",
            "unspent_since": {"height": 10, "hash": unspent_since},
            "unspent_at": {"height": 20, "hash": unspent_at},
            "minimum_sats": 12_345,
            "context_digest": digest,
            "nonce": "ab".repeat(32),
        })
    );

    // X = T is one block, named twice.
    let single = Claim::new(
        &context(20, 20)?,
        MinimumAmount::try_from(1)?,
        Nonce::default(),
    );
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
        (changed(NETWORK - 2, b"1"), "tag or version"),
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

#[test]
fn signing_message_matches_independent_vector_and_binds_every_field() -> Result<()> {
    // Python hashlib and OpenSSL vector: synthetic genesis state, encoding only.
    let endpoint = BlockEndpoint::new(Network::Bitcoin, 0, header(Network::Bitcoin))?;
    let context = ChainContext::new(
        endpoint.clone(),
        endpoint,
        AccumulatorState::new(3, vec![[0x11; 32], [0x22; 32]])?,
    )?;
    let claim = Claim::new(
        &context,
        MinimumAmount::try_from(12_345)?,
        Nonce([0xab; 32]),
    );
    assert_eq!(claim.to_bytes().len(), 195);
    assert_eq!(
        claim.signing_message(),
        "greenhold/claim:a2a1e054b839676de37c07893c6b0625ab170e5aeed5799762e21f26a2a06fed"
    );
    let original = self::claim()?;
    let bytes = original.to_bytes();
    for offset in [
        NETWORK,
        FIRST,
        FIRST + 4,
        LAST,
        LAST + 4,
        MINIMUM,
        MINIMUM + 8,
        MINIMUM + 8 + 32,
    ] {
        let mut changed = bytes.clone();
        if offset == NETWORK {
            changed[NETWORK..FIRST]
                .copy_from_slice(genesis_block(Network::Signet).block_hash().as_byte_array());
        } else {
            changed[offset] ^= 1;
        }
        let changed = Claim::try_from(changed.as_slice())?;
        assert_ne!(changed.signing_message(), original.signing_message());
    }
    let mut old_format = bytes;
    old_format.extend([0; 32]);
    assert!(Claim::try_from(old_format.as_slice()).is_err());
    Ok(())
}

#[test]
fn nonce_parsing_requires_exactly_32_hex_bytes() -> Result<()> {
    let nonce: Nonce = "AB".repeat(32).parse()?;
    assert_eq!(nonce.to_string(), "ab".repeat(32));
    for text in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "gg".repeat(32),
    ] {
        assert!(text.parse::<Nonce>().is_err());
        assert!(serde_json::from_value::<Nonce>(json!(text)).is_err());
    }
    Ok(())
}
