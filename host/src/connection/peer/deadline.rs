use std::{
    io::{self, BufReader, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

/// One deadline for transport negotiation, the Bitcoin handshake, and the operation.
/// TCP connection setup and DNS resolution happen before this deadline starts.
#[derive(Debug)]
pub(super) struct DeadlineStream {
    socket: BufReader<TcpStream>,
    deadline: Instant,
}

impl DeadlineStream {
    pub(super) fn new(socket: TcpStream, timeout: Duration) -> Self {
        Self {
            socket: BufReader::new(socket),
            deadline: Instant::now() + timeout,
        }
    }

    /// Separate handles for the library's reader and writer, with one deadline.
    pub(super) fn pair(socket: TcpStream, timeout: Duration) -> io::Result<(Self, Self)> {
        let reader = Self::new(socket, timeout);
        let writer = Self {
            socket: BufReader::new(reader.socket.get_ref().try_clone()?),
            deadline: reader.deadline,
        };
        Ok((reader, writer))
    }

    pub(super) fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "P2P operation deadline exceeded")
            })
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket
            .get_ref()
            .set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket
            .get_ref()
            .set_write_timeout(Some(self.remaining()?))?;
        self.socket.get_mut().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket
            .get_ref()
            .set_write_timeout(Some(self.remaining()?))?;
        self.socket.get_mut().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use std::net::TcpListener;

    #[test]
    fn shared_deadline_applies_to_buffered_reads_and_writes() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let client = TcpStream::connect(listener.local_addr()?)?;
        let (mut server, _) = listener.accept()?;
        let (mut reader, mut writer) = DeadlineStream::pair(client, Duration::from_secs(20))?;
        assert_eq!(reader.deadline, writer.deadline);

        // Put unread data in the buffer, then expire the deadline deterministically.
        server.write_all(&[1, 2])?;
        reader.socket.read_exact(&mut [0; 1])?;
        if reader.socket.buffer().is_empty() {
            use std::io::BufRead;
            reader.socket.fill_buf()?;
        }
        assert!(!reader.socket.buffer().is_empty());
        reader.deadline = Instant::now();
        writer.deadline = reader.deadline;
        assert_eq!(
            reader
                .read(&mut [0; 1])
                .err()
                .context("expected read timeout")?
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            writer
                .write(&[3])
                .err()
                .context("expected write timeout")?
                .kind(),
            io::ErrorKind::TimedOut
        );
        Ok(())
    }
}
