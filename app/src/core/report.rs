use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::api::{ApiError, Client};
use super::engine::Engine;

const RELAY: &str = "https://framecorder-report.coah80.com/v1/report";
static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub os: String,
    pub device: String,
    pub arch: String,
}

impl AppInfo {
    pub fn current(name: &str, version: &str) -> Self {
        #[cfg(unix)]
        let os = std::process::Command::new("uname").arg("-sr").output().ok();
        #[cfg(windows)]
        let os = std::process::Command::new("cmd").args(["/C", "ver"]).output().ok();
        #[cfg(not(any(unix, windows)))]
        let os: Option<std::process::Output> = None;
        let os = os
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| std::env::consts::OS.into());
        #[cfg(target_os = "linux")]
        let os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
            })
            .unwrap_or(os);
        Self {
            name: name.into(),
            version: version.into(),
            os,
            device: "unknown".into(),
            arch: std::env::consts::ARCH.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportLog {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub source: String,
    pub description: String,
    pub app: AppInfo,
    pub headset: Option<Value>,
    pub headset_unavailable: Option<String>,
    pub app_logs: Vec<ReportLog>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sent {
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct ReportError(pub String);
impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ReportError {}

struct RecentLogger(Box<dyn log::Log>);
impl log::Log for RecentLogger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        self.0.enabled(m)
    }
    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            let text = redact(&format!("{} {} {}: {}", super::engine::now_unix(), r.level(), r.target(), r.args()));
            if let Ok(mut lines) = LINES.lock() {
                for line in text.lines() {
                    let mut line = line.to_string();
                    truncate_end(&mut line, 2048);
                    lines.push_back(line);
                    while lines.len() > 500 {
                        lines.pop_front();
                    }
                }
            }
            self.0.log(r);
        }
    }
    fn flush(&self) {
        self.0.flush();
    }
}

pub fn init_logger(logger: Box<dyn log::Log>, level: log::LevelFilter) -> Result<(), log::SetLoggerError> {
    log::set_boxed_logger(Box::new(RecentLogger(logger)))?;
    log::set_max_level(level);
    Ok(())
}

fn truncate_end(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut start = text.len() - limit;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        *text = text[start..].into();
    }
}

#[path = "../../../report/src/shared.rs"]
mod shared;

pub fn redact(text: &str) -> String {
    static PATTERNS: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    let (secret, home, mac) = PATTERNS.get_or_init(|| (
        Regex::new(r"(?i)token|bearer|password|pairing.?code|certificate|private.?key|serial|LHR-[A-Z0-9]+|-----BEGIN|\b[0-9]{6}\b|\b[a-f0-9]{32,}\b|^[A-Za-z0-9+/=]{64,}$").expect("secret pattern"),
        Regex::new(r"(?i)(?:/home/|/Users/|[A-Z]:[\\/]Users[\\/])[^/\\\s]+|/root\b|/data/user/\d+/[^/\s]+").expect("home pattern"),
        Regex::new(r"(?i)\b[0-9a-f]{2}(?:[:-][0-9a-f]{2}){5}\b").expect("mac pattern"),
    ));
    static HOME: OnceLock<Option<String>> = OnceLock::new();
    static USERS: OnceLock<Vec<Regex>> = OnceLock::new();
    let home_dir = HOME.get_or_init(|| dirs::home_dir().map(|p| p.to_string_lossy().into_owned()));
    let users = USERS.get_or_init(|| {
        ["USER", "USERNAME", "LOGNAME"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .filter(|u| u.len() >= 2)
            .filter_map(|user| Regex::new(&format!(r"\b{}\b", regex::escape(&user))).ok())
            .collect()
    });
    let mut out = Vec::new();
    for line in text.lines() {
        if secret.is_match(line) {
            out.push("<redacted>".into());
            continue;
        }
        let mut line = line.to_string();
        if let Some(home) = &home_dir {
            line = line.replace(home, "~");
        }
        line = home.replace_all(&line, "~").into_owned();
        for pattern in users {
            line = pattern.replace_all(&line, "<user>").into_owned();
        }
        line = mac.replace_all(&line, "<mac>").into_owned();
        out.push(shared::hide(&line));
    }
    out.join("\n")
}

fn scrub(value: &mut Value) {
    match value {
        Value::String(s) => *s = redact(s),
        Value::Array(values) => values.iter_mut().for_each(scrub),
        Value::Object(map) => {
            map.retain(|key, _| {
                let k = key.to_ascii_lowercase();
                ![
                    "token",
                    "password",
                    "certificate",
                    "private_key",
                    "pairing_code",
                    "serial",
                    "username",
                    "login",
                ]
                .iter()
                .any(|s| k.contains(s))
            });
            map.values_mut().for_each(scrub);
        }
        _ => {}
    }
}

pub async fn preview(description: &str, app: AppInfo) -> Report {
    preview_with_client(description, app, None).await
}

pub async fn preview_with_client(description: &str, app: AppInfo, client: Option<&Client>) -> Report {
    let (mut headset, unavailable) = match client {
        None => (None, Some("no headset is paired".into())),
        Some(client) => match tokio::time::timeout(Duration::from_secs(4), client.diagnostics()).await {
            Ok(Ok(Some(d))) => (Some(d), None),
            Ok(Ok(None)) => {
                let fallback = tokio::time::timeout(Duration::from_secs(2), async {
                    let hello = client.hello().await.ok();
                    let frame = client.frame().await.ok();
                    json!({"sync_protocol": hello.map(|h| h.version), "frame": frame})
                })
                .await
                .ok();
                (fallback, Some("the headset runs an older framecorder".into()))
            }
            Ok(Err(ApiError::Unauthorized)) => (None, Some("the headset no longer accepts this pairing; pair again".into())),
            Ok(Err(ApiError::WrongFingerprint)) => (None, Some("the headset's identity changed; pair again".into())),
            _ => (
                None,
                Some("the headset could not be reached; check it is awake and on the same Wi-Fi".into()),
            ),
        },
    };
    if let Some(h) = &mut headset {
        scrub(h);
        // older headsets send the Steam name, it doesn't go on a public issue
        if let Some(steam) = h.get_mut("steam").and_then(Value::as_object_mut) {
            steam.remove("name");
        }
    }
    let text = LINES
        .lock()
        .map(|l| l.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    let mut text = redact(&text);
    truncate_end(&mut text, 48 * 1024);
    Report {
        version: 1,
        source: if app.name.to_ascii_lowercase().contains("android") {
            "android"
        } else {
            "desktop"
        }
        .into(),
        description: redact(description),
        app: AppInfo {
            name: redact(&app.name),
            version: redact(&app.version),
            os: redact(&app.os),
            device: redact(&app.device),
            arch: redact(&app.arch),
        },
        headset,
        headset_unavailable: unavailable,
        app_logs: vec![ReportLog {
            name: "app.log".into(),
            text,
        }],
    }
}

impl Engine {
    pub async fn report_preview(&self, description: &str, app: AppInfo) -> Report {
        let statuses = self.statuses();
        let host = self.hosts().into_iter().next();
        let client = host.as_ref().and_then(|h| {
            let addr = statuses
                .iter()
                .find(|s| s.fingerprint == h.fingerprint)
                .map(|s| s.addr.as_str())
                .unwrap_or(&h.addr);
            Client::pinned(addr, &h.fingerprint, self.app())
                .ok()
                .map(|c| c.with_token(&h.token))
        });
        let mut report = preview_with_client(description, app, client.as_ref()).await;
        let status: Vec<_> = statuses
            .iter()
            .map(|s| json!({"address": redact(&s.addr), "state": s.state, "last_error": s.message.as_deref().map(redact)}))
            .collect();
        report.app_logs.push(ReportLog {
            name: "sync-state.json".into(),
            text: json!({"paired": self.hosts().len(), "headsets": status, "downloads_free_bytes": self.report_free_space()}).to_string(),
        });
        // what this device is called and what it paired with can carry
        // someone's name ("Cole's S25 Ultra")
        let names: Vec<String> = self.hosts().into_iter().map(|h| h.name).chain([self.device_name.clone(), super::device_name()]).collect();
        hide_names(&mut report, &names);
        report
    }
}

fn hide_names(report: &mut Report, names: &[String]) {
    fn walk(value: &mut Value, names: &[String]) {
        match value {
            Value::String(s) => *s = shared::hide_names(s, names),
            Value::Array(values) => values.iter_mut().for_each(|v| walk(v, names)),
            Value::Object(map) => map.values_mut().for_each(|v| walk(v, names)),
            _ => {}
        }
    }
    if let Some(h) = &mut report.headset {
        walk(h, names);
    }
    for log in &mut report.app_logs {
        log.text = shared::hide_names(&log.text, names);
    }
    report.app.device = shared::hide_names(&report.app.device, names);
}

pub async fn send(report: &Report) -> Result<Sent, ReportError> {
    let url = std::env::var("FRAMECORDER_REPORT_URL").unwrap_or_else(|_| RELAY.into());
    send_to(report, &url).await
}

async fn send_to(report: &Report, url: &str) -> Result<Sent, ReportError> {
    if report.description.trim().is_empty() || report.description.chars().count() > 4000 {
        return Err(ReportError("describe what went wrong in 1 to 4000 characters".into()));
    }
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ReportError("couldn't prepare the report connection".into()))?;
    let response = client
        .post(url)
        .header(
            reqwest::header::USER_AGENT,
            format!("framecorder-{}/{}", report.source, report.app.version),
        )
        .json(report)
        .send()
        .await
        .map_err(|_| ReportError("couldn't send the report; check your internet connection and try again".into()))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| ReportError("the report service sent an unreadable reply; try again later".into()))?;
    if !status.is_success() || value.get("ok") != Some(&Value::Bool(true)) {
        return Err(ReportError(
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the report service couldn't accept this report; try again later")
                .into(),
        ));
    }
    serde_json::from_value(value).map_err(|_| ReportError("the report service didn't return an issue link; try again later".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Read, Write};

    #[test]
    fn recent_logger_keeps_a_bounded_redacted_tail_and_forwards_every_record() {
        use log::Log;
        struct Forward(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl log::Log for Forward {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, _: &log::Record<'_>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            fn flush(&self) {}
        }
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let logger = RecentLogger(Box::new(Forward(count.clone())));
        for n in 0..510 {
            logger.log(
                &log::Record::builder()
                    .level(log::Level::Warn)
                    .target("sync")
                    .args(format_args!("line {n} at /home/someone/download via 8.8.8.8"))
                    .build(),
            );
        }
        let lines = LINES.lock().unwrap();
        assert_eq!(lines.len(), 500);
        assert!(!lines.front().unwrap().contains("line 0 "));
        assert!(lines.back().unwrap().contains("line 509 "));
        assert!(lines.iter().all(|l| !l.contains("someone") && !l.contains("8.8.8.8")));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 510);
    }

    #[tokio::test]
    async fn relay_rejects_a_self_signed_certificate() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert.cert.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())),
            )
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("https://localhost:{}/v1/report", listener.local_addr().unwrap().port());
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut connection = rustls::ServerConnection::new(std::sync::Arc::new(config)).unwrap();
            assert!(connection.complete_io(&mut stream).is_err());
        });
        let report = preview("broken", AppInfo::current("framecorder desktop", "test")).await;
        assert!(send_to(&report, &url).await.is_err());
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn relay_receives_the_preview_and_returns_plain_errors_or_an_issue_link() {
        for (status, body) in [
            (429, r#"{"ok":false,"error":"please wait before sending another report"}"#),
            (
                200,
                r#"{"ok":true,"number":42,"url":"https://github.com/coah80/framecorder/issues/42"}"#,
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/v1/report", listener.local_addr().unwrap());
            let report = preview("broken", AppInfo::current("framecorder desktop", "test")).await;
            let expected = serde_json::to_value(&report).unwrap();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("POST /v1/report "));
                let mut size = 0;
                let mut headers = String::new();
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line.to_ascii_lowercase());
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        size = n.trim().parse().unwrap();
                    }
                }
                assert!(headers.contains("user-agent: framecorder-desktop/test"));
                let mut bytes = vec![0; size];
                reader.read_exact(&mut bytes).unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), expected);
                write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let result = send_to(&report, &url).await;
            thread.join().unwrap();
            if status == 200 {
                assert_eq!(result.unwrap().number, 42);
            } else {
                assert_eq!(result.unwrap_err().0, "please wait before sending another report");
            }
        }
    }
}
