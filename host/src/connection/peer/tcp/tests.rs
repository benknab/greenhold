use super::super::super::TIMEOUT;
use super::*;
use std::{io, net::TcpListener};

#[test]
fn reads_one_message_at_a_time() -> Result<()> {
    let payload = vec![7; 128 * 1024];
    let mut bytes = serialize(&RawNetworkMessage::new(
        Magic::BITCOIN,
        NetworkMessage::Unknown {
            command: "uproof".try_into()?,
            payload: payload.clone(),
        },
    ));
    bytes.extend(serialize(&RawNetworkMessage::new(
        Magic::BITCOIN,
        NetworkMessage::Verack,
    )));
    let mut input = bytes.as_slice();
    assert_eq!(
        read_message(&mut input)?,
        NetworkMessage::Unknown {
            command: "uproof".try_into()?,
            payload,
        }
    );
    assert!(matches!(read_message(&mut input)?, NetworkMessage::Verack));
    assert!(input.is_empty());
    Ok(())
}

#[test]
fn rejects_wrong_network_checksum_and_truncated_messages() {
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
    let mut transport = Tcp::new(client, TIMEOUT);
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
    let mut transport = Tcp::new(client, Duration::ZERO);
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
