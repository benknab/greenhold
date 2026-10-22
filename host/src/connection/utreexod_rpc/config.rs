use super::Credentials;
use std::fmt;
use url::Url;

pub struct Config {
    pub(super) url: Url,
    pub(super) credentials: Option<Credentials>,
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
