use crate::support::{
    MINING_ADDRESS, Node, command, image, rejection, success,
    transactions::{broadcast, mined, spend},
};
use anyhow::{Context, Result};
use bitcoin::{Address, Network, OutPoint, ScriptBuf};
use serde_json::json;

fn discover(node: &Node, address: &Address) -> Result<Vec<OutPoint>> {
    let output = command(node, "regtest", "discover", &[&address.to_string()])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| Ok(line.parse()?))
        .collect()
}

#[test]
#[ignore = "requires Docker; runs Utreexod nodes"]
fn discovery_lists_confirmed_coins_across_pages_and_feeds_the_prover() -> Result<()> {
    let image = image()?;
    let mining = Address::p2wsh(&ScriptBuf::from_bytes(vec![0x51]), Network::Regtest);
    let destination = MINING_ADDRESS
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
    node.rpc("generate", &json!([1_000]))?;
    let coins = discover(&node, &mining)?;
    let mut expected = (1..=1_000)
        .map(|height| mined(&node, height, &mining.script_pubkey()))
        .collect::<Result<Vec<_>>>()?;
    expected.sort_unstable();
    assert_eq!(coins, expected);
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
        discover(&node, &mining)?.contains(&spent),
        "mempool spends do not change confirmed state"
    );
    node.rpc("generate", &json!([1]))?;
    let outputs = discover(&node, &destination)?;
    assert_eq!(
        outputs,
        [
            OutPoint::new(funding.compute_txid(), 0),
            OutPoint::new(funding.compute_txid(), 1)
        ]
    );
    let mining_coins = discover(&node, &mining)?;
    assert!(!mining_coins.contains(&spent));
    assert!(mining_coins.contains(&OutPoint::new(funding.compute_txid(), 2)));
    assert_eq!(mining_coins.len(), 1_233);
    let report = success(&command(
        &node,
        "regtest",
        "prove",
        &[
            &outputs.first().context("discovered coin")?.to_string(),
            "--dry-run",
            "--minimum-sats",
            "2000000000",
        ],
    )?)?;
    assert_eq!(report["claim"]["minimum_sats"], 2_000_000_000_u64);
    // An unused address is an empty result, even when other addresses have history.
    let unused = Address::p2wsh(&ScriptBuf::from_bytes(vec![0x52]), Network::Regtest);
    assert!(discover(&node, &unused)?.is_empty());
    rejection(
        &command(
            &node,
            "signet",
            "discover",
            &[&Address::p2wsh(&ScriptBuf::from_bytes(vec![0x51]), Network::Signet).to_string()],
        )?,
        "not on the requested network",
    );
    let unindexed = Node::start(image, &["--noutreexo".into()], None)?;
    rejection(
        &command(
            &unindexed,
            "regtest",
            "discover",
            &[&destination.to_string()],
        )?,
        "node needs --addrindex",
    );
    Ok(())
}
