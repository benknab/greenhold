use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use testcontainers::{
    Container, GenericBuildableImage, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::{SyncBuilder, SyncRunner},
};
use wait_timeout::ChildExt;

pub const MINING_ADDRESS: &str = "bcrt1qusgerygumpd0ztn735s5pypq6wsv2zzhuc4yak";

pub fn image() -> Result<GenericImage> {
    Ok(
        GenericBuildableImage::new("walledcity-utreexod-test", "0.6.0")
            .with_dockerfile_string(include_str!("Dockerfile"))
            .with_data(include_str!("entrypoint.sh"), "entrypoint.sh")
            .build_image()?
            .with_exposed_port(18334.tcp())
            .with_exposed_port(50001.tcp())
            .with_wait_for(WaitFor::message_on_stdout("RPC server listening")),
    )
}

#[derive(Debug)]
pub struct Node {
    pub container: Container<GenericImage>,
    pub rpc_url: String,
    pub electrum_address: String,
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
        let electrum_address = format!("{host}:{}", container.get_host_port_ipv4(50001.tcp())?);
        let node = Self {
            container,
            rpc_url,
            electrum_address,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(3)))
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

    pub fn electrum(&self, method: &str, params: &Value) -> Result<Value> {
        let socket = TcpStream::connect(&self.electrum_address)?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        writeln!(
            &socket,
            "{}",
            json!({"id":1,"method":method,"params":params})
        )?;
        let mut line = String::new();
        BufReader::new(socket).read_line(&mut line)?;
        let reply: Value = serde_json::from_str(&line)?;
        if !reply["error"].is_null() {
            bail!("test Electrum {method}: {}", reply["error"]);
        }
        reply
            .get("result")
            .cloned()
            .context("missing Electrum result")
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

pub fn context(rpc: &Node, electrum: &Node, network: &str, heights: &[&str]) -> Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_walledcity-host"))
        .args([
            "context",
            "--network",
            network,
            "--utreexod-rpc",
            &rpc.rpc_url,
            "--utreexod-rpc-user",
            "test",
            "--utreexod-rpc-password",
            "test",
            "--electrum",
            &format!("tcp://{}", electrum.electrum_address),
        ])
        .args(heights)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if child.wait_timeout(Duration::from_secs(30))?.is_none() {
        child.kill()?;
        child.wait()?;
        bail!("context command timed out");
    }
    Ok(child.wait_with_output()?)
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
