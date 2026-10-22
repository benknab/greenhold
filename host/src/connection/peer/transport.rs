use anyhow::Result;
use bitcoin::p2p::message::NetworkMessage;

/// Message boundary shared by plaintext v1 and encrypted v2 transports.
/// Implementations own framing, deadlines, and failure cleanup.
pub(super) trait Transport {
    fn send(&mut self, message: NetworkMessage) -> Result<()>;
    fn receive(&mut self) -> Result<NetworkMessage>;
}
