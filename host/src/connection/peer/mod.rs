mod bip324;
mod deadline;
mod tcp;

use super::socket::SocketUrl;
use anyhow::{Context, Result, bail};
use bitcoin::p2p::{
    Address, ServiceFlags, message::NetworkMessage, message_network::VersionMessage,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

const MAX_HANDSHAKE_MESSAGES: usize = 100;
// This client uses modern version messages and nonce-bearing ping/pong.
const MIN_PROTOCOL_VERSION: u32 = 70001;

/// Message boundary shared by plaintext v1 and encrypted v2 transports.
/// Implementations own framing, deadlines, and failure cleanup.
trait Transport {
    fn send(&mut self, message: NetworkMessage) -> Result<()>;
    fn receive(&mut self) -> Result<NetworkMessage>;
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    url: SocketUrl,
    transport: TransportKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportKind {
    Tcp,
    Bip324,
}

impl Endpoint {
    pub fn socket_url(&self) -> &SocketUrl {
        &self.url
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

/// Apply one caller-selected budget to transport negotiation and the Bitcoin handshake.
/// DNS and TCP connection setup happen before the budget starts.
pub fn probe(endpoint: &Endpoint, timeout: Duration) -> Result<VersionMessage> {
    match endpoint.transport {
        TransportKind::Tcp => handshake(&mut tcp::Tcp::connect(endpoint.socket_url(), timeout)?),
        TransportKind::Bip324 => handshake(&mut bip324::Bip324::connect(
            endpoint.socket_url(),
            timeout,
        )?),
    }
}

/// Leaves the transport open for the caller's operation after negotiation succeeds.
fn handshake(transport: &mut impl Transport) -> Result<VersionMessage> {
    let address = Address::new(
        &SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        ServiceFlags::NONE,
    );
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let version = VersionMessage::new(
        ServiceFlags::NONE,
        i64::try_from(now.as_secs())?,
        address.clone(),
        address,
        u64::try_from(now.as_nanos() & u128::from(u64::MAX))?,
        concat!("/greenhold:", env!("CARGO_PKG_VERSION"), "/").into(),
        0,
    );
    transport.send(NetworkMessage::Version(version))?;
    let mut remote = None;
    for _ in 0..MAX_HANDSHAKE_MESSAGES {
        let message = transport.receive()?;
        match handle_control(transport, message)? {
            Some(NetworkMessage::Version(version)) => {
                if remote.is_some() {
                    bail!("duplicate P2P version");
                }
                if version.version < MIN_PROTOCOL_VERSION {
                    bail!("unsupported P2P protocol version: {}", version.version);
                }
                remote = Some(version);
                transport.send(NetworkMessage::Verack)?;
            }
            Some(NetworkMessage::Verack) => {
                return remote.context("P2P verack before version");
            }
            _ => {}
        }
    }
    bail!("too many unrelated P2P messages")
}

/// Answer routine traffic; leave operation-specific messages for the caller.
fn handle_control(
    transport: &mut impl Transport,
    message: NetworkMessage,
) -> Result<Option<NetworkMessage>> {
    match message {
        NetworkMessage::Ping(nonce) => {
            transport.send(NetworkMessage::Pong(nonce))?;
            Ok(None)
        }
        NetworkMessage::Reject(_) => bail!("P2P request rejected"),
        message => Ok(Some(message)),
    }
}

#[cfg(test)]
mod tests;
