use super::*;
use std::io::Cursor;

#[test]
fn replies_require_one_complete_line() -> Result<()> {
    let reply = b"{\"id\":1,\"result\":[]}\n";
    let mut stream = Cursor::new([reply.as_slice(), reply.as_slice()].concat());
    assert_eq!(read_reply(&mut stream)?.id, 1);
    assert_eq!(stream.position(), reply.len() as u64);
    assert_eq!(read_reply(&mut stream)?.id, 1);

    for bytes in [b"".as_slice(), &reply[..reply.len() - 1]] {
        let error = read_reply(&mut Cursor::new(bytes))
            .err()
            .context("invalid reply accepted")?;
        assert_eq!(error.to_string(), "Electrum response ended before newline");
    }
    Ok(())
}

#[derive(Debug)]
struct FailedRead(io::ErrorKind);

impl Read for FailedRead {
    fn read(&mut self, _bytes: &mut [u8]) -> io::Result<usize> {
        Err(self.0.into())
    }
}

#[test]
fn socket_timeouts_are_distinct_from_other_read_failures() -> Result<()> {
    for (kind, message) in [
        (io::ErrorKind::TimedOut, "Electrum read timed out"),
        (io::ErrorKind::WouldBlock, "Electrum read timed out"),
        (io::ErrorKind::ConnectionReset, "Electrum read failed"),
    ] {
        let error = read_reply(&mut BufReader::new(FailedRead(kind)))
            .err()
            .context("failed read accepted")?;
        assert_eq!(error.to_string(), message);
        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .context("I/O cause")?
                .kind(),
            kind
        );
    }
    Ok(())
}
