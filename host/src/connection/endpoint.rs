use anyhow::{Context, Result, bail};
use std::str::FromStr;
use url::Url;

/// A host and explicit nonzero port, without credentials or request-specific URL parts.
#[derive(Clone, Debug)]
pub struct SocketUrl {
    url: Url,
}

impl SocketUrl {
    pub fn url(&self) -> &Url {
        &self.url
    }
}

impl FromStr for SocketUrl {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::try_from(Url::parse(text).context("invalid socket URL")?)
    }
}

impl TryFrom<Url> for SocketUrl {
    type Error = anyhow::Error;

    fn try_from(url: Url) -> Result<Self> {
        if url.host().is_none()
            || !matches!(url.port(), Some(1..))
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!(
                "endpoint must use SCHEME://HOST:PORT with a nonzero port and no credentials, path, query, or fragment"
            );
        }
        Ok(Self { url })
    }
}
