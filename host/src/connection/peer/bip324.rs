use super::{
    super::{
        TIMEOUT,
        socket::{SocketUrl, connect_tcp},
    },
    Transport,
    deadline::DeadlineStream,
};
use anyhow::{Context, Result, bail};
use bip324::{
    PacketType, Role,
    io::{Payload, Protocol},
};
use bitcoin::{
    consensus::{deserialize, serialize},
    hashes::{Hash, sha256d},
    p2p::{
        Magic,
        message::{NetworkMessage, RawNetworkMessage},
    },
};
use std::{fmt, net::TcpStream, time::Duration};

// BIP324 message IDs, followed by the assignments in its message_type_ids.md.
const SHORT_COMMANDS: [&str; 38] = [
    "",
    "addr",
    "block",
    "blocktxn",
    "cmpctblock",
    "feefilter",
    "filteradd",
    "filterclear",
    "filterload",
    "getblocks",
    "getblocktxn",
    "getdata",
    "getheaders",
    "headers",
    "inv",
    "mempool",
    "merkleblock",
    "notfound",
    "ping",
    "pong",
    "sendcmpct",
    "tx",
    "getcfilters",
    "cfilter",
    "getcfheaders",
    "cfheaders",
    "getcfcheckpt",
    "cfcheckpt",
    "addrv2",
    "uproof",
    "getuproof",
    "uttls",
    "getuttls",
    "usummary",
    "utreexotx",
    "uroot",
    "geturoot",
    "feature",
];

pub(super) struct Bip324 {
    session: Option<Session>,
}

// Cipher state is deliberately excluded from Debug output.
impl fmt::Debug for Bip324 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bip324")
            .field("connected", &self.session.is_some())
            .finish_non_exhaustive()
    }
}

type Session = Protocol<DeadlineStream, DeadlineStream>;

impl Bip324 {
    pub(super) fn connect(endpoint: &SocketUrl) -> Result<Self> {
        Self::new(connect_tcp(endpoint)?, TIMEOUT)
    }

    fn new(socket: TcpStream, timeout: Duration) -> Result<Self> {
        let (reader, writer) = DeadlineStream::pair(socket, timeout)?;
        let session = Protocol::new(
            Magic::BITCOIN.to_bytes(),
            Role::Initiator,
            None,
            None,
            reader,
            writer,
        )
        .context("BIP324 handshake failed")?;
        Ok(Self {
            session: Some(session),
        })
    }
}

impl Transport for Bip324 {
    fn send(&mut self, message: NetworkMessage) -> Result<()> {
        let contents = encode_message(&message)?;
        let mut session = self.session.take().context("P2P connection is closed")?;
        session
            .write(&Payload::genuine(contents))
            .context("BIP324 write failed")?;
        self.session = Some(session);
        Ok(())
    }

    fn receive(&mut self) -> Result<NetworkMessage> {
        let mut session = self.session.take().context("P2P connection is closed")?;
        loop {
            let payload = session.read().context("BIP324 read failed")?;
            if payload.packet_type() == PacketType::Decoy {
                continue;
            }
            let message = decode_message(payload.contents())?;
            self.session = Some(session);
            return Ok(message);
        }
    }
}

fn encode_message(message: &NetworkMessage) -> Result<Vec<u8>> {
    let payload = serialize(message);
    let command = message.command();
    let mut contents = match SHORT_COMMANDS
        .iter()
        .position(|name| *name == command.as_ref())
    {
        Some(id) if id != 0 => vec![u8::try_from(id)?],
        _ => {
            let mut prefix = vec![0];
            prefix.extend(serialize(&command));
            prefix
        }
    };
    contents.extend(payload);
    Ok(contents)
}

fn decode_message(contents: &[u8]) -> Result<NetworkMessage> {
    let (&id, rest) = contents
        .split_first()
        .context("missing BIP324 message type")?;
    let (command, payload) = if id == 0 {
        let command = rest.get(..12).context("truncated BIP324 command")?;
        let end = command
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(command.len());
        if end == 0
            || !command[..end]
                .iter()
                .all(|byte| (0x20..=0x7e).contains(byte))
            || command[end..].iter().any(|byte| *byte != 0)
        {
            bail!("invalid BIP324 command");
        }
        (command.to_vec(), &rest[12..])
    } else {
        let name = SHORT_COMMANDS
            .get(usize::from(id))
            .context("unknown BIP324 message type")?;
        let mut command = name.as_bytes().to_vec();
        command.resize(12, 0);
        (command, rest)
    };
    // bitcoin 0.32 exposes command-based decoding through RawNetworkMessage.
    // Build an in-memory v1 envelope to reuse it; no v1 bytes go over the wire.
    let mut encoded = Magic::BITCOIN.to_bytes().to_vec();
    encoded.extend(command);
    encoded.extend(u32::try_from(payload.len())?.to_le_bytes());
    encoded.extend(&sha256d::Hash::hash(payload).to_byte_array()[..4]);
    encoded.extend(payload);
    let message: RawNetworkMessage =
        deserialize(&encoded).context("invalid BIP324 message payload")?;
    Ok(message.into_payload())
}

#[cfg(test)]
mod tests {
    use super::super::{P2pEndpoint, handshake, probe};
    use super::*;
    use std::{
        io::{self, Read, Write},
        net::{Shutdown, TcpListener},
        thread,
    };

    type Remote = Session;

    fn serve(
        action: impl FnOnce(Remote) -> Result<()> + Send + 'static,
    ) -> Result<(P2pEndpoint, thread::JoinHandle<Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("bip324://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || {
            let (socket, _) = listener.accept()?;
            let (reader, writer) = DeadlineStream::pair(socket, TIMEOUT)?;
            // Exercise maximum handshake garbage, authenticated as AAD on the first decoy.
            let protocol = Protocol::new(
                Magic::BITCOIN.to_bytes(),
                Role::Responder,
                Some(vec![0x55; 4095]),
                Some(vec![vec![1, 2, 3]; 101]),
                reader,
                writer,
            )?;
            action(protocol)
        });
        Ok((endpoint, task))
    }

    fn finish_bitcoin_handshake(remote: &mut Remote) -> Result<()> {
        let payload = remote.read()?;
        assert!(payload.contents().starts_with(b"\0version\0\0\0\0\0"));
        let NetworkMessage::Version(mut version) = decode_message(payload.contents())? else {
            bail!("expected version");
        };
        version.start_height = 123;
        version.nonce ^= 1;
        remote.write(&Payload::genuine(encode_message(
            &NetworkMessage::Version(version),
        )?))?;
        assert_eq!(remote.read()?.contents(), b"\0verack\0\0\0\0\0\0");
        remote.write(&Payload::decoy(vec![9, 8, 7]))?;
        let mut request = vec![18];
        request.extend(42_u64.to_le_bytes());
        remote.write(&Payload::genuine(request))?;
        let mut response = vec![19];
        response.extend(42_u64.to_le_bytes());
        assert_eq!(remote.read()?.contents(), response);
        remote.write(&Payload::genuine(b"\0verack\0\0\0\0\0\0".to_vec()))?;
        Ok(())
    }

    fn reply_to_pings(mut remote: Remote) -> Result<()> {
        finish_bitcoin_handshake(&mut remote)?;
        for nonce in 0_u64..230 {
            let mut request = vec![18];
            request.extend(nonce.to_le_bytes());
            assert_eq!(remote.read()?.contents(), request);
            // Both compact and long command encodings must be accepted.
            let mut response = if nonce % 2 == 0 {
                vec![19]
            } else {
                b"\0pong\0\0\0\0\0\0\0\0".to_vec()
            };
            response.extend(nonce.to_le_bytes());
            remote.write(&Payload::genuine(response))?;
        }
        Ok(())
    }

    #[test]
    fn encrypted_handshake_and_reuse_cross_rekey_boundary() -> Result<()> {
        let (endpoint, task) = serve(reply_to_pings)?;
        let mut transport = Bip324::connect(endpoint.socket_url())?;
        assert_eq!(handshake(&mut transport)?.start_height, 123);
        for nonce in 0..230 {
            transport.send(NetworkMessage::Ping(nonce))?;
            assert_eq!(transport.receive()?, NetworkMessage::Pong(nonce));
        }
        task.join()
            .map_err(|_| anyhow::anyhow!("mock v2 peer panicked"))??;
        Ok(())
    }

    #[test]
    fn probe_selects_v2_and_closes_after_handshake() -> Result<()> {
        let (endpoint, task) = serve(|mut remote| {
            finish_bitcoin_handshake(&mut remote)?;
            assert!(remote.read().is_err());
            Ok(())
        })?;
        assert_eq!(probe(&endpoint)?.start_height, 123);
        task.join()
            .map_err(|_| anyhow::anyhow!("mock v2 peer panicked"))??;
        Ok(())
    }

    #[test]
    fn decoding_covers_short_long_and_extension_commands() -> Result<()> {
        for message in [
            NetworkMessage::Ping(42),
            NetworkMessage::Verack,
            NetworkMessage::Unknown {
                command: "uroot".try_into()?,
                payload: vec![1, 2, 3],
            },
            NetworkMessage::Unknown {
                command: "custom".try_into()?,
                payload: vec![4, 5],
            },
        ] {
            let contents = encode_message(&message)?;
            assert_eq!(decode_message(&contents)?, message);
        }
        assert_eq!(
            encode_message(&NetworkMessage::Unknown {
                command: "uroot".try_into()?,
                payload: vec![]
            })?,
            [35]
        );
        for contents in [
            b"".as_slice(),
            &[255],
            b"\0ping",
            b"\0ping\0BAD\0\0\0\0",
            &[18, 1, 2],
            b"\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ] {
            assert!(decode_message(contents).is_err(), "{contents:?}");
        }
        let mut extra = vec![18];
        extra.extend([0; 9]);
        assert!(decode_message(&extra).is_err());
        Ok(())
    }

    fn send_bad_packet(remote: Remote, case: u8) -> Result<()> {
        let (_reader, writer) = remote.into_split();
        let (mut cipher, mut writer) = writer.into_inner();
        let contents = if case == 2 {
            vec![255]
        } else {
            b"\0verack\0\0\0\0\0\0".to_vec()
        };
        let mut packet = cipher.encrypt_to_vec(&contents, PacketType::Genuine, None);
        match case {
            0 => *packet.last_mut().context("missing authentication tag")? ^= 1,
            1 => packet.truncate(packet.len() - 1),
            _ => {}
        }
        writer.write_all(&packet)?;
        Ok(())
    }

    #[test]
    fn rejects_tampered_truncated_and_invalid_packets() -> Result<()> {
        for case in 0..3 {
            let (endpoint, task) = serve(move |remote| send_bad_packet(remote, case))?;
            let mut transport = Bip324::connect(endpoint.socket_url())?;
            assert!(transport.receive().is_err());
            assert!(transport.session.is_none());
            assert!(transport.send(NetworkMessage::Ping(42)).is_err());
            task.join()
                .map_err(|_| anyhow::anyhow!("mock v2 peer panicked"))??;
        }
        Ok(())
    }

    #[test]
    fn accepts_large_messages_and_more_than_one_hundred_decoys() -> Result<()> {
        let message = NetworkMessage::Unknown {
            command: "custom".try_into()?,
            payload: vec![7; 128 * 1024],
        };
        let expected = encode_message(&message)?;
        let (endpoint, task) = serve(move |mut remote| {
            assert_eq!(remote.read()?.contents(), expected);
            for _ in 0..101 {
                remote.write(&Payload::decoy(Vec::new()))?;
            }
            remote.write(&Payload::genuine(expected))?;
            Ok(())
        })?;
        let mut transport = Bip324::connect(endpoint.socket_url())?;
        transport.send(message.clone())?;
        assert_eq!(transport.receive()?, message);
        task.join()
            .map_err(|_| anyhow::anyhow!("mock v2 peer panicked"))??;
        Ok(())
    }

    #[test]
    fn handshake_deadline_and_v1_failure_do_not_downgrade() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let client = TcpStream::connect(listener.local_addr()?)?;
        let (server, _) = listener.accept()?;
        let error = Bip324::new(client, Duration::ZERO)
            .err()
            .context("expected deadline error")?;
        assert_eq!(
            error
                .chain()
                .find_map(|cause| cause.downcast_ref::<io::Error>())
                .context("expected I/O error")?
                .kind(),
            io::ErrorKind::TimedOut
        );
        drop(server);

        let endpoint = format!("bip324://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || -> Result<TcpListener> {
            let (mut server, _) = listener.accept()?;
            server.set_read_timeout(Some(TIMEOUT))?;
            let mut key = [0; 64];
            server.read_exact(&mut key)?;
            let mut v1 = Magic::BITCOIN.to_bytes().to_vec();
            v1.extend(b"version\0\0\0\0\0");
            v1.resize(64, 0);
            server.write_all(&v1)?;
            server.shutdown(Shutdown::Both)?;
            listener.set_nonblocking(true)?;
            Ok(listener)
        });
        assert!(probe(&endpoint).is_err());
        let listener = task
            .join()
            .map_err(|_| anyhow::anyhow!("mock v1 peer panicked"))??;
        assert_eq!(
            listener
                .accept()
                .err()
                .context("unexpected v1 retry")?
                .kind(),
            io::ErrorKind::WouldBlock
        );
        Ok(())
    }
}
