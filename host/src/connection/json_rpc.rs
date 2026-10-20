use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::{error::Error, fmt};

/// A server rejection, distinct from a malformed response. Remote details are omitted.
#[derive(Debug, Deserialize)]
pub(super) struct ServerError {
    pub code: i64,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("node rejected request")
    }
}

impl Error for ServerError {}

#[derive(Serialize)]
pub(super) struct Request<'a, P> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<&'a str>,
    pub id: u64,
    pub method: &'a str,
    pub params: P,
}

#[derive(Deserialize)]
pub(super) struct Response {
    pub id: u64,
    // Preserve explicit null results while rejecting a missing result on success.
    #[serde(default, deserialize_with = "present")]
    result: Option<Box<RawValue>>,
    error: Option<ServerError>,
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl Response {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // Serde errors can quote remote values. Keep those out of error chains.
        let response: Self =
            serde_json::from_slice(bytes).map_err(|_| anyhow!("invalid JSON-RPC response"))?;
        if response.result.is_none() && response.error.is_none() {
            bail!("missing JSON-RPC result or error");
        }
        Ok(response)
    }

    pub fn into_result<T: DeserializeOwned>(self) -> Result<T> {
        if let Some(error) = self.error {
            return Err(error.into());
        }
        let result = self.result.context("missing JSON-RPC result")?;
        serde_json::from_str(result.get()).map_err(|_| anyhow!("invalid JSON-RPC result"))
    }
}

#[cfg(test)]
mod tests;
