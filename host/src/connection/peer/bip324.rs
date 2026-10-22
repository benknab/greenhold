use super::{
    super::socket::{self, SocketUrl},
    Transport,
    deadline::DeadlineStream,
};
use anyhow::{Context, Result, bail};
use bip324::{
    PacketType, Role,
    io::{Payload, Protocol},
};
use bitcoin::{
    consensus,
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
    /// The exchange budget starts after TCP connection setup and includes BIP324 negotiation.
    pub(super) fn connect(endpoint: &SocketUrl, timeout: Duration) -> Result<Self> {
        Self::new(socket::connect_tcp(endpoint)?, timeout)
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
    let payload = consensus::serialize(message);
    let command = message.command();
    let mut contents = match SHORT_COMMANDS
        .iter()
        .position(|name| *name == command.as_ref())
    {
        Some(id) if id != 0 => vec![u8::try_from(id)?],
        _ => {
            let mut prefix = vec![0];
            prefix.extend(consensus::serialize(&command));
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
        consensus::deserialize(&encoded).context("invalid BIP324 message payload")?;
    Ok(message.into_payload())
}

#[cfg(test)]
mod tests;
