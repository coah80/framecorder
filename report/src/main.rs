mod github;
mod issue;
mod privacy;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use github::{now, save, Config, GitHub};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    fs,
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

#[derive(Serialize, Deserialize)]
struct SetupToken {
    token: String,
    created: u64,
}

fn random_token() -> Result<SetupToken> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("could not make a setup link"))?;
    Ok(SetupToken {
        token: URL_SAFE_NO_PAD.encode(bytes),
        created: now(),
    })
}

fn setup_matches(path: &std::path::Path, supplied: &str) -> bool {
    fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice::<SetupToken>(&v).ok())
        .is_some_and(|t| now().saturating_sub(t.created) < 3600 && t.token == supplied)
}

struct Service {
    config: Config,
    github: Mutex<GitHub>,
    limits: Mutex<Limits>,
    setup: Mutex<()>,
}

struct Reply {
    status: u16,
    mime: &'static str,
    body: String,
    location: Option<String>,
}
fn json_reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        mime: "application/json",
        body: body.to_string(),
        location: None,
    }
}
fn error(status: u16, message: &str) -> Reply {
    json_reply(status, json!({"ok":false,"error":message}))
}
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        let param = |name: &str| {
            url.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        };
        match (method, url.path()) {
            ("GET", "/health") => json_reply(
                200,
                json!({"ok":true,"bot_configured":self.config.state.join("app.json").is_file()}),
            ),
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
            ("GET", "/setup") => {
                let _guard = self.setup.lock().expect("setup lock");
                if self.config.state.join("app.json").exists() {
                    return error(403, "The bot is already set up.");
                }
                let path = self.config.state.join("setup-token.json");
                if !setup_matches(&path, &param("token")) {
                    return error(403, "This setup link has expired or was used. Run setup-link again on the server.");
                }
                let result = (|| -> Result<String> {
                    let state = random_token()?;
                    save(&self.config.state.join("setup-pending.json"), &state)?;
                    fs::remove_file(path)?;
                    let manifest = json!({"name":self.config.app_name,"url":self.config.public_url,"redirect_url":format!("{}/setup/done",self.config.public_url),"public":false,"hook_attributes":{"active":false},"default_permissions":{"issues":"write","metadata":"read"},"default_events":[]});
                    Ok(format!("<!doctype html><meta charset=utf-8><title>Set up framecorder</title><form method=post action=\"https://github.com/settings/apps/new?state={}\"><input type=hidden name=manifest value=\"{}\"><button>Set up the GitHub bot</button></form><script>document.forms[0].submit()</script>",state.token,html(&manifest.to_string())))
                })();
                match result {
                    Ok(body) => Reply {
                        status: 200,
                        mime: "text/html; charset=utf-8",
                        body,
                        location: None,
                    },
                    Err(_) => error(
                        503,
                        "Setup could not be saved. Check the server's state folder and try again.",
                    ),
                }
            }
            ("GET", "/setup/done") => {
                let _guard = self.setup.lock().expect("setup lock");
                let path = self.config.state.join("setup-pending.json");
                if self.config.state.join("app.json").exists()
                    || !setup_matches(&path, &param("state"))
                {
                    return error(403,"This setup link has expired or was used. Run setup-link again on the server.");
                }
                match self.github.lock().expect("GitHub lock").convert(&param("code")) {
                    Ok(c) => {
                        let _ = fs::remove_file(path);
                        Reply { status:303, mime:"application/json", body:json!({"ok":true}).to_string(),location:Some(format!("https://github.com/apps/{}/installations/new",c.slug)) }
                    },
                    Err(_) => error(503,"GitHub could not finish setup. Run setup-link again on the server and try again."),
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
        303 => "See Other",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let location = reply
        .location
        .map(|l| format!("Location: {l}\r\n"))
        .unwrap_or_default();
    let _ = write!(stream,"HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n{}\r\n{}",reply.status,reason,reply.mime,reply.body.len(),location,reply.body);
}

fn main() -> Result<()> {
    let config = Config::load()?;
    match std::env::args().nth(1).as_deref() {
        Some("setup-link") => {
            if config.state.join("app.json").exists() {
                anyhow::bail!("the bot is already set up");
            }
            let token = random_token()?;
            save(&config.state.join("setup-token.json"), &token)?;
            println!("{}/setup?token={}", config.public_url, token.token);
            return Ok(());
        }
        Some(_) => anyhow::bail!("usage: framecorder-report [setup-link]"),
        None => {}
    }
    let listener =
        TcpListener::bind(("127.0.0.1", config.port)).context("opening the report service port")?;
    eprintln!("framecorder-report listening on {}", listener.local_addr()?);
    let service = Arc::new(Service {
        github: Mutex::new(GitHub::new(config.clone())),
        config,
        limits: Mutex::new(Limits::default()),
        setup: Mutex::new(()),
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
