use super::{BlockHeaderResponse, BlockLocation, ChainTipProof, Config, Roots, TxOutResponse};
use crate::connection::{
    MAX_MESSAGE, TIMEOUT,
    header::HeaderResponse,
    json_rpc::{Request, Response},
};
use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitcoin::{BlockHash, Txid};
use serde::{Serialize, de::DeserializeOwned};
use std::io::Read;
use ureq::{Agent, http::HeaderValue};

/// HTTP JSON-RPC client with one method per supported Utreexod endpoint.
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

    pub fn best_block(&mut self) -> Result<BlockLocation> {
        self.call("getbestblock", [] as [(); 0])
    }

    pub fn block_hash(&mut self, height: u32) -> Result<BlockHash> {
        self.call("getblockhash", [height])
    }

    pub fn block_header(
        &mut self,
        block: &BlockHash,
        verbose: bool,
    ) -> Result<BlockHeaderResponse> {
        if verbose {
            self.call("getblockheader", (block, true))
                .map(BlockHeaderResponse::Verbose)
        } else {
            let HeaderResponse(header) = self.call("getblockheader", (block, false))?;
            Ok(BlockHeaderResponse::Raw(header))
        }
    }

    pub fn utreexo_roots(&mut self, block: &BlockHash) -> Result<Roots> {
        self.call("getutreexoroots", [block])
    }

    pub fn tx_out(
        &mut self,
        txid: &Txid,
        vout: u32,
        include_mempool: bool,
    ) -> Result<Option<TxOutResponse>> {
        self.call("gettxout", (txid, vout, include_mempool))
    }

    /// Supports the structured proof response (RPC verbosity 1).
    pub fn prove_utxo_chain_tip_inclusion(
        &mut self,
        txids: &[Txid],
        vouts: &[u32],
    ) -> Result<ChainTipProof> {
        self.call("proveutxochaintipinclusion", (txids, vouts, 1))
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
