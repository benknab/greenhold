use super::{
    super::{
        MAX_REQUEST,
        socket::{SocketUrl, connect_tcp},
    },
    Transport,
    deadline::DeadlineStream,
};
use anyhow::{Context, Result, bail};
use bitcoin::{
    consensus::{deserialize, serialize},
    p2p::{
        Magic,
        message::{NetworkMessage, RawNetworkMessage},
    },
};
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

/// Mainnet v1 framing. An I/O or framing failure discards the connection.
#[derive(Debug)]
pub(super) struct Tcp {
    stream: Option<DeadlineStream>,
}

impl Tcp {
    /// The exchange budget starts after TCP connection setup.
    pub(super) fn connect(endpoint: &SocketUrl, timeout: Duration) -> Result<Self> {
        Ok(Self::new(connect_tcp(endpoint)?, timeout))
    }

    pub(super) fn new(socket: TcpStream, timeout: Duration) -> Self {
        Self {
            stream: Some(DeadlineStream::new(socket, timeout)),
        }
    }
}

impl Transport for Tcp {
    fn send(&mut self, message: NetworkMessage) -> Result<()> {
        let message = RawNetworkMessage::new(Magic::BITCOIN, message);
        let bytes = serialize(&message);
        if bytes.len() as u64 > MAX_REQUEST + 24 {
            bail!("P2P request exceeds size limit");
        }
        let mut stream = self.stream.take().context("P2P connection is closed")?;
        stream.write_all(&bytes).context("P2P write failed")?;
        stream.flush().context("P2P flush failed")?;
        self.stream = Some(stream);
        Ok(())
    }

    fn receive(&mut self) -> Result<NetworkMessage> {
        let mut stream = self.stream.take().context("P2P connection is closed")?;
        let message = read_message(&mut stream)?;
        self.stream = Some(stream);
        Ok(message)
    }
}

fn read_message(stream: &mut impl Read) -> Result<NetworkMessage> {
    let mut header = [0; 24];
    stream
        .read_exact(&mut header)
        .context("P2P header read failed")?;
    if header[..4] != Magic::BITCOIN.to_bytes() {
        bail!("wrong P2P network magic");
    }
    let size = u32::from_le_bytes(header[16..20].try_into()?);
    let mut bytes = header.to_vec();
    // Read only this frame and grow the buffer as its payload arrives.
    stream
        .take(u64::from(size))
        .read_to_end(&mut bytes)
        .context("P2P body read failed")?;
    if bytes.len() - header.len() != usize::try_from(size)? {
        bail!("P2P body is truncated");
    }
    let message: RawNetworkMessage =
        deserialize(&bytes).context("invalid P2P message or checksum")?;
    Ok(message.into_payload())
}

#[cfg(test)]
mod tests;
