use super::{MAX_MESSAGE, TIMEOUT, result};
use crate::Result;
use reqwest::{blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use std::{fmt, io::Read};
use url::Url;

pub struct RpcConfig {
    url: Url,
    credentials: Option<(String, String)>,
}

impl fmt::Debug for RpcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcConfig")
            .field("authenticated", &self.credentials.is_some())
            .finish_non_exhaustive()
    }
}

impl RpcConfig {
    pub fn new(url: Url, credentials: Option<(String, String)>) -> Self {
        Self { url, credentials }
    }
}

pub struct Rpc<'a> {
    config: &'a RpcConfig,
    client: Client,
    id: u64,
}

impl<'a> Rpc<'a> {
    pub fn new(config: &'a RpcConfig) -> Result<Self> {
        Ok(Self {
            config,
            client: Client::builder()
                .timeout(TIMEOUT)
                .redirect(Policy::none())
                .build()
                .map_err(|_| "RPC client setup failed")?,
            id: 0,
        })
    }

    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value> {
        self.id += 1;
        let body = serde_json::to_vec(
            &json!({"jsonrpc":"1.0", "id":self.id, "method":method, "params":params}),
        )?;
        let mut request = self
            .client
            .post(self.config.url.clone())
            .header("Content-Type", "application/json")
            .body(body);
        if let Some((user, password)) = &self.config.credentials {
            request = request.basic_auth(user, Some(password));
        }
        let response = request.send().map_err(|_| "RPC transport failed")?;
        if !response.status().is_success() {
            return Err(format!(
                "RPC HTTP {}; check authentication and node availability",
                response.status().as_u16()
            )
            .into());
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_MESSAGE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "RPC response read failed")?;
        if bytes.len() as u64 > MAX_MESSAGE {
            return Err("RPC response exceeds size limit".into());
        }
        // Remote bodies and transport errors may contain secrets. Do not print them.
        let reply: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid RPC JSON")?;
        result(&reply, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn read_headers(stream: &mut impl BufRead) -> Result<String> {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            if stream.read_line(&mut line)? == 0 {
                return Err("request ended before headers".into());
            }
            if line == "\r\n" {
                return Ok(headers);
            }
            headers.push_str(&line.to_ascii_lowercase());
        }
    }

    fn serve(
        status: &str,
        body: String,
        auth: bool,
    ) -> Result<(Url, thread::JoinHandle<Result<()>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
        let status = status.to_owned();
        let task = thread::spawn(move || {
            let (socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(TIMEOUT))?;
            let mut stream = BufReader::new(socket);
            let headers = read_headers(&mut stream)?;
            assert!(headers.starts_with("post / http/1.1\r\n"));
            assert_eq!(headers.contains("authorization:"), auth);
            if auth {
                assert!(headers.contains("authorization: basic dxnlcjpwyxnz\r\n"));
            }
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .ok_or("missing request length")?
                .parse::<usize>()?;
            let mut request = vec![0; length];
            stream.read_exact(&mut request)?;
            let request: Value = serde_json::from_slice(&request)?;
            assert_eq!(
                request,
                json!({"jsonrpc":"1.0","id":1,"method":"getbestblockhash","params":[]})
            );
            write!(
                stream.get_mut(),
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nLocation: /redirected\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(())
        });
        Ok((url, task))
    }

    #[test]
    fn anonymous_and_authenticated_requests() -> Result<()> {
        for auth in [false, true] {
            let (url, task) = serve(
                "200 OK",
                json!({"id":1,"result":"tip","error":null}).to_string(),
                auth,
            )?;
            let credentials = auth.then(|| ("user".into(), "pass".into()));
            let config = RpcConfig::new(url, credentials);
            assert_eq!(
                Rpc::new(&config)?.call("getbestblockhash", &json!([]))?,
                json!("tip")
            );
            task.join().map_err(|_| "mock server panicked")??;
        }
        Ok(())
    }

    #[test]
    fn debug_redacts_credentials() -> Result<()> {
        let config = RpcConfig::new(
            Url::parse("http://localhost/")?,
            Some(("secret-user".into(), "secret-password".into())),
        );
        assert!(!format!("{config:?}").contains("secret"));
        Ok(())
    }

    #[test]
    fn rejects_redirects_errors_bad_ids_and_oversized_bodies() -> Result<()> {
        for (status, body) in [
            ("302 Found", "secret".into()),
            ("401 Unauthorized", "secret".into()),
            ("200 OK", json!({"id":2,"result":"secret"}).to_string()),
            (
                "200 OK",
                json!({"id":1,"error":{"message":"secret"}}).to_string(),
            ),
            ("200 OK", "secret".into()),
            ("200 OK", " ".repeat(usize::try_from(MAX_MESSAGE)? + 1)),
        ] {
            let (url, task) = serve(status, body, false)?;
            let config = RpcConfig::new(url, None);
            let response = Rpc::new(&config)?.call("getbestblockhash", &json!([]));
            assert!(response.is_err());
            assert!(!format!("{response:?}").contains("secret"));
            task.join().map_err(|_| "mock server panicked")??;
        }
        Ok(())
    }
}
