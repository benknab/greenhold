use bitcoin::block::Header;
use std::fmt;
use url::Url;

mod client;
mod responses;
pub use client::Client;
pub use responses::{BlockLocation, ChainTipProof, Roots, TxOutResponse};

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

/// The two response formats of getblockheader, with only the fields we use.
#[derive(Debug)]
pub enum BlockHeaderResponse {
    Raw(Header),
    Verbose(BlockLocation),
}

#[cfg(test)]
mod tests;
