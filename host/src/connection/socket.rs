use super::TIMEOUT;
use anyhow::{Context, Result, bail};
use std::{net::TcpStream, str::FromStr};
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

pub(super) fn connect_tcp(endpoint: &SocketUrl) -> Result<TcpStream> {
    let addresses = endpoint
        .url()
        .socket_addrs(|| None)
        .context("node DNS lookup failed")?;
    for address in addresses {
        if let Ok(socket) = TcpStream::connect_timeout(&address, TIMEOUT) {
            socket.set_read_timeout(Some(TIMEOUT))?;
            socket.set_write_timeout(Some(TIMEOUT))?;
            return Ok(socket);
        }
    }
    bail!("node connection failed")
}

#[cfg(test)]
mod tests;
