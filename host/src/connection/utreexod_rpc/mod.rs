use std::fmt;
use url::Url;

mod client;
mod credentials;
mod responses;
mod search_options;
pub use client::Client;
pub use credentials::Credentials;
pub use responses::{AddressTransaction, BlockLocation, ChainTipProof, Roots, TxOutResponse};
pub use search_options::{HistoryOrder, InputDetails, SearchOptions};

pub struct Config {
    url: Url,
    credentials: Option<Credentials>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("authenticated", &self.credentials.is_some())
            .finish_non_exhaustive()
    }
}

impl Config {
    pub fn new(url: Url, credentials: Option<Credentials>) -> Self {
        Self { url, credentials }
    }
}

#[cfg(test)]
mod tests;
