use crate::support::{
    Node, WALLET_ADDRESS, command, image, rejection, success,
    transactions::{broadcast, mined, spend},
};
use anyhow::{Context, Result};
use bitcoin::{
    Address, Block, BlockHash, Network, OutPoint, ScriptBuf, consensus::deserialize, hex::FromHex,
};
use serde_json::{Value, json};
use std::fs;

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
struct DiscoveredCoin {
    outpoint: OutPoint,
    value_sats: u64,
    creation_height: u32,
    creation_hash: BlockHash,
}

fn discover(node: &Node, address: &Address) -> Result<Vec<DiscoveredCoin>> {
    let output = command(
        node,
        "regtest",
        "discover",
        &["--json", "--address", &address.to_string()],
    )?;
    let report = success(&output)?;
    Ok(serde_json::from_value(report)?)
}

fn discover_human(node: &Node, address: &Address) -> Result<Vec<DiscoveredCoin>> {
    let output = command(
        node,
        "regtest",
        "discover",
        &["--address", &address.to_string()],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines();
    assert_eq!(
        lines
            .next()
            .context("missing discovery header")?
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["OUTPOINT", "VALUE_SATS", "CREATION_HEIGHT", "CREATION_HASH"]
    );
    lines
        .map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 4);
            Ok(DiscoveredCoin {
                outpoint: fields[0].parse()?,
                value_sats: fields[1].parse()?,
                creation_height: fields[2].parse()?,
                creation_hash: fields[3].parse()?,
            })
        })
        .collect()
}

#[test]
#[ignore = "requires Docker; runs Utreexod nodes"]
fn discovery_lists_confirmed_coins_across_pages_and_prepares_a_claim() -> Result<()> {
    let image = image()?;
    let mining = Address::p2wsh(&ScriptBuf::from_bytes(vec![0x51]), Network::Regtest);
    let destination = WALLET_ADDRESS
        .parse::<Address<_>>()?
        .require_network(Network::Regtest)?;
    let node = Node::start(
        image.clone(),
        &[
            "--addrindex".into(),
            "--txindex".into(),
            "--utreexoproofindex".into(),
            "--utreexoproofindexmaxmemory=250".into(),
            format!("--miningaddr={mining}"),
        ],
        None,
    )?;
    assert!(discover(&node, &destination)?.is_empty());
    // A full 1,000-transaction page, followed by Utreexod's end-of-results RPC error.
    // The coin checks span ten batches.
    let blocks = node.rpc("generate", &json!([1_000]))?;
    let coins = discover(&node, &mining)?;
    assert_eq!(coins, mined_coins(&node, &blocks, &mining.script_pubkey())?);
    // Extend history beyond one page. SegWit is active and the funding coin is mature.
    node.rpc("generate", &json!([232]))?;
    let spent = mined(&node, 1, &mining.script_pubkey())?;
    let mut funding = spend(
        spent,
        &[2_000_000_000, 2_999_990_000],
        &destination.script_pubkey(),
    );
    // A third output belongs to another address and must not appear in discovery.
    funding.output[1].value = bitcoin::Amount::from_sat(2_999_980_000);
    funding.output.push(bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(10_000),
        script_pubkey: mining.script_pubkey(),
    });
    broadcast(&node, &funding)?;
    assert!(
        discover(&node, &destination)?.is_empty(),
        "mempool outputs are not confirmed coins"
    );
    assert!(
        discover(&node, &mining)?
            .iter()
            .any(|coin| coin.outpoint == spent),
        "mempool spends do not change confirmed state"
    );
    node.rpc("generate", &json!([1]))?;
    let creation_hash: BlockHash = node
        .rpc("getblockhash", &json!([1_233]))?
        .as_str()
        .context("creation block hash")?
        .parse()?;
    let outputs = discover(&node, &destination)?;
    assert_eq!(discover_human(&node, &destination)?, outputs);
    assert_eq!(
        outputs,
        [(0, 2_000_000_000), (1, 2_999_980_000)].map(|(vout, value_sats)| DiscoveredCoin {
            outpoint: OutPoint::new(funding.compute_txid(), vout),
            value_sats,
            creation_height: 1_233,
            creation_hash,
        })
    );
    let mining_coins = discover(&node, &mining)?;
    assert!(!mining_coins.iter().any(|coin| coin.outpoint == spent));
    assert!(mining_coins.contains(&DiscoveredCoin {
        outpoint: OutPoint::new(funding.compute_txid(), 2),
        value_sats: 10_000,
        creation_height: 1_233,
        creation_hash
    }));
    assert_eq!(mining_coins.len(), 1_233);
    prepare_discovered(&node, outputs.first().context("discovered coin")?.outpoint)?;
    // An unused address is an empty result, even when other addresses have history.
    let unused = Address::p2wsh(&ScriptBuf::from_bytes(vec![0x52]), Network::Regtest);
    assert!(discover(&node, &unused)?.is_empty());
    rejection(
        &command(
            &node,
            "signet",
            "discover",
            &[
                "--address",
                &Address::p2wsh(&ScriptBuf::from_bytes(vec![0x51]), Network::Signet).to_string(),
            ],
        )?,
        "not on the requested network",
    );
    let unindexed = Node::start(image, &["--noutreexo".into()], None)?;
    rejection(
        &command(
            &unindexed,
            "regtest",
            "discover",
            &["--address", &destination.to_string()],
        )?,
        "node needs --addrindex",
    );
    Ok(())
}

fn mined_coins(node: &Node, blocks: &Value, script: &ScriptBuf) -> Result<Vec<DiscoveredCoin>> {
    let mut coins = blocks
        .as_array()
        .context("generated blocks")?
        .iter()
        .enumerate()
        .map(|(index, hash)| {
            let raw = node.rpc("getblock", &json!([hash, 0]))?;
            let block: Block =
                deserialize(&Vec::<u8>::from_hex(raw.as_str().context("raw block")?)?)?;
            let coinbase = &block.txdata[0];
            let (vout, output) = coinbase
                .output
                .iter()
                .enumerate()
                .find(|(_, output)| output.script_pubkey == *script)
                .context("mining output")?;
            Ok(DiscoveredCoin {
                outpoint: OutPoint::new(coinbase.compute_txid(), u32::try_from(vout)?),
                value_sats: output.value.to_sat(),
                creation_height: u32::try_from(index + 1)?,
                creation_hash: block.block_hash(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    coins.sort_unstable_by_key(|coin| coin.outpoint);
    Ok(coins)
}

fn prepare_discovered(node: &Node, outpoint: OutPoint) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let witness_path = directory.path().join("coin.witness");
    let claim_path = directory.path().join("coin.claim");
    let witness = witness_path.to_str().context("witness path")?;
    let claim = claim_path.to_str().context("claim path")?;
    let report = success(&command(
        node,
        "regtest",
        "claim",
        &[
            "--json",
            "--output",
            claim,
            "--outpoint",
            &outpoint.to_string(),
            "--witness",
            witness,
            "--minimum-sats",
            "2000000000",
        ],
    )?)?;
    let bytes = fs::read(&claim_path)?;
    let parsed = greenhold_core::claim::Claim::try_from(bytes.as_slice())?;
    assert_eq!(serde_json::to_value(parsed)?, report["claim"]);
    assert!(report.get("encoded").is_none());
    assert_eq!(report["claim"]["minimum_sats"], 2_000_000_000_u64);
    Ok(())
}
