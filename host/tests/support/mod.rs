use anyhow::{Context, Result, bail};
use bitcoin::{
    BlockHash,
    hashes::{Hash, sha256},
    hex::FromHex,
};
use serde_json::{Value, json};
use std::{
    io::{Read as _, Seek as _},
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
use testcontainers::{
    Container, GenericBuildableImage, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::{SyncBuilder, SyncRunner},
};
use wait_timeout::ChildExt;

pub mod transactions;

pub const MINING_ADDRESS: &str = "bcrt1qusgerygumpd0ztn735s5pypq6wsv2zzhuc4yak";

/// The regtest test wallet: BIP-32 test vector 1 seed, P2WPKH key at m/84'/1'/0'/0/0.
/// `signer/` made the BIP-322 simple signatures over `MESSAGE` with this seed.
pub const WALLET_ADDRESS: &str = "bcrt1q7f0pjwhc3jzzv0w4uurm589506glv2dggfan4s";
pub const WALLET_PATH: &str = "m/84'/1'/0'/0/0";
pub const WALLET_SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const MESSAGE: &str = "Greenhold regtest";
pub const WALLET: [&str; 6] = [
    "--address",
    WALLET_ADDRESS,
    "--message",
    MESSAGE,
    "--signature",
    "smpAkcwRAIgNno2DwKezTtqqFm/cIfTY54R/h9/4aprHuQGCgjDIKECIG72IwNvtX40SYOmW1YmoqCdjM20PsNGmq/c0shpcuYXASECDH9N4cx2D8Bod1sVE9Z9CngC8LSxxhqoV4Tr9yKQWyc=",
];
/// The same seed's P2TR key at m/86'/1'/0'/0/0. It locks no coin in the scenarios.
pub const TAPROOT_WALLET: [&str; 6] = [
    "--address",
    "bcrt1pzh75rtx74l85v2xqfr5uln7mhy40vyqzm68ml4yngcqy9v085tqqpe5h98",
    "--message",
    MESSAGE,
    "--signature",
    "smpAUCmB//A6JQ5tpwr4N0kQd13rKN4NAesK39e/+2FK8nP/a/ALpQjOZwQmJ8Dewl//oZF/cMQYWIkBOAWwAla6XoW",
];

pub fn image() -> Result<GenericImage> {
    Ok(
        GenericBuildableImage::new("greenhold-utreexod-test", "0.6.0")
            .with_dockerfile_string(include_str!("Dockerfile"))
            .with_data(include_str!("entrypoint.sh"), "entrypoint.sh")
            .build_image()?
            .with_exposed_port(18334.tcp())
            .with_wait_for(WaitFor::message_on_stdout("RPC server listening")),
    )
}

#[derive(Debug)]
pub struct Node {
    pub container: Container<GenericImage>,
    pub rpc_url: String,
    agent: ureq::Agent,
}

impl Node {
    pub fn start(image: GenericImage, args: &[String], peer: Option<&Self>) -> Result<Self> {
        let mut request = image.with_cmd(args.iter().cloned());
        if let Some(peer) = peer {
            request = request.with_env_var(
                "REGTEST_PEER",
                peer.container.get_bridge_ip_address()?.to_string(),
            );
        }
        let container = request.start()?;
        let host = container.get_host()?;
        let rpc_url = format!(
            "http://{host}:{}/",
            container.get_host_port_ipv4(18334.tcp())?
        );
        let node = Self {
            container,
            rpc_url,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .build()
                .into(),
        };
        wait_for("node RPC", || {
            node.rpc("getblockcount", &json!([])).map(|_| true)
        })
        .with_context(|| {
            format!(
                "RPC {}\nstdout: {}\nstderr: {}",
                node.rpc_url,
                String::from_utf8_lossy(&node.container.stdout_to_vec().unwrap_or_default()),
                String::from_utf8_lossy(&node.container.stderr_to_vec().unwrap_or_default())
            )
        })?;
        Ok(node)
    }

    // Independent raw RPC client: never calls production adapters or loads node files.
    pub fn rpc(&self, method: &str, params: &Value) -> Result<Value> {
        let mut response = self
            .agent
            .post(&self.rpc_url)
            .header("Authorization", "Basic dGVzdDp0ZXN0")
            .header("Content-Type", "application/json")
            .send(serde_json::to_vec(
                &json!({"jsonrpc":"1.0","id":1,"method":method,"params":params}),
            )?)?;
        let reply: Value = serde_json::from_slice(&response.body_mut().read_to_vec()?)?;
        if !reply["error"].is_null() {
            bail!("test node {method}: {}", reply["error"]);
        }
        reply.get("result").cloned().context("missing RPC result")
    }

    pub fn wait_for_block(&self, bridge: &Self, height: u32) -> Result<()> {
        let expected = bridge.rpc("getblockhash", &json!([height]))?;
        wait_for("matching node block", || {
            Ok(self.rpc("getblockhash", &json!([height]))? == expected)
        })
        .with_context(|| {
            format!(
                "node sync to {height}\n{}",
                String::from_utf8_lossy(&self.container.stdout_to_vec().unwrap_or_default())
            )
        })
    }
}

pub fn wait_for(description: &str, mut ready: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_mins(1);
    loop {
        let result = ready();
        if matches!(result, Ok(true)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for {description}: {result:?}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Independent encoding of the public context commitment from raw node data.
/// Do not call `ChainContext::digest` or the production RPC client as the oracle.
pub fn context_digest(node: &Node, first: u32, last: u32) -> Result<String> {
    let first_hash = node.rpc("getblockhash", &json!([first]))?;
    let last_hash = node.rpc("getblockhash", &json!([last]))?;
    let roots = node.rpc("getutreexoroots", &json!([last_hash]))?;
    let leaves = roots["numleaves"].as_u64().context("missing leaf count")?;
    assert!(leaves > 0, "the scenario must have a nonempty accumulator");
    let roots: Vec<Vec<u8>> = roots["roots"]
        .as_array()
        .context("missing roots")?
        .iter()
        .map(|root| Ok(Vec::from_hex(root.as_str().context("root is not hex")?)?))
        .collect::<Result<_>>()?;
    let genesis = node.rpc("getblockhash", &json!([0]))?;
    let genesis: BlockHash = genesis.as_str().context("missing genesis hash")?.parse()?;
    let mut preimage = b"greenhold/chain-context/v1\0".to_vec();
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
    Ok(sha256::Hash::hash(&preimage).to_string())
}

/// Run `inspect`, which takes no network or node flags.
pub fn inspect(receipt: &str) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args(["inspect", receipt])
        .output()?)
}

pub fn command(rpc: &Node, network: &str, command: &str, args: &[&str]) -> Result<Output> {
    command_with_timeout(rpc, network, command, args, Duration::from_secs(30))
}

pub fn command_with_timeout(
    rpc: &Node,
    network: &str,
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output> {
    // Temporary files prevent long discovery lists from filling a pipe while we wait.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_greenhold-host"))
        .args([
            command,
            "--network",
            network,
            "--utreexod-rpc",
            &rpc.rpc_url,
            "--utreexod-rpc-user",
            "test",
            "--utreexod-rpc-password",
            "test",
        ])
        .args(args)
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    let Some(status) = child.wait_timeout(timeout)? else {
        child.kill()?;
        child.wait()?;
        bail!("{command} command timed out");
    };
    stdout.rewind()?;
    stderr.rewind()?;
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.read_to_end(&mut output.stdout)?;
    stderr.read_to_end(&mut output.stderr)?;
    Ok(output)
}

pub fn success(output: &Output) -> Result<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn rejection(output: &Output, message: &str) {
    assert!(!output.status.success(), "invalid context was accepted");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
