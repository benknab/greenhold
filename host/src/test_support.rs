use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::{self, BufRead};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

// A missing request must fail the fixture, not leave a join blocked forever.
pub fn accept(listener: &TcpListener) -> Result<(TcpStream, SocketAddr)> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((socket, address)) => {
                socket.set_nonblocking(false)?;
                socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                socket.set_write_timeout(Some(Duration::from_secs(5)))?;
                return Ok((socket, address));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    bail!("mock did not receive its expected connection");
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub fn read_http(stream: &mut impl BufRead) -> Result<(String, Value)> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line)? == 0 {
            bail!("mock request ended before HTTP headers");
        }
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line.to_ascii_lowercase());
    }
    let length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .context("missing content length")?
        .parse::<usize>()?;
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    Ok((headers, serde_json::from_slice(&body)?))
}
