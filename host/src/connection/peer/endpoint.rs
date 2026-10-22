use crate::connection::socket::SocketUrl;
use anyhow::{Context, Result, bail};
use std::str::FromStr;
use url::Url;

#[derive(Clone, Debug)]
pub struct Endpoint {
    url: SocketUrl,
    transport: TransportKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransportKind {
    Tcp,
    Bip324,
}

impl Endpoint {
    pub fn socket_url(&self) -> &SocketUrl {
        &self.url
    }

    pub(super) fn transport(&self) -> TransportKind {
        self.transport
    }
}

impl FromStr for Endpoint {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::try_from(Url::parse(text).context("invalid P2P URL")?)
    }
}

impl TryFrom<Url> for Endpoint {
    type Error = anyhow::Error;

    fn try_from(url: Url) -> Result<Self> {
        let url = SocketUrl::try_from(url)?;
        let transport = match url.url().scheme() {
            "tcp" => TransportKind::Tcp,
            "bip324" => TransportKind::Bip324,
            _ => bail!("P2P endpoint must use tcp:// or bip324://"),
        };
        Ok(Self { url, transport })
    }
}

#[cfg(test)]
mod tests;
