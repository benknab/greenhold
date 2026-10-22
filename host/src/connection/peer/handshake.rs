use super::bip324::Bip324;
use super::endpoint::{Endpoint, TransportKind};
use super::tcp::Tcp;
use super::transport::Transport;
use anyhow::{Context, Result, bail};
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Address, ServiceFlags};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_HANDSHAKE_MESSAGES: usize = 100;
// This client uses modern version messages and nonce-bearing ping/pong.
const MIN_PROTOCOL_VERSION: u32 = 70001;

/// Apply one caller-selected budget to transport negotiation and the Bitcoin handshake.
/// DNS and TCP connection setup happen before the budget starts.
pub fn probe(endpoint: &Endpoint, timeout: Duration) -> Result<VersionMessage> {
    match endpoint.transport() {
        TransportKind::Tcp => handshake(&mut Tcp::connect(endpoint.socket_url(), timeout)?),
        TransportKind::Bip324 => handshake(&mut Bip324::connect(endpoint.socket_url(), timeout)?),
    }
}

/// Leaves the transport open for the caller's operation after negotiation succeeds.
pub(super) fn handshake(transport: &mut impl Transport) -> Result<VersionMessage> {
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
