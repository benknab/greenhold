use super::{MAX_MESSAGE, connect};
use anyhow::{Context, Result, bail};
use bitcoin::{
    consensus::{deserialize, serialize},
    p2p::{
        Address, Magic, ServiceFlags,
        message::{NetworkMessage, RawNetworkMessage},
        message_network::VersionMessage,
    },
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

pub fn handshake(endpoint: &Url) -> Result<Value> {
    if endpoint.scheme() != "tcp" {
        bail!("unsupported P2P transport");
    }
    exchange(&mut connect(endpoint)?)
}

fn exchange(stream: &mut (impl Read + Write)) -> Result<Value> {
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
        "/walledcity:0.1/".into(),
        0,
    );
    send(stream, NetworkMessage::Version(version))?;
    let mut remote = None;
    for _ in 0..100 {
        match receive(stream)? {
            NetworkMessage::Version(version) if remote.is_none() => {
                remote = Some(
                    json!({"protocol_version":version.version, "start_height":version.start_height}),
                );
                send(stream, NetworkMessage::Verack)?;
            }
            NetworkMessage::Verack => {
                return remote.context("P2P verack before version");
            }
            NetworkMessage::Ping(nonce) => send(stream, NetworkMessage::Pong(nonce))?,
            NetworkMessage::Reject(_) => bail!("P2P handshake rejected"),
            _ => {}
        }
    }
    bail!("too many unrelated P2P messages")
}

fn send(stream: &mut impl Write, message: NetworkMessage) -> Result<()> {
    stream
        .write_all(&serialize(&RawNetworkMessage::new(Magic::BITCOIN, message)))
        .context("P2P write failed")?;
    stream.flush().context("P2P flush failed")?;
    Ok(())
}

fn receive(stream: &mut impl Read) -> Result<NetworkMessage> {
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
    use std::{net::TcpListener, thread};

    #[test]
    fn completes_mainnet_handshake_and_answers_ping() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = Url::parse(&format!("tcp://{}", listener.local_addr()?))?;
        let task = thread::spawn(move || -> Result<()> {
            let (mut socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(super::super::TIMEOUT))?;
            let NetworkMessage::Version(version) = receive(&mut socket)? else {
                bail!("expected version");
            };
            send(&mut socket, NetworkMessage::Version(version))?;
            assert!(matches!(receive(&mut socket)?, NetworkMessage::Verack));
            send(&mut socket, NetworkMessage::Ping(42))?;
            assert!(matches!(receive(&mut socket)?, NetworkMessage::Pong(42)));
            send(&mut socket, NetworkMessage::Verack)
        });
        assert!(handshake(&endpoint)?["protocol_version"].as_u64().is_some());
        task.join()
            .map_err(|_| anyhow::anyhow!("mock peer panicked"))??;
        Ok(())
    }

    #[test]
    fn rejects_wrong_network_checksum_and_oversized_messages() {
        let wrong_network = serialize(&RawNetworkMessage::new(
            Magic::TESTNET3,
            NetworkMessage::Verack,
        ));
        assert!(receive(&mut wrong_network.as_slice()).is_err());
        let mut invalid = serialize(&RawNetworkMessage::new(
            Magic::BITCOIN,
            NetworkMessage::Verack,
        ));
        invalid[20] ^= 1;
        assert!(receive(&mut invalid.as_slice()).is_err());
        invalid[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(receive(&mut invalid.as_slice()).is_err());
        assert!(receive(&mut &invalid[..23]).is_err());
    }
}
