use super::*;
use std::{collections::VecDeque, net::TcpListener, thread};

#[test]
fn endpoint_requires_explicit_transport() -> Result<()> {
    let endpoint: Endpoint = "tcp://[::1]:8335".parse()?;
    assert_eq!(endpoint.transport, TransportKind::Tcp);
    let endpoint: Endpoint = "bip324://[::1]:8335".parse()?;
    assert_eq!(endpoint.transport, TransportKind::Bip324);
    for text in [
        "ssl://localhost:8335",
        "tls://localhost:8335",
        "https://localhost:8335",
    ] {
        assert!(text.parse::<Endpoint>().is_err(), "{text}");
    }
    Ok(())
}

#[test]
fn handshake_leaves_transport_ready_for_an_operation() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint: Endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
    let task = thread::spawn(move || -> Result<()> {
        let (socket, _) = crate::test_support::accept(&listener)?;
        let mut transport = tcp::Tcp::new(socket, super::super::TIMEOUT);
        let NetworkMessage::Version(version) = transport.receive()? else {
            bail!("expected version");
        };
        transport.send(NetworkMessage::Version(version))?;
        assert!(matches!(transport.receive()?, NetworkMessage::Verack));
        transport.send(NetworkMessage::Ping(42))?;
        assert!(matches!(transport.receive()?, NetworkMessage::Pong(42)));
        transport.send(NetworkMessage::Verack)?;
        assert!(matches!(transport.receive()?, NetworkMessage::Ping(7)));
        transport.send(NetworkMessage::Pong(7))
    });
    let mut transport = tcp::Tcp::connect(endpoint.socket_url(), super::super::TIMEOUT)?;
    assert!(handshake(&mut transport)?.version >= MIN_PROTOCOL_VERSION);
    transport.send(NetworkMessage::Ping(7))?;
    assert!(matches!(transport.receive()?, NetworkMessage::Pong(7)));
    task.join()
        .map_err(|_| anyhow::anyhow!("mock peer panicked"))??;
    Ok(())
}

struct ScriptedTransport {
    replies: VecDeque<NetworkMessage>,
    sent: Vec<NetworkMessage>,
}

impl Transport for ScriptedTransport {
    fn send(&mut self, message: NetworkMessage) -> Result<()> {
        self.sent.push(message);
        Ok(())
    }

    fn receive(&mut self) -> Result<NetworkMessage> {
        self.replies.pop_front().context("script exhausted")
    }
}

fn remote_version() -> NetworkMessage {
    let address = Address::new(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, 8335)),
        ServiceFlags::NONE,
    );
    NetworkMessage::Version(VersionMessage::new(
        ServiceFlags::NONE,
        0,
        address.clone(),
        address,
        42,
        "/test/".into(),
        123,
    ))
}

#[test]
fn handshake_is_independent_of_wire_framing() -> Result<()> {
    let mut transport = ScriptedTransport {
        replies: VecDeque::from([
            remote_version(),
            NetworkMessage::Ping(42),
            NetworkMessage::Verack,
        ]),
        sent: Vec::new(),
    };
    let version = handshake(&mut transport)?;
    assert_eq!(version.start_height, 123);
    assert_eq!(version.user_agent, "/test/");
    assert!(matches!(
        transport.sent.as_slice(),
        [
            NetworkMessage::Version(_),
            NetworkMessage::Verack,
            NetworkMessage::Pong(42)
        ]
    ));
    Ok(())
}

#[test]
fn rejects_invalid_handshakes_and_message_floods() -> Result<()> {
    let NetworkMessage::Version(mut obsolete) = remote_version() else {
        bail!("expected version");
    };
    obsolete.version = 70000;
    // Valid continuations prevent mock exhaustion from masquerading as rejection.
    for (replies, expected) in [
        (
            vec![
                NetworkMessage::Verack,
                remote_version(),
                NetworkMessage::Verack,
            ],
            "P2P verack before version",
        ),
        (
            vec![remote_version(), remote_version(), NetworkMessage::Verack],
            "duplicate P2P version",
        ),
        (
            vec![NetworkMessage::Version(obsolete), NetworkMessage::Verack],
            "unsupported P2P protocol version: 70000",
        ),
        (
            std::iter::repeat_n(NetworkMessage::Ping(42), 100)
                .chain([remote_version(), NetworkMessage::Verack])
                .collect(),
            "too many unrelated P2P messages",
        ),
    ] {
        let mut transport = ScriptedTransport {
            replies: replies.into(),
            sent: Vec::new(),
        };
        let error = handshake(&mut transport)
            .err()
            .context("invalid handshake was accepted")?;
        assert_eq!(error.to_string(), expected);
    }
    // Protocol minimum and the last permitted handshake message are accepted.
    let NetworkMessage::Version(mut minimum) = remote_version() else {
        bail!("expected version");
    };
    minimum.version = 70001;
    let mut transport = ScriptedTransport {
        replies: std::iter::repeat_n(NetworkMessage::Ping(42), 98)
            .chain([NetworkMessage::Version(minimum), NetworkMessage::Verack])
            .collect(),
        sent: Vec::new(),
    };
    assert_eq!(handshake(&mut transport)?.version, 70001);
    Ok(())
}
