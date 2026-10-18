use super::{
    MAX_MESSAGE, TIMEOUT,
    json_rpc::{Request, Response},
};
use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitcoin::{BlockHash, hex::FromHex};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use std::{fmt, io::Read};
use ureq::{Agent, http::HeaderValue};
use url::Url;

pub struct Config {
    url: Url,
    credentials: Option<(String, String)>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("authenticated", &self.credentials.is_some())
            .finish_non_exhaustive()
    }
}

impl Config {
    pub fn new(url: Url, credentials: Option<(String, String)>) -> Self {
        Self { url, credentials }
    }
}

/// Parsed accumulator state reported by the node, not independently verified chain state.
#[derive(Debug, Deserialize)]
#[serde(try_from = "RootsResponse")]
pub struct Roots {
    num_leaves: u64,
    roots: Vec<[u8; 32]>,
}

impl Roots {
    pub fn num_leaves(&self) -> u64 {
        self.num_leaves
    }

    /// Root bytes in the node's order, with no block-hash byte reversal.
    pub fn roots(&self) -> &[[u8; 32]] {
        &self.roots
    }
}

#[derive(Deserialize)]
struct RootsResponse {
    #[serde(rename = "numleaves")]
    num_leaves: u64,
    #[serde(deserialize_with = "deserialize_roots")]
    roots: Vec<[u8; 32]>,
}

fn deserialize_roots<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<[u8; 32]>, D::Error> {
    // Go encodes a nil slice as null for an empty accumulator.
    let roots = Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default();
    roots
        .iter()
        .map(|text| {
            <[u8; 32]>::from_hex(text).map_err(|_| serde::de::Error::custom("invalid root hash"))
        })
        .collect()
}

impl TryFrom<RootsResponse> for Roots {
    type Error = anyhow::Error;

    fn try_from(response: RootsResponse) -> Result<Self> {
        let RootsResponse { num_leaves, roots } = response;
        // The forest has one root for each set bit in the leaf count.
        if roots.len() != usize::try_from(num_leaves.count_ones())? {
            bail!("RPC root count does not match leaf count");
        }
        Ok(Self { num_leaves, roots })
    }
}

/// Utreexod's HTTP JSON-RPC client.
pub struct Client<'a> {
    config: &'a Config,
    agent: Agent,
    id: u64,
}

impl<'a> Client<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            agent: Agent::config_builder()
                .timeout_global(Some(TIMEOUT))
                .max_redirects(0)
                .http_status_as_error(false)
                .build()
                .into(),
            id: 0,
        }
    }

    pub fn best_block_hash(&mut self) -> Result<BlockHash> {
        self.call("getbestblockhash", [] as [(); 0])
    }

    /// Request accumulator state at this exact block, rather than a moving tip.
    pub fn utreexo_roots(&mut self, block: &BlockHash) -> Result<Roots> {
        self.call("getutreexoroots", [block])
    }

    fn call<T: DeserializeOwned>(&mut self, method: &str, params: impl Serialize) -> Result<T> {
        self.id += 1;
        let body = serde_json::to_vec(&Request {
            jsonrpc: Some("1.0"),
            id: self.id,
            method,
            params,
        })?;
        let mut request = self
            .agent
            .post(self.config.url.as_str())
            .header("Content-Type", "application/json");
        if let Some((user, password)) = &self.config.credentials {
            let encoded = STANDARD.encode(format!("{user}:{password}"));
            let mut authorization = HeaderValue::from_str(&format!("Basic {encoded}"))
                .map_err(|_| anyhow!("RPC authentication header setup failed"))?;
            authorization.set_sensitive(true);
            request = request.header("Authorization", authorization);
        }
        let mut response = request
            .send(body)
            .map_err(|_| anyhow!("RPC transport failed"))?;
        if !response.status().is_success() {
            bail!(
                "RPC HTTP {}; check authentication and node availability",
                response.status().as_u16()
            );
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_MESSAGE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow!("RPC response read failed"))?;
        if bytes.len() as u64 > MAX_MESSAGE {
            bail!("RPC response exceeds size limit");
        }
        // Remote bodies and transport errors may contain secrets. Do not print them.
        let reply = Response::parse(&bytes)?;
        if reply.id != self.id {
            bail!("JSON-RPC response ID does not match request");
        }
        reply.into_result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    const BLOCK: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn read_headers(stream: &mut impl BufRead) -> Result<String> {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            if stream.read_line(&mut line)? == 0 {
                bail!("request ended before headers");
            }
            if line == "\r\n" {
                return Ok(headers);
            }
            headers.push_str(&line.to_ascii_lowercase());
        }
    }

    fn serve(
        status: &str,
        body: String,
        auth: bool,
    ) -> Result<(Url, thread::JoinHandle<Result<()>>)> {
        serve_responses(
            status,
            vec![(
                json!({"jsonrpc":"1.0","id":1,"method":"getbestblockhash","params":[]}),
                body,
            )],
            auth,
        )
    }

    fn serve_responses(
        status: &str,
        responses: Vec<(Value, String)>,
        auth: bool,
    ) -> Result<(Url, thread::JoinHandle<Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
        let status = status.to_owned();
        let task = thread::spawn(move || {
            for (expected, body) in responses {
                let (socket, _) = listener.accept()?;
                socket.set_read_timeout(Some(TIMEOUT))?;
                let mut stream = BufReader::new(socket);
                let headers = read_headers(&mut stream)?;
                assert!(headers.starts_with("post / http/1.1\r\n"));
                assert_eq!(headers.contains("authorization:"), auth);
                assert!(!auth || headers.contains("authorization: basic dxnlcjpwyxnz\r\n"));
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .context("missing request length")?
                    .parse::<usize>()?;
                let mut request = vec![0; length];
                stream.read_exact(&mut request)?;
                let request: Value = serde_json::from_slice(&request)?;
                assert_eq!(request, expected);
                write!(
                    stream.get_mut(),
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nLocation: /redirected\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
            }
            Ok(())
        });
        Ok((url, task))
    }

    #[test]
    fn anonymous_and_authenticated_requests() -> Result<()> {
        for auth in [false, true] {
            let (url, task) = serve(
                "200 OK",
                json!({"id":1,"result":BLOCK,"error":null}).to_string(),
                auth,
            )?;
            let credentials = auth.then(|| ("user".into(), "pass".into()));
            let config = Config::new(url, credentials);
            assert_eq!(Client::new(&config).best_block_hash()?.to_string(), BLOCK);
            task.join().map_err(|_| anyhow!("mock server panicked"))??;
        }
        Ok(())
    }

    #[test]
    fn debug_redacts_credentials() -> Result<()> {
        let config = Config::new(
            Url::parse("http://localhost/")?,
            Some(("secret-user".into(), "secret-password".into())),
        );
        assert!(!format!("{config:?}").contains("secret"));
        Ok(())
    }

    #[test]
    fn rejects_redirects_errors_bad_ids_and_oversized_bodies() -> Result<()> {
        for (status, body) in [
            ("302 Found", "secret".into()),
            ("401 Unauthorized", "secret".into()),
            ("200 OK", json!({"id":2,"result":BLOCK}).to_string()),
            (
                "200 OK",
                json!({"id":1,"error":{"message":"secret"}}).to_string(),
            ),
            ("200 OK", "secret".into()),
            ("200 OK", " ".repeat(usize::try_from(MAX_MESSAGE)? + 1)),
        ] {
            let (url, task) = serve(status, body, false)?;
            let config = Config::new(url, None);
            let response = Client::new(&config).best_block_hash();
            assert!(response.is_err());
            assert!(!format!("{response:?}").contains("secret"));
            if status != "200 OK" {
                let error = response.err().context("expected HTTP status error")?;
                assert_eq!(
                    error.to_string(),
                    format!(
                        "RPC HTTP {}; check authentication and node availability",
                        &status[..3]
                    )
                );
            }
            task.join().map_err(|_| anyhow!("mock server panicked"))??;
        }
        Ok(())
    }

    #[test]
    fn roots_lookup_uses_parsed_block_and_preserves_root_bytes() -> Result<()> {
        let responses = vec![
            (json!({"jsonrpc":"1.0","id":1,"method":"getbestblockhash","params":[]}),
             json!({"id":1,"result":BLOCK,"error":null}).to_string()),
            (json!({"jsonrpc":"1.0","id":2,"method":"getutreexoroots","params":[BLOCK]}),
             json!({"id":2,"result":{"numleaves":3,"roots":[BLOCK,"00".repeat(32)]},"error":null}).to_string()),
        ];
        let (url, task) = serve_responses("200 OK", responses, false)?;
        let config = Config::new(url, None);
        let mut client = Client::new(&config);
        let block = client.best_block_hash()?;
        let roots = client.utreexo_roots(&block)?;
        assert_eq!(roots.num_leaves(), 3);
        let mut expected = [0; 32];
        for (byte, value) in expected.iter_mut().zip(0_u8..32) {
            *byte = value;
        }
        assert_eq!(roots.roots(), &[expected, [0; 32]]);
        task.join().map_err(|_| anyhow!("mock server panicked"))??;
        Ok(())
    }

    #[test]
    fn rejects_malformed_block_hash_responses() -> Result<()> {
        for value in [
            Value::Null,
            json!(7),
            json!({"hash":BLOCK}),
            json!("secret"),
            json!("ab".repeat(31)),
            json!("ab".repeat(33)),
            json!("zz".repeat(32)),
        ] {
            let (url, task) = serve(
                "200 OK",
                json!({"id":1,"result":value,"error":null}).to_string(),
                false,
            )?;
            let config = Config::new(url, None);
            let error = Client::new(&config)
                .best_block_hash()
                .err()
                .context("expected invalid block hash")?;
            assert_eq!(format!("{error:#}"), "invalid JSON-RPC result");
            task.join().map_err(|_| anyhow!("mock server panicked"))??;
        }
        Ok(())
    }

    #[test]
    fn rejects_malformed_roots_responses() -> Result<()> {
        let block = BLOCK.parse()?;
        for value in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"numleaves":0}),
            json!({"roots":[]}),
            json!({"numleaves":-1,"roots":[]}),
            json!({"numleaves":1.5,"roots":[BLOCK]}),
            json!({"numleaves":"1","roots":[BLOCK]}),
            json!({"numleaves":18_446_744_073_709_551_616_u128,"roots":[]}),
            json!({"numleaves":1,"roots":null}),
            json!({"numleaves":1,"roots":BLOCK}),
            json!({"numleaves":0,"roots":[BLOCK]}),
            json!({"numleaves":3,"roots":[BLOCK]}),
            json!({"numleaves":1,"roots":[false]}),
            json!({"numleaves":1,"roots":["secret"]}),
            json!({"numleaves":1,"roots":["ab".repeat(31)]}),
            json!({"numleaves":1,"roots":["ab".repeat(33)]}),
            json!({"numleaves":1,"roots":["zz".repeat(32)]}),
        ] {
            let (url, task) = serve_responses(
                "200 OK",
                vec![(
                    json!({"jsonrpc":"1.0","id":1,"method":"getutreexoroots","params":[BLOCK]}),
                    json!({"id":1,"result":value,"error":null}).to_string(),
                )],
                false,
            )?;
            let config = Config::new(url, None);
            let error = Client::new(&config)
                .utreexo_roots(&block)
                .err()
                .context("expected invalid roots")?;
            assert!(!format!("{error:#}").contains("secret"));
            task.join().map_err(|_| anyhow!("mock server panicked"))??;
        }
        Ok(())
    }

    #[test]
    fn accepts_empty_and_large_leaf_counts() -> Result<()> {
        for roots in [json!([]), Value::Null] {
            let state: Roots = serde_json::from_value(json!({"numleaves":0,"roots":roots}))?;
            assert_eq!(state.num_leaves(), 0);
            assert!(state.roots().is_empty());
        }
        for num_leaves in [1_u64 << 63, u64::MAX] {
            let state: Roots = serde_json::from_value(json!({
                "numleaves":num_leaves,
                "roots":vec![BLOCK; usize::try_from(num_leaves.count_ones())?],
            }))?;
            assert_eq!(state.num_leaves(), num_leaves);
        }
        Ok(())
    }
}
