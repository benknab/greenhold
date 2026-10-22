use super::super::super::TIMEOUT;
use super::*;
use crate::{connection::peer, test_support};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpListener},
    thread,
};

type Remote = Session;

fn serve(
    action: impl FnOnce(Remote) -> Result<()> + Send + 'static,
) -> Result<(peer::Endpoint, thread::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("bip324://{}", listener.local_addr()?).parse()?;
    let task = thread::spawn(move || {
        let (socket, _) = test_support::accept(&listener)?;
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
    let mut transport = Bip324::connect(endpoint.socket_url(), TIMEOUT)?;
    assert_eq!(peer::handshake(&mut transport)?.start_height, 123);
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
    assert_eq!(peer::probe(&endpoint, TIMEOUT)?.start_height, 123);
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
        assert_eq!(decode_message(&encode_message(&message)?)?, message);
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
        let mut transport = Bip324::connect(endpoint.socket_url(), TIMEOUT)?;
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
    let mut transport = Bip324::connect(endpoint.socket_url(), TIMEOUT)?;
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
    let (server, _) = test_support::accept(&listener)?;
    let error = Bip324::try_new(client, Duration::ZERO)
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
        let (mut server, _) = test_support::accept(&listener)?;
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
    assert!(peer::probe(&endpoint, TIMEOUT).is_err());
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
