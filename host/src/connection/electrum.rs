use super::{MAX_MESSAGE, connect, result, validate_socket_url};
use anyhow::{Context, Result, bail};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    str::FromStr,
    sync::Arc,
};
use url::{Host, Url};

/// A validated address. Private fields keep the URL and transport consistent.
#[derive(Clone, Debug)]
pub struct ElectrumEndpoint {
    url: Url,
    transport: Transport,
}

#[derive(Clone, Debug)]
enum Transport {
    Tcp,
    Tls(ServerName<'static>),
}

impl ElectrumEndpoint {
    pub fn url(&self) -> &Url {
        &self.url
    }
}

impl FromStr for ElectrumEndpoint {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::try_from(Url::parse(text).context("invalid Electrum URL")?)
    }
}

impl TryFrom<Url> for ElectrumEndpoint {
    type Error = anyhow::Error;

    fn try_from(url: Url) -> Result<Self> {
        validate_socket_url(&url)?;
        let transport = match url.scheme() {
            "tcp" => Transport::Tcp,
            "tls" | "ssl" => Transport::Tls(server_name(&url)?),
            _ => bail!("Electrum endpoint must use tcp://, tls://, or ssl://"),
        };
        Ok(Self { url, transport })
    }
}

fn server_name(endpoint: &Url) -> Result<ServerName<'static>> {
    match endpoint.host().context("missing Electrum TLS host")? {
        Host::Domain(host) => {
            ServerName::try_from(host.to_owned()).context("invalid Electrum TLS host")
        }
        Host::Ipv4(ip) => Ok(ServerName::IpAddress(ip.into())),
        Host::Ipv6(ip) => Ok(ServerName::IpAddress(ip.into())),
    }
}

#[derive(Debug)]
enum Stream {
    Tcp(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(bytes),
            Self::Tls(stream) => stream.read(bytes),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(bytes),
            Self::Tls(stream) => stream.write(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

/// A persistent connection with one request in flight at a time.
#[derive(Debug)]
pub struct Electrum {
    stream: Option<BufReader<Stream>>,
    next_id: u64,
}

impl Electrum {
    pub fn connect(endpoint: &ElectrumEndpoint) -> Result<Self> {
        let stream = match &endpoint.transport {
            Transport::Tcp => Stream::Tcp(connect(endpoint.url())?),
            Transport::Tls(name) => {
                let config = ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()?
                .with_root_certificates(RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                })
                .with_no_client_auth();
                let mut connection = ClientConnection::new(Arc::new(config), name.clone())?;
                let mut socket = connect(endpoint.url())?;
                // Finish certificate verification before returning a connected client.
                while connection.is_handshaking() {
                    connection
                        .complete_io(&mut socket)
                        .context("Electrum TLS handshake failed")?;
                }
                Stream::Tls(Box::new(StreamOwned::new(connection, socket)))
            }
        };
        Ok(Self {
            stream: Some(BufReader::new(stream)),
            next_id: 1,
        })
    }

    /// Send a request/response method and positional parameters. Subscriptions are unsupported.
    /// I/O or protocol errors close the connection; matched server errors do not.
    pub fn call(&mut self, method: &str, params: &[Value]) -> Result<Value> {
        let id = self.next_id;
        let next_id = id.checked_add(1).context("Electrum request ID exhausted")?;
        let mut request = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))?;
        request.push(b'\n');
        if request.len() as u64 > MAX_MESSAGE {
            bail!("Electrum request exceeds size limit");
        }
        let mut stream = self
            .stream
            .take()
            .context("Electrum connection is closed")?;
        self.next_id = next_id;
        stream
            .get_mut()
            .write_all(&request)
            .context("Electrum write failed")?;
        stream.get_mut().flush().context("Electrum flush failed")?;
        let reply = read_reply(&mut stream, id)?;
        self.stream = Some(stream);
        result(&reply, id)
    }
}

fn read_reply(stream: &mut impl BufRead, id: u64) -> Result<Value> {
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take(MAX_MESSAGE + 1)
        .read_until(b'\n', &mut bytes)
        .context("Electrum read failed")?;
    if bytes.len() as u64 > MAX_MESSAGE || bytes.last() != Some(&b'\n') {
        bail!("Electrum response is truncated or exceeds size limit");
    }
    let reply: Value = serde_json::from_slice(&bytes).context("invalid Electrum JSON")?;
    if reply.get("method").is_some() {
        bail!("unexpected Electrum notification or server request");
    }
    if reply.get("id") != Some(&json!(id)) {
        bail!("Electrum response ID does not match request");
    }
    if reply.get("result").is_none() && reply["error"].is_null() {
        bail!("missing Electrum result or error");
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    #[test]
    fn validated_transports_and_tls_names() -> Result<()> {
        let tcp: ElectrumEndpoint = "tcp://localhost:50001".parse()?;
        assert!(matches!(tcp.transport, Transport::Tcp));
        for scheme in ["tls", "ssl"] {
            let endpoint: ElectrumEndpoint = format!("{scheme}://example.com:50002").parse()?;
            assert!(matches!(
                endpoint.transport,
                Transport::Tls(ServerName::DnsName(_))
            ));
            for host in ["127.0.0.1", "[::1]"] {
                let endpoint: ElectrumEndpoint = format!("{scheme}://{host}:50002").parse()?;
                assert!(matches!(
                    endpoint.transport,
                    Transport::Tls(ServerName::IpAddress(_))
                ));
            }
        }
        assert!(
            "tls://bad%20host:50002"
                .parse::<ElectrumEndpoint>()
                .is_err()
        );
        assert!(ElectrumEndpoint::try_from(Url::parse("https://localhost:50002")?).is_err());
        Ok(())
    }

    #[test]
    fn reuses_connection_for_different_requests_and_server_errors() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || -> Result<()> {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(super::super::TIMEOUT))?;
            let mut stream = BufReader::new(socket);
            for (id, method, params, response) in [
                (
                    1,
                    "server.version",
                    json!(["test-client", "1.4"]),
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
        let mut client = Electrum::connect(&endpoint)?;
        assert_eq!(
            client.call("server.version", &[json!("test-client"), json!("1.4")])?,
            json!(["test-server", "1.4"])
        );
        assert!(
            client
                .call("unknown.method", &[json!("argument"), json!(7)])
                .is_err()
        );
        assert_eq!(client.call("server.ping", &[])?, Value::Null);
        task.join()
            .map_err(|_| anyhow::anyhow!("mock Electrum server panicked"))??;
        Ok(())
    }

    #[test]
    fn protocol_failure_closes_connection() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let endpoint = format!("tcp://{}", listener.local_addr()?).parse()?;
        let task = thread::spawn(move || -> Result<()> {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(super::super::TIMEOUT))?;
            let mut stream = BufReader::new(socket);
            let mut request = String::new();
            stream.read_line(&mut request)?;
            stream
                .get_mut()
                .write_all(b"{\"id\":2,\"result\":0}\n{\"id\":1,\"result\":1}\n")?;
            assert_eq!(stream.read_line(&mut String::new())?, 0);
            Ok(())
        });
        let mut client = Electrum::connect(&endpoint)?;
        assert!(client.call("server.ping", &[]).is_err());
        assert!(client.stream.is_none());
        assert_eq!(
            client
                .call("server.ping", &[])
                .err()
                .context("expected closed client")?
                .to_string(),
            "Electrum connection is closed"
        );
        task.join()
            .map_err(|_| anyhow::anyhow!("mock Electrum server panicked"))??;
        Ok(())
    }

    #[test]
    fn notifications_framing_and_ids() -> Result<()> {
        let mut input = &b"{\"id\":7,\"result\":null}\n{\"id\":8,\"result\":0}\n"[..];
        assert_eq!(result(&read_reply(&mut input, 7)?, 7)?, Value::Null);
        assert_eq!(result(&read_reply(&mut input, 8)?, 8)?, json!(0));
        for input in [
            b"{\"id\":2,\"result\":0}\n".as_slice(),
            b"{\"id\":1,\"result\":0}",
            b"{\"id\":1}\n",
            b"{\"method\":\"notice\",\"params\":[]}\n{\"id\":1,\"result\":0}\n",
            b"{\"id\":null,\"method\":\"notice\",\"params\":[]}\n",
            b"{\"id\":1,\"method\":\"server.request\",\"params\":[]}\n",
            b"not JSON\n",
            b"",
        ] {
            assert!(read_reply(&mut &*input, 1).is_err());
        }
        let oversized = vec![b' '; usize::try_from(MAX_MESSAGE)? + 1];
        assert!(read_reply(&mut oversized.as_slice(), 1).is_err());
        Ok(())
    }
}
