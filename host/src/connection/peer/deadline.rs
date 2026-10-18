use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

/// One deadline for transport negotiation, the Bitcoin handshake, and the operation.
/// TCP connection setup and DNS resolution happen before this deadline starts.
#[derive(Debug)]
pub(super) struct DeadlineStream {
    socket: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    pub(super) fn new(socket: TcpStream, timeout: Duration) -> Self {
        Self {
            socket,
            deadline: Instant::now() + timeout,
        }
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
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.flush()
    }
}
