use super::{MAX_MESSAGE, connect, result};
use crate::Result;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::Arc,
};
use url::{Host, Url};

pub fn tip(endpoint: &Url) -> Result<Value> {
    match endpoint.scheme() {
        "tcp" => return exchange(connect(endpoint)?),
        "tls" | "ssl" => {}
        _ => return Err("unsupported Electrum transport".into()),
    }
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            })
            .with_no_client_auth();
    let connection = ClientConnection::new(Arc::new(config), server_name(endpoint)?)?;
    exchange(StreamOwned::new(connection, connect(endpoint)?))
}

fn server_name(endpoint: &Url) -> Result<ServerName<'static>> {
    match endpoint.host().ok_or("missing Electrum TLS host")? {
        Host::Domain(host) => {
            ServerName::try_from(host.to_owned()).map_err(|_| "invalid Electrum TLS host".into())
        }
        Host::Ipv4(ip) => Ok(ServerName::IpAddress(ip.into())),
        Host::Ipv6(ip) => Ok(ServerName::IpAddress(ip.into())),
    }
}

fn exchange(stream: impl Read + Write) -> Result<Value> {
    let mut stream = BufReader::new(stream);
    let mut request =
        serde_json::to_vec(&json!({"id":1,"method":"blockchain.headers.subscribe","params":[]}))?;
    request.push(b'\n');
    stream
        .get_mut()
        .write_all(&request)
        .map_err(|_| "Electrum write failed")?;
    stream
        .get_mut()
        .flush()
        .map_err(|_| "Electrum flush failed")?;
    read_reply(&mut stream)
}

fn read_reply(stream: &mut impl BufRead) -> Result<Value> {
    for _ in 0..100 {
        let mut bytes = Vec::new();
        stream
            .by_ref()
            .take(MAX_MESSAGE + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "Electrum read failed")?;
        if bytes.len() as u64 > MAX_MESSAGE || bytes.last() != Some(&b'\n') {
            return Err("Electrum response is truncated or exceeds size limit".into());
        }
        let reply: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid Electrum JSON")?;
        if reply.get("id").is_none() && reply.get("method").is_some() {
            continue;
        }
        return result(&reply, 1);
    }
    Err("too many Electrum notifications".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    #[test]
    fn tls_server_names_handle_domains_and_ip_addresses() -> Result<()> {
        assert!(matches!(
            server_name(&Url::parse("tls://example.com:50002")?)?,
            ServerName::DnsName(_)
        ));
        for host in ["127.0.0.1", "[::1]"] {
            assert!(matches!(
                server_name(&Url::parse(&format!("tls://{host}:50002"))?)?,
                ServerName::IpAddress(_)
            ));
        }
        Ok(())
    }

    #[test]
    fn plaintext_tcp_reads_tip() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || -> Result<()> {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(super::super::TIMEOUT))?;
            let mut stream = BufReader::new(socket);
            let mut request = String::new();
            stream.read_line(&mut request)?;
            assert_eq!(
                serde_json::from_str::<Value>(&request)?,
                json!({"id":1,"method":"blockchain.headers.subscribe","params":[]})
            );
            stream
                .get_mut()
                .write_all(b"{\"id\":1,\"result\":{\"height\":123}}\n")?;
            Ok(())
        });
        assert_eq!(tip(&endpoint)?, json!({"height":123}));
        task.join().map_err(|_| "mock Electrum server panicked")??;
        Ok(())
    }

    #[test]
    fn notifications_framing_and_ids() -> Result<()> {
        let mut input =
            &b"{\"method\":\"notice\",\"params\":[]}\n{\"id\":1,\"result\":{\"height\":123}}\n"[..];
        assert_eq!(read_reply(&mut input)?, json!({"height":123}));
        for input in [
            b"{\"id\":2,\"result\":0}\n".as_slice(),
            b"{\"id\":1,\"result\":0}",
            b"{\"id\":1,\"error\":{}}\n",
        ] {
            assert!(read_reply(&mut &*input).is_err());
        }
        let oversized = vec![b' '; usize::try_from(MAX_MESSAGE)? + 1];
        assert!(read_reply(&mut oversized.as_slice()).is_err());
        Ok(())
    }
}
