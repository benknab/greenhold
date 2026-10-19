use super::*;
use crate::test_support::accept;
use serde_json::{Value, json};
use std::{net::TcpListener, thread};

#[test]
fn validated_transports_and_tls_names() -> Result<()> {
    let tcp: Endpoint = "tcp://localhost:50001".parse()?;
    assert!(matches!(tcp.transport, Transport::Tcp));
    for scheme in ["tls", "ssl"] {
        let endpoint: Endpoint = format!("{scheme}://example.com:50002").parse()?;
        assert!(matches!(
            endpoint.transport,
            Transport::Tls(ServerName::DnsName(_))
        ));
        for host in ["127.0.0.1", "[::1]"] {
            let endpoint: Endpoint = format!("{scheme}://{host}:50002").parse()?;
            assert!(matches!(
                endpoint.transport,
                Transport::Tls(ServerName::IpAddress(_))
            ));
        }
    }
    assert!("tls://bad%20host:50002".parse::<Endpoint>().is_err());
    assert!(Endpoint::try_from(Url::parse("https://localhost:50002")?).is_err());
    Ok(())
}

#[test]
fn reuses_connection_for_different_requests_and_server_errors() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
    let task = thread::spawn(move || -> Result<()> {
        let (socket, _) = accept(&listener)?;
        let mut stream = BufReader::new(socket);
        for (id, method, params, response) in [
            (
                1,
                "server.version",
                json!([
                    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
                    "1.4"
                ]),
                json!({"result":["test-server", "1.4"]}),
            ),
            (
                2,
                "unknown.method",
                json!(["argument", 7]),
                json!({"error":{"code":-32601,"message":"unknown method"}}),
            ),
            (3, "server.ping", json!([]), json!({"result":null})),
        ] {
            let mut request = String::new();
            stream.read_line(&mut request)?;
            assert_eq!(
                serde_json::from_str::<Value>(&request)?,
                json!({"id":id,"method":method,"params":params})
            );
            let mut response = response;
            response["id"] = json!(id);
            writeln!(stream.get_mut(), "{response}")?;
        }
        Ok(())
    });
    let mut client = Client::connect(&endpoint)?;
    let version = client.server_version()?;
    assert_eq!(version.server_version(), "test-server");
    assert_eq!(version.protocol_version(), "1.4");
    let error = client
        .call::<Value>("unknown.method", [json!("argument"), json!(7)])
        .err()
        .context("expected server error")?;
    assert!(error.is::<ServerError>());
    assert_eq!(
        client.call::<Value>("server.ping", [] as [(); 0])?,
        Value::Null
    );
    task.join()
        .map_err(|_| anyhow::anyhow!("mock Electrum server panicked"))??;
    Ok(())
}

#[test]
fn invalid_results_and_mismatched_ids_close_connection() -> Result<()> {
    for (reply, expected) in [
        (
            json!({"id":1,"result":["test-server", "1.5"]}),
            "invalid JSON-RPC result",
        ),
        (json!({"id":1,"result":"secret"}), "invalid JSON-RPC result"),
        (
            json!({"id":2,"result":["test-server", "1.4"]}),
            "JSON-RPC response ID does not match request",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || -> Result<()> {
            let (socket, _) = accept(&listener)?;
            let mut stream = BufReader::new(socket);
            stream.read_line(&mut String::new())?;
            writeln!(stream.get_mut(), "{reply}")?;
            assert_eq!(stream.read_line(&mut String::new())?, 0);
            Ok(())
        });
        let mut client = Client::connect(&endpoint)?;
        let error = client
            .server_version()
            .err()
            .context("invalid response accepted")?;
        assert_eq!(format!("{error:#}"), expected);
        let error = client
            .server_version()
            .err()
            .context("closed client was reused")?;
        assert_eq!(error.to_string(), "Electrum connection is closed");
        task.join()
            .map_err(|_| anyhow::anyhow!("mock Electrum server panicked"))??;
    }
    Ok(())
}

#[test]
fn notifications_and_framing() -> Result<()> {
    let mut input = &b"{\"id\":7,\"result\":null}\n{\"id\":8,\"result\":0}\n"[..];
    assert_eq!(read_reply(&mut input)?.into_result::<Value>()?, Value::Null);
    assert_eq!(read_reply(&mut input)?.into_result::<Value>()?, json!(0));
    for input in [
        b"{\"id\":1,\"result\":0}".as_slice(),
        b"{\"id\":1}\n",
        b"{\"method\":\"notice\",\"params\":[]}\n{\"id\":1,\"result\":0}\n",
        b"{\"id\":null,\"method\":\"notice\",\"params\":[]}\n",
        b"{\"id\":1,\"method\":\"server.request\",\"params\":[]}\n",
        b"not JSON\n",
        b"",
    ] {
        assert!(read_reply(&mut &*input).is_err());
    }
    let oversized = vec![b' '; usize::try_from(MAX_MESSAGE)? + 1];
    assert!(read_reply(&mut oversized.as_slice()).is_err());
    Ok(())
}
