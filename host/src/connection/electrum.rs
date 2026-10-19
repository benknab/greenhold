use super::{
    MAX_MESSAGE,
    header::HeaderResponse,
    json_rpc::{Request, Response, ServerError},
    socket::{SocketUrl, connect_tcp},
};
use anyhow::{Context, Result, bail};
use bitcoin::{BlockHash, block::Header};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    str::FromStr,
    sync::Arc,
};
use url::{Host, Url};

const PROTOCOL_VERSION: &str = "1.4";

#[derive(Deserialize)]
struct Features {
    genesis_hash: BlockHash,
}

/// Server identity and the negotiated protocol version.
#[derive(Debug, Deserialize)]
#[serde(try_from = "[String; 2]")]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "context retrieval negotiates the version without displaying it"
    )
)]
pub struct Version {
    server_version: String,
    protocol_version: String,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "version accessors remain covered by client tests")
)]
impl Version {
    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }
}

impl TryFrom<[String; 2]> for Version {
    type Error = anyhow::Error;

    fn try_from([server_version, protocol_version]: [String; 2]) -> Result<Self> {
        // A single version string in the request offers exactly that version.
        if protocol_version != PROTOCOL_VERSION {
            bail!("Electrum server selected an unsupported protocol version");
        }
        Ok(Self {
            server_version,
            protocol_version,
        })
    }
}

/// A validated address. Private fields keep the URL and transport consistent.
#[derive(Clone, Debug)]
pub struct Endpoint {
    url: SocketUrl,
    transport: Transport,
}

#[derive(Clone, Debug)]
enum Transport {
    Tcp,
    Tls(ServerName<'static>),
}

impl Endpoint {
    pub fn socket_url(&self) -> &SocketUrl {
        &self.url
    }
}

impl FromStr for Endpoint {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::try_from(Url::parse(text).context("invalid Electrum URL")?)
    }
}

impl TryFrom<Url> for Endpoint {
    type Error = anyhow::Error;

    fn try_from(url: Url) -> Result<Self> {
        let url = SocketUrl::try_from(url)?;
        let transport = match url.url().scheme() {
            "tcp" => Transport::Tcp,
            "tls" | "ssl" => Transport::Tls(server_name(url.url())?),
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
pub struct Client {
    stream: Option<BufReader<Stream>>,
    next_id: u64,
}

impl Client {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let stream = match &endpoint.transport {
            Transport::Tcp => Stream::Tcp(connect_tcp(endpoint.socket_url())?),
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
                let mut socket = connect_tcp(endpoint.socket_url())?;
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

    /// Identify this client and negotiate the supported Electrum protocol version.
    pub fn server_version(&mut self) -> Result<Version> {
        self.call(
            "server.version",
            [
                concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
                PROTOCOL_VERSION,
            ],
        )
    }

    pub fn genesis_hash(&mut self) -> Result<BlockHash> {
        let features: Features = self.call("server.features", [] as [(); 0])?;
        Ok(features.genesis_hash)
    }

    pub fn block_header(&mut self, height: u32) -> Result<Header> {
        let HeaderResponse(header) = self.call("blockchain.block.header", [height])?;
        Ok(header)
    }

    /// Send a request/response method and positional parameters. Subscriptions are unsupported.
    /// I/O or protocol errors close the connection; matched server errors do not.
    fn call<T: DeserializeOwned>(&mut self, method: &str, params: impl Serialize) -> Result<T> {
        let id = self.next_id;
        let next_id = id.checked_add(1).context("Electrum request ID exhausted")?;
        let mut request = serde_json::to_vec(&Request {
            jsonrpc: None,
            id,
            method,
            params,
        })?;
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
        let reply = read_reply(&mut stream)?;
        if reply.id != id {
            bail!("JSON-RPC response ID does not match request");
        }
        let result: Result<T> = reply.into_result();
        if result.is_ok() || result.as_ref().is_err_and(anyhow::Error::is::<ServerError>) {
            self.stream = Some(stream);
        }
        result
    }
}

fn read_reply(stream: &mut impl BufRead) -> Result<Response> {
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take(MAX_MESSAGE + 1)
        .read_until(b'\n', &mut bytes)
        .context("Electrum read failed")?;
    if bytes.len() as u64 > MAX_MESSAGE || bytes.last() != Some(&b'\n') {
        bail!("Electrum response is truncated or exceeds size limit");
    }
    Response::parse(&bytes)
}

#[cfg(test)]
mod tests;
