use super::{
    MAX_MESSAGE, TIMEOUT,
    header::HeaderResponse,
    json_rpc::{Request, Response},
};
use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitcoin::{BlockHash, block::Header, hex::FromHex};
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

/// Hash and height from one RPC best-chain snapshot.
#[derive(Debug, Deserialize)]
pub struct BlockLocation {
    hash: BlockHash,
    height: u32,
}

impl BlockLocation {
    pub fn hash(&self) -> BlockHash {
        self.hash
    }

    pub fn height(&self) -> u32 {
        self.height
    }
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

    pub fn best_block(&mut self) -> Result<BlockLocation> {
        self.call("getbestblock", [] as [(); 0])
    }

    pub fn block_hash(&mut self, height: u32) -> Result<BlockHash> {
        self.call("getblockhash", [height])
    }

    pub fn block_header(&mut self, block: &BlockHash) -> Result<Header> {
        let HeaderResponse(header) = self.call("getblockheader", (block, false))?;
        if header.block_hash() != *block {
            bail!("RPC header does not match requested block hash");
        }
        Ok(header)
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
mod tests;
