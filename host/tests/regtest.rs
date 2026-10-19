mod support;

use anyhow::{Context, Result};
use bitcoin::{
    BlockHash,
    hashes::{Hash, sha256},
    hex::FromHex,
};
use serde_json::{Value, json};
use support::{MINING_ADDRESS, Node, context, image, rejection, success, wait_for};

// Run explicitly with `cargo regtest`. Missing Docker is a failure, never a skip.
#[test]
#[ignore = "requires Docker; runs stock Utreexod nodes"]
fn historical_context_uses_stock_nodes_and_fails_closed() -> Result<()> {
    let image = image()?;
    let bridge = Node::start(
        image.clone(),
        &[
            "--utreexoproofindex".into(),
            "--utreexoproofindexmaxmemory=250".into(),
            format!("--miningaddr={MINING_ADDRESS}"),
        ],
        None,
    )?;
    bridge.rpc("generate", &json!([8]))?;
    // A bridge cannot also host watch-only Electrum. Use a separate validating node.
    let chain = Node::start(
        image.clone(),
        &[
            "--watchonlywallet".into(),
            "--electrumlisteners=0.0.0.0:50001".into(),
            "--tlselectrumlisteners=127.0.0.1:50002".into(),
        ],
        Some(&bridge),
    )?;
    chain.wait_for_block(&bridge, 8)?;
    let historical_hash = bridge.rpc("getblockhash", &json!([6]))?;
    let historical_header = bridge.rpc("getblockheader", &json!([historical_hash, false]))?;
    wait_for("Electrum historical header", || {
        Ok(chain.electrum("blockchain.block.header", &json!([6]))? == historical_header)
    })?;

    let historical = success(&context(
        &bridge,
        &chain,
        "regtest",
        &["6", "--first-height", "3"],
    )?)?;
    assert_context(&bridge, &historical, 3, 6)?;
    let single = success(&context(&bridge, &chain, "regtest", &["6"])?)?;
    assert_context(&bridge, &single, 6, 6)?;
    bridge.rpc("generate", &json!([3]))?;
    chain.wait_for_block(&bridge, 11)?;
    assert_eq!(
        success(&context(
            &bridge,
            &chain,
            "regtest",
            &["6", "--first-height", "3"]
        )?)?,
        historical
    );
    let tip = success(&context(&bridge, &chain, "regtest", &[])?)?;
    assert_context(&bridge, &tip, 11, 11)?;
    assert_ne!(historical["context"]["state"], tip["context"]["state"]);

    rejection(
        &context(&bridge, &chain, "bitcoin", &["6"])?,
        "not on the requested network",
    );
    // Same chain, but deliberately no accumulator index. The application must fail.
    let unindexed = Node::start(image, &["--noutreexo".into()], Some(&bridge))?;
    unindexed.wait_for_block(&bridge, 11)?;
    rejection(
        &context(&unindexed, &chain, "regtest", &["6"])?,
        "cannot retrieve Utreexo state",
    );
    Ok(())
}

fn assert_context(node: &Node, report: &Value, first: u32, last: u32) -> Result<()> {
    assert_eq!(report["network"], "regtest");
    let first_hash = node.rpc("getblockhash", &json!([first]))?;
    let last_hash = node.rpc("getblockhash", &json!([last]))?;
    assert_eq!(report["first_block_hash"], first_hash);
    assert_eq!(report["last_block_hash"], last_hash);
    assert_eq!(report["context"]["first"]["height"], first);
    assert_eq!(report["context"]["last"]["height"], last);
    let roots = node.rpc("getutreexoroots", &json!([last_hash]))?;
    let leaves = roots["numleaves"].as_u64().context("missing leaf count")?;
    assert!(
        leaves > 0,
        "historical scenario must have a nonempty accumulator"
    );
    assert_eq!(report["context"]["state"]["num_leaves"], leaves);
    let roots: Vec<Vec<u8>> = roots["roots"]
        .as_array()
        .context("missing roots")?
        .iter()
        .map(|root| {
            Ok(Vec::<u8>::from_hex(
                root.as_str().context("root is not hex")?,
            )?)
        })
        .collect::<Result<_>>()?;
    assert_eq!(report["context"]["state"]["roots"], json!(roots));

    // Independent encoding of the public commitment rules from raw stock-node data.
    // Do not call ChainContext::digest or the production RPC client as the oracle.
    let genesis = node.rpc("getblockhash", &json!([0]))?;
    let genesis: BlockHash = genesis.as_str().context("missing genesis hash")?.parse()?;
    let mut preimage = b"walledcity/chain-context/v1\0".to_vec();
    preimage.extend(genesis.to_byte_array());
    for (height, hash) in [(first, first_hash), (last, last_hash)] {
        let header = node.rpc("getblockheader", &json!([hash, false]))?;
        preimage.extend(height.to_le_bytes());
        preimage.extend(Vec::<u8>::from_hex(
            header.as_str().context("missing header")?,
        )?);
    }
    preimage.extend(leaves.to_le_bytes());
    preimage.extend(u32::try_from(roots.len())?.to_le_bytes());
    for root in roots {
        preimage.extend(root);
    }
    assert_eq!(
        report["context_digest"],
        sha256::Hash::hash(&preimage).to_string()
    );
    Ok(())
}
