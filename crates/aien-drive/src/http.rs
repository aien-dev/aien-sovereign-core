//! Minimal plain-HTTP client over `std::net` (loopback endpoints only).
//! Sends HTTP/1.0 with `Connection: close` and decodes chunked bodies if the
//! server uses them anyway.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct Response {
    pub status: u16,
    pub body: Box<dyn BufRead>,
}

fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// endpoints are supported: {url}"))?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    Ok((host, path.to_string()))
}

struct Chunked<R: BufRead> {
    inner: R,
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.left == 0 {
            let mut line = String::new();
            self.inner.read_line(&mut line)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            if size == 0 {
                self.done = true;
                return Ok(0);
            }
            self.left = size;
        }
        let n = buf.len().min(self.left);
        let got = self.inner.read(&mut buf[..n])?;
        self.left -= got;
        if self.left == 0 {
            let mut crlf = String::new();
            self.inner.read_line(&mut crlf)?;
        }
        Ok(got)
    }
}

/// POST a JSON body. `timeout` bounds each socket read and write.
pub fn post_json(url: &str, body: &str, timeout: Duration) -> Result<Response, String> {
    let (host, path) = split_url(url)?;
    let mut s = TcpStream::connect(&host).map_err(|e| format!("connect {host}: {e}"))?;
    s.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "POST {path} HTTP/1.0\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut r = BufReader::new(s);
    let mut status_line = String::new();
    r.read_line(&mut status_line)
        .map_err(|e| format!("read: {e}"))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("bad status line: {status_line:?}"))?;
    let mut chunked = false;
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h).map_err(|e| format!("read: {e}"))?;
        if n == 0 || h.trim().is_empty() {
            break;
        }
        let l = h.to_ascii_lowercase();
        if l.starts_with("transfer-encoding:") && l.contains("chunked") {
            chunked = true;
        }
    }
    let body: Box<dyn BufRead> = if chunked {
        Box::new(BufReader::new(Chunked {
            inner: r,
            left: 0,
            done: false,
        }))
    } else {
        Box::new(r)
    };
    Ok(Response { status, body })
}
