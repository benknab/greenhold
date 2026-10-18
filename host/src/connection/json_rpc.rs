use anyhow::{Context, Result, anyhow, bail};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, IgnoredAny},
};
use serde_json::value::RawValue;
use std::{error::Error, fmt};

/// A server rejection, distinct from a malformed response. Remote details are omitted.
#[derive(Debug)]
pub(super) struct ServerError;

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
    error: Option<IgnoredAny>,
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
        if self.error.is_some() {
            return Err(ServerError.into());
        }
        let result = self.result.context("missing JSON-RPC result")?;
        serde_json::from_str(result.get()).map_err(|_| anyhow!("invalid JSON-RPC result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_null_missing_and_error_results() -> Result<()> {
        assert_eq!(
            Response::parse(br#"{"id":1,"result":null}"#)?.into_result::<Option<u64>>()?,
            None
        );
        assert_eq!(
            Response::parse(br#"{"id":1,"result":7,"error":null}"#)?.into_result::<u64>()?,
            7
        );
        for bytes in [
            br#"{"id":1}"#.as_slice(),
            br#"{"id":1,"error":null}"#,
            br#"{"id":null,"result":7}"#,
            br#"{"id":"1","result":7}"#,
            br#"{"result":7}"#,
            br#"{"id":1,"id":1,"result":7}"#,
        ] {
            assert!(Response::parse(bytes).is_err());
        }
        // A server error must not attempt to deserialize a success schema.
        for bytes in [
            br#"{"id":1,"error":{"code":-1,"message":"secret"}}"#.as_slice(),
            br#"{"id":1,"result":null,"error":{"code":-1,"message":"secret"}}"#,
        ] {
            let reply = Response::parse(bytes)?;
            let error = reply
                .into_result::<u64>()
                .err()
                .context("expected server error")?;
            assert_eq!(format!("{error:#}"), "node rejected request");
            assert!(error.is::<ServerError>());
        }
        Ok(())
    }

    #[test]
    fn schema_errors_do_not_leak_remote_values() -> Result<()> {
        let reply = Response::parse(br#"{"id":1,"result":"secret"}"#)?;
        let error = reply
            .into_result::<u64>()
            .err()
            .context("expected schema error")?;
        assert_eq!(format!("{error:#}"), "invalid JSON-RPC result");
        assert!(!error.is::<ServerError>());
        let error = Response::parse(br#"{"id":"secret","result":7}"#)
            .err()
            .context("expected envelope error")?;
        assert_eq!(format!("{error:#}"), "invalid JSON-RPC response");
        Ok(())
    }
}
