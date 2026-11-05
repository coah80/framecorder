// hide_names is for the apps and the headset, which know the names
#[allow(dead_code)]
mod shared;
mod github;
mod issue;
mod privacy;

use anyhow::{Context, Result};
use github::{now, Config, GitHub};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Limits {
    clients: HashMap<IpAddr, VecDeque<u64>>,
    overall: VecDeque<u64>,
}

impl Limits {
    fn allow(&mut self, ip: IpAddr, time: u64) -> bool {
        self.overall.retain(|t| time.saturating_sub(*t) < 86400);
        self.clients.retain(|_, times| {
            times.retain(|t| time.saturating_sub(*t) < 86400);
            !times.is_empty()
        });
        let times = self.clients.entry(ip).or_default();
        if self.overall.len() >= 300
            || times.len() >= 20
            || times
                .iter()
                .filter(|t| time.saturating_sub(**t) < 3600)
                .count()
                >= 5
        {
            return false;
        }
        times.push_back(time);
        self.overall.push_back(time);
        true
    }
}

struct Service {
    github: Mutex<GitHub>,
    limits: Mutex<Limits>,
}

struct Reply {
    status: u16,
    mime: &'static str,
    body: String,
}
fn json_reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        mime: "application/json",
        body: body.to_string(),
    }
}
fn error(status: u16, message: &str) -> Reply {
    json_reply(status, json!({"ok":false,"error":message}))
}
impl Service {
    fn route(
        &self,
        method: &str,
        target: &str,
        ip: IpAddr,
        body: &[u8],
        content_type: &str,
    ) -> Reply {
        let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else {
            return error(400, "This address could not be read.");
        };
        match (method, url.path()) {
            ("GET", "/health") => match self.github.lock().expect("GitHub lock").health() {
                Ok(slug) => json_reply(
                    200,
                    json!({"ok":true,"key_loaded":true,"installation_available":true,"app":slug}),
                ),
                Err(_) => json_reply(
                    503,
                    json!({"ok":false,"key_loaded":true,"installation_available":false,"error":"The bot cannot reach its GitHub installation. Check its repository access and try again."}),
                ),
            },
            ("POST", "/v1/report") => {
                if content_type.split(';').next().unwrap_or_default().trim() != "application/json" {
                    return error(
                        400,
                        "Send the report as JSON. Update framecorder and try again.",
                    );
                }
                let report = match issue::parse(body) {
                    Ok(r) => r,
                    Err(e) => return error(400, e),
                };
                if !self
                    .limits
                    .lock()
                    .expect("rate limiter lock")
                    .allow(ip, now())
                {
                    return error(
                        429,
                        "Too many reports have been sent. Please try again tomorrow.",
                    );
                }
                let (title, body) = issue::build(&report);
                match self.github.lock().expect("GitHub lock").issue(title, body) {
                    Ok(v) => json_reply(200, v),
                    Err(_) => error(
                        503,
                        "The issue service is unavailable. Please try again later.",
                    ),
                }
            }
            _ => error(
                404,
                "This address was not found. Update framecorder and try again.",
            ),
        }
    }
}

fn receive(stream: &mut TcpStream, service: &Service) -> Reply {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut buf = [0; 8192];
    let (method, target, length, ip, content_type, header_end) = loop {
        if !read_deadline(stream, deadline) {
            return error(400, "The report took too long to arrive. Try again.");
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => {
                return error(400, "The report was interrupted. Try sending it again.")
            }
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
        let mut headers = [httparse::EMPTY_HEADER; 40];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&bytes) {
            Ok(httparse::Status::Complete(end)) => {
                if end > 16384 {
                    return error(400, "The request headers are too large.");
                }
                let values: Vec<_> = req
                    .headers
                    .iter()
                    .filter(|h| h.name.eq_ignore_ascii_case("Content-Length"))
                    .collect();
                if values.len() > 1
                    || req
                        .headers
                        .iter()
                        .any(|h| h.name.eq_ignore_ascii_case("Transfer-Encoding"))
                {
                    return error(
                        400,
                        "The request format is not supported. Try sending it again.",
                    );
                }
                let length = match values.first() {
                    Some(h) => match std::str::from_utf8(h.value)
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                    {
                        Some(n) => n,
                        None => return error(400, "The report size could not be read."),
                    },
                    None => 0,
                };
                if length > issue::MAX_REQUEST {
                    return error(413, "This report is too large. Shorten it and try again.");
                }
                let header = |name: &str| {
                    req.headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case(name))
                        .and_then(|h| std::str::from_utf8(h.value).ok())
                        .unwrap_or_default()
                };
                let peer = match stream.peer_addr() {
                    Ok(a) => a.ip(),
                    Err(_) => return error(400, "The connection was interrupted."),
                };
                let ip = header("CF-Connecting-IP").parse().unwrap_or(peer);
                break (
                    req.method.unwrap_or_default().to_owned(),
                    req.path.unwrap_or_default().to_owned(),
                    length,
                    ip,
                    header("Content-Type").to_owned(),
                    end,
                );
            }
            Ok(httparse::Status::Partial) if bytes.len() <= 16384 => {}
            _ => return error(400, "The request could not be read. Try sending it again."),
        }
    };
    while bytes.len() - header_end < length {
        let want = buf.len().min(length - (bytes.len() - header_end));
        if !read_deadline(stream, deadline) {
            return error(400, "The report took too long to arrive. Try again.");
        }
        match stream.read(&mut buf[..want]) {
            Ok(0) | Err(_) => {
                return error(400, "The report was interrupted. Try sending it again.")
            }
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
    }
    service.route(
        &method,
        &target,
        ip,
        &bytes[header_end..header_end + length],
        &content_type,
    )
}

fn read_deadline(stream: &TcpStream, deadline: Instant) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    !remaining.is_zero() && stream.set_read_timeout(Some(remaining)).is_ok()
}

fn handle(mut stream: TcpStream, service: &Service) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let reply = receive(&mut stream, service);
    let drain = reply.status == 413 || reply.status == 400;
    respond(&mut stream, reply);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    if drain {
        // let clients receive the error before closing an upload we rejected early.
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut discarded = 0;
        let mut buffer = [0; 8192];
        while discarded < 2 * issue::MAX_REQUEST && read_deadline(&stream, deadline) {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => discarded += n,
            }
        }
    }
}

fn respond(stream: &mut TcpStream, reply: Reply) {
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let _ = write!(stream,"HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\r\n{}",reply.status,reason,reply.mime,reply.body.len(),reply.body);
}

fn main() -> Result<()> {
    let config = Config::load()?;
    if std::env::args().len() > 1 {
        anyhow::bail!("usage: framecorder-report");
    }
    let github = GitHub::new(config.clone())?;
    let listener =
        TcpListener::bind(("127.0.0.1", config.port)).context("opening the report service port")?;
    eprintln!("framecorder-report listening on {}", listener.local_addr()?);
    let service = Arc::new(Service {
        github: Mutex::new(github),
        limits: Mutex::new(Limits::default()),
    });
    let (send, receive) = mpsc::sync_channel::<TcpStream>(16);
    let receive = Arc::new(Mutex::new(receive));
    for _ in 0..4 {
        let receive = receive.clone();
        let service = service.clone();
        std::thread::spawn(move || loop {
            let stream = receive.lock().expect("connection queue lock").recv();
            match stream {
                Ok(stream) => handle(stream, &service),
                Err(_) => break,
            }
        });
    }
    for stream in listener.incoming().flatten() {
        if let Err(mpsc::TrySendError::Full(mut stream)) = send.try_send(stream) {
            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
            respond(
                &mut stream,
                error(503, "The issue service is busy. Please try again later."),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_roll_without_sleeping() {
        let mut l = Limits::default();
        let ip = "192.168.0.1".parse().unwrap();
        for hour in 0..4 {
            for _ in 0..5 {
                assert!(l.allow(ip, hour * 3600));
            }
            assert!(!l.allow(ip, hour * 3600));
        }
        assert!(!l.allow(ip, 4 * 3600));
        assert!(l.allow(ip, 86400));
        let mut l = Limits::default();
        for i in 0..300u16 {
            assert!(l.allow(IpAddr::from([10, 0, (i / 256) as u8, (i % 256) as u8]), 0));
        }
        assert!(!l.allow("10.1.0.1".parse().unwrap(), 0));
        assert!(l.allow("10.1.0.1".parse().unwrap(), 86400));
    }
}
