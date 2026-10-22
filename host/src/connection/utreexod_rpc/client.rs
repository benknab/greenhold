use super::{
    AddressTransaction, BlockHeaderResponse, BlockLocation, ChainTipProof, Config, HistoryOrder,
    InputDetails, Roots, SearchOptions, TxOutResponse,
};
use crate::connection::{
    TIMEOUT,
    header::HeaderResponse,
    json_rpc::{Request, Response, ServerError},
};
use anyhow::{Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitcoin::{BlockHash, OutPoint, Txid};
use serde::{Serialize, de::DeserializeOwned};
use std::{io::Read, time::Duration};
use ureq::{Agent, http::HeaderValue};

const BULK_TIMEOUT: Duration = Duration::from_mins(1);

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

    /// Check outpoints in one HTTP request. Return results in the input order.
    /// Missing, duplicate, and unexpected response IDs invalidate the whole batch.
    pub fn tx_outs(
        &mut self,
        outpoints: &[OutPoint],
        include_mempool: bool,
    ) -> Result<Vec<Option<TxOutResponse>>> {
        if outpoints.is_empty() {
            return Ok(Vec::new());
        }
        let first_id = self.id + 1;
        let requests: Vec<_> = outpoints
            .iter()
            .map(|outpoint| {
                self.id += 1;
                Request {
                    jsonrpc: Some("2.0"),
                    id: self.id,
                    method: "gettxout",
                    params: (outpoint.txid, outpoint.vout, include_mempool),
                }
            })
            .collect();
        let bytes = self.send(serde_json::to_vec(&requests)?, BULK_TIMEOUT)?;
        let mut replies = Response::parse_batch(&bytes)?;
        replies.sort_unstable_by_key(|reply| reply.id);
        if replies.len() != outpoints.len()
            || replies
                .iter()
                .zip(first_id..=self.id)
                .any(|(reply, id)| reply.id != id)
        {
            bail!("JSON-RPC batch response IDs do not match requests");
        }
        replies.into_iter().map(Response::into_result).collect()
    }

    /// Decoded address history with caller-selected input details and ordering.
    pub fn search_raw_transactions(
        &mut self,
        address: &str,
        options: SearchOptions,
    ) -> Result<Vec<AddressTransaction>> {
        match self.call_with_timeout(
            "searchrawtransactions",
            (
                address,
                1,
                options.skip,
                options.count,
                u8::from(options.input_details == InputDetails::Include),
                options.order == HistoryOrder::NewestFirst,
            ),
            BULK_TIMEOUT,
        ) {
            // Utreexod uses ErrRPCNoTxInfo for unused addresses and exhausted pages.
            Err(error)
                if error
                    .downcast_ref::<ServerError>()
                    .is_some_and(|e| e.code == -5) =>
            {
                Ok(Vec::new())
            }
            result => result,
        }
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
        self.call_with_timeout(method, params, TIMEOUT)
    }

    fn call_with_timeout<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<T> {
        self.id += 1;
        let body = serde_json::to_vec(&Request {
            jsonrpc: Some("1.0"),
            id: self.id,
            method,
            params,
        })?;
        let bytes = self.send(body, timeout)?;
        // Remote bodies and transport errors may contain secrets. Do not print them.
        let reply = Response::parse(&bytes)?;
        if reply.id != self.id {
            bail!("JSON-RPC response ID does not match request");
        }
        reply.into_result()
    }

    fn send(&self, body: Vec<u8>, timeout: Duration) -> Result<Vec<u8>> {
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
            .config()
            .timeout_global(Some(timeout))
            .build()
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
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow!("RPC response read failed"))?;
        Ok(bytes)
    }
}
