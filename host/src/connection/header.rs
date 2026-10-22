use anyhow::Result;
use bitcoin::{block::Header, consensus, hex::FromHex};
use serde::Deserialize;

/// An exact consensus header encoded as RPC hex. Decode inside the RPC result
/// parser so malformed remote values never appear in error chains.
#[derive(Deserialize)]
#[serde(try_from = "String")]
pub(super) struct HeaderResponse(pub Header);

impl TryFrom<String> for HeaderResponse {
    type Error = anyhow::Error;

    fn try_from(text: String) -> Result<Self> {
        let bytes = <[u8; 80]>::from_hex(&text)?;
        Ok(Self(consensus::deserialize(&bytes)?))
    }
}
