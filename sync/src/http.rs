//! Just enough HTTP/1.1 for our handful of endpoints: requests with small
//! bodies, keep-alive, and responses we write straight to the socket.

use std::io::{self, BufRead, Read, Write};

const MAX_HEAD: usize = 8 * 1024;
pub const MAX_BODY: usize = 16 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub keep_alive: bool,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn bearer(&self) -> Option<&str> {
        let auth = self.header("authorization")?;
        let (scheme, token) = auth.split_once(' ')?;
        scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
    }
}

#[derive(Debug)]
pub enum ReadError {
    /// The client went away or was idle too long, just close.
    Closed,
    /// Say this status and close.
    Bad(u16),
}

impl From<io::Error> for ReadError {
    fn from(_: io::Error) -> Self {
        ReadError::Closed
    }
}

pub fn read_request(r: &mut impl BufRead) -> Result<Request, ReadError> {
    let mut head = Vec::with_capacity(512);
    loop {
        let before = head.len();
        let n = (&mut *r).take((MAX_HEAD + 1 - before) as u64).read_until(b'\n', &mut head)?;
        if n == 0 {
            return Err(ReadError::Closed);
        }
        if head.len() > MAX_HEAD {
            return Err(ReadError::Bad(431));
        }
        // skip stray blank lines between requests
        if head == b"\r\n" || head == b"\n" {
            head.clear();
            continue;
        }
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            break;
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(&head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(ReadError::Bad(400)),
    }
    let method = req.method.unwrap_or("").to_string();
    let path = req.path.unwrap_or("/").to_string();
    let http10 = req.version == Some(0);
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).trim().to_string()))
        .collect();
    let mut out = Request { method, path, headers, body: Vec::new(), keep_alive: !http10 };
    if let Some(conn) = out.header("connection") {
        let conn = conn.to_ascii_lowercase();
        if conn.contains("close") {
            out.keep_alive = false;
        } else if conn.contains("keep-alive") {
            out.keep_alive = true;
        }
    }

    if out.header("transfer-encoding").is_some() {
        return Err(ReadError::Bad(411));
    }
    let len = match out.header("content-length") {
        Some(v) => v.parse::<usize>().map_err(|_| ReadError::Bad(400))?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err(ReadError::Bad(413));
    }
    out.body.resize(len, 0);
    r.read_exact(&mut out.body)?;
    Ok(out)
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

pub fn write_head(w: &mut impl Write, status: u16, headers: &[(&str, String)], keep_alive: bool) -> io::Result<()> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str(if keep_alive { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
    w.write_all(out.as_bytes())
}

pub fn write_json(w: &mut impl Write, status: u16, body: &str, keep_alive: bool) -> io::Result<()> {
    write_head(
        w,
        status,
        &[("Content-Type", "application/json".into()), ("Content-Length", body.len().to_string())],
        keep_alive,
    )?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

pub fn write_error(w: &mut impl Write, status: u16, msg: &str, keep_alive: bool) -> io::Result<()> {
    write_json(w, status, &serde_json::json!({ "error": msg }).to_string(), keep_alive)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Range {
    /// No usable Range header, send it all.
    Full,
    /// Inclusive byte range.
    Part(u64, u64),
    Unsatisfiable,
}

/// Parses a single `bytes=` range against a file of `size` bytes. Anything
/// we don't understand (other units, several ranges) gets the whole file,
/// which is what RFC 9110 allows.
pub fn parse_range(header: Option<&str>, size: u64) -> Range {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Range::Full };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((start, end)) = spec.trim().split_once('-') else { return Range::Full };
    let (start, end) = (start.trim(), end.trim());
    if start.is_empty() {
        // last N bytes
        let Ok(n) = end.parse::<u64>() else { return Range::Full };
        if n == 0 || size == 0 {
            return Range::Unsatisfiable;
        }
        return Range::Part(size.saturating_sub(n), size - 1);
    }
    let Ok(start) = start.parse::<u64>() else { return Range::Full };
    if start >= size {
        return Range::Unsatisfiable;
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        match end.parse::<u64>() {
            Ok(e) if e >= start => e.min(size - 1),
            _ => return Range::Full,
        }
    };
    Range::Part(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Range::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 100), Range::Part(0, 99));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Range::Part(10, 19));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Range::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Range::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Range::Part(0, 99));
        assert_eq!(parse_range(Some("bytes=99-"), 100), Range::Part(99, 99));
        assert_eq!(parse_range(Some(" bytes= 5 - 6 "), 100), Range::Part(5, 6));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-"), 0), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-1"), 100), Range::Full);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Range::Full);
        assert_eq!(parse_range(Some("items=0-1"), 100), Range::Full);
        assert_eq!(parse_range(Some("bytes=abc-"), 100), Range::Full);
        assert_eq!(parse_range(Some("bytes=5"), 100), Range::Full);
    }

    #[test]
    fn reads_requests_with_bodies_and_keep_alive() {
        let raw = b"POST /pair HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nAuthorization: Bearer abc \r\n\r\nhelloGET /hello HTTP/1.0\r\n\r\n";
        let mut r = BufReader::new(&raw[..]);
        let req = read_request(&mut r).unwrap();
        assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", "/pair"));
        assert_eq!(req.body, b"hello");
        assert_eq!(req.bearer(), Some("abc"));
        assert!(req.keep_alive);
        let req = read_request(&mut r).unwrap();
        assert_eq!(req.path, "/hello");
        assert!(!req.keep_alive);
        assert!(matches!(read_request(&mut r), Err(ReadError::Closed)));
    }

    #[test]
    fn rejects_oversized_and_chunked() {
        let big = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert!(matches!(read_request(&mut BufReader::new(big.as_bytes())), Err(ReadError::Bad(431))));
        let chunked = b"POST /pair HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(matches!(read_request(&mut BufReader::new(&chunked[..])), Err(ReadError::Bad(411))));
        let body = format!("POST /pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert!(matches!(read_request(&mut BufReader::new(body.as_bytes())), Err(ReadError::Bad(413))));
    }
}
