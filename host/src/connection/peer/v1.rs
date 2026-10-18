use super::{
    super::{
        MAX_MESSAGE, TIMEOUT,
        socket::{SocketUrl, connect_tcp},
    },
    Transport,
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
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

/// Mainnet v1 framing. An I/O or framing failure discards the connection.
#[derive(Debug)]
pub(super) struct V1 {
    stream: Option<DeadlineStream>,
}

impl V1 {
    pub(super) fn connect(endpoint: &SocketUrl) -> Result<Self> {
        Ok(Self::new(connect_tcp(endpoint)?, TIMEOUT))
    }

    pub(super) fn new(socket: TcpStream, timeout: Duration) -> Self {
        Self {
            stream: Some(DeadlineStream {
                socket,
                deadline: Instant::now() + timeout,
            }),
        }
    }
}

impl Transport for V1 {
    fn send(&mut self, message: NetworkMessage) -> Result<()> {
        let message = RawNetworkMessage::new(Magic::BITCOIN, message);
        let bytes = serialize(&message);
        if bytes.len() as u64 > MAX_MESSAGE + 24 {
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

/// One deadline for the entire exchange, including partial reads and writes.
/// TCP connection setup and DNS resolution happen before this deadline starts.
#[derive(Debug)]
struct DeadlineStream {
    socket: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "P2P operation deadline exceeded")
            })
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.flush()
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
    if u64::from(size) > MAX_MESSAGE {
        bail!("P2P response exceeds size limit");
    }
    let mut bytes = header.to_vec();
    bytes.resize(24 + usize::try_from(size)?, 0);
    stream
        .read_exact(&mut bytes[24..])
        .context("P2P body read failed")?;
    let message: RawNetworkMessage =
        deserialize(&bytes).context("invalid P2P message or checksum")?;
    Ok(message.into_payload())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn reads_one_message_at_a_time() -> Result<()> {
        let mut bytes = serialize(&RawNetworkMessage::new(
            Magic::BITCOIN,
            NetworkMessage::Ping(42),
        ));
        bytes.extend(serialize(&RawNetworkMessage::new(
            Magic::BITCOIN,
            NetworkMessage::Verack,
        )));
        let mut input = bytes.as_slice();
        assert!(matches!(
            read_message(&mut input)?,
            NetworkMessage::Ping(42)
        ));
        assert!(matches!(read_message(&mut input)?, NetworkMessage::Verack));
        assert!(input.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_wrong_network_checksum_and_oversized_messages() {
        let wrong_network = serialize(&RawNetworkMessage::new(
            Magic::TESTNET3,
            NetworkMessage::Verack,
        ));
        assert!(read_message(&mut wrong_network.as_slice()).is_err());
        let valid = serialize(&RawNetworkMessage::new(
            Magic::BITCOIN,
            NetworkMessage::Ping(42),
        ));
        assert!(read_message(&mut &valid[..23]).is_err());
        assert!(read_message(&mut &valid[..valid.len() - 1]).is_err());
        let mut invalid = valid;
        invalid[20] ^= 1;
        assert!(read_message(&mut invalid.as_slice()).is_err());
        invalid[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_message(&mut invalid.as_slice()).is_err());
    }

    fn pair() -> Result<(TcpStream, TcpStream)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let client = TcpStream::connect(listener.local_addr()?)?;
        Ok((client, listener.accept()?.0))
    }

    #[test]
    fn protocol_error_discards_transport() -> Result<()> {
        let (client, mut server) = pair()?;
        server.write_all(&serialize(&RawNetworkMessage::new(
            Magic::TESTNET3,
            NetworkMessage::Verack,
        )))?;
        let mut transport = V1::new(client, TIMEOUT);
        assert!(transport.receive().is_err());
        assert!(transport.stream.is_none());
        assert!(transport.send(NetworkMessage::Ping(42)).is_err());
        Ok(())
    }

    #[test]
    fn expired_deadline_discards_transport_even_with_ready_data() -> Result<()> {
        let (client, mut server) = pair()?;
        server.write_all(&serialize(&RawNetworkMessage::new(
            Magic::BITCOIN,
            NetworkMessage::Verack,
        )))?;
        let mut transport = V1::new(client, Duration::ZERO);
        let error = transport
            .receive()
            .err()
            .context("expected deadline error")?;
        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .context("expected I/O error")?
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(transport.stream.is_none());
        Ok(())
    }
}
