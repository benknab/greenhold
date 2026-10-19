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
