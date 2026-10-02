//! Talking to framecorder-sync on the Frame.

use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use super::tls::{client_config, PinnedVerifier};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The Frame sends a keepalive every 25 s, so this much silence means the
/// connection is dead even if TCP hasn't noticed yet.
const EVENTS_IDLE: Duration = Duration::from_secs(60);
const DOWNLOAD_IDLE: Duration = Duration::from_secs(30);

/// How the headset itself is doing.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct About {
    pub battery: Option<Battery>,
    pub storage: Option<Storage>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Battery {
    pub percent: u8,
    /// On the charger, charging or full.
    pub charging: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Storage {
    pub free: u64,
    pub total: u64,
    /// How much of it framecorder's videos take.
    pub videos: u64,
}

/// What the Frame's dashboard tab is up to, for a remote.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Remote {
    /// The tab's running; it starts with SteamVR.
    pub available: bool,
    /// Its setup's been done, so it can record.
    pub ready: bool,
    pub recording: bool,
    /// Recording and not paused (it pauses while the tab's on screen).
    pub running: bool,
    pub elapsed_ms: u64,
    /// The clip length while clipping's on.
    pub clips: Option<u32>,
    /// A clip can be saved right now.
    pub clip_ready: bool,
}

/// The Frame's recording settings. They apply from its next recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// wide, square, tall or both
    pub shape: String,
    /// standard, high or max
    pub quality: String,
    /// auto, 60 or 30
    pub fps: String,
    pub game_audio: bool,
    pub mic: bool,
    /// Whether it keeps a replay buffer to save clips from.
    pub clips: bool,
    /// How long a clip is, in seconds: 15, 30, 60 or 120.
    pub clip: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteClip {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub duration_s: Option<f64>,
    pub created: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hello {
    pub name: String,
    pub version: u32,
    pub fingerprint: String,
}

/// What the Frame says about framecorder updates.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct UpdateStatus {
    /// The version on the Frame.
    pub installed: String,
    /// The newest one, when the release says (older ones didn't).
    pub latest: Option<String>,
    /// There's something newer than what's installed.
    pub available: bool,
    /// An update's being installed right now.
    #[serde(default)]
    pub updating: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Paired {
    pub token: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// Couldn't talk to it at all: off, asleep, other network, not running.
    Unreachable(String),
    /// Something answered, but with a different certificate than we paired with.
    WrongFingerprint,
    /// Our token isn't accepted anymore (removed on the Frame).
    Unauthorized,
    /// The Frame said no, with its reason.
    Refused(u16, String),
    /// This device doesn't have the space for a file.
    Full { need: u64, free: u64 },
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unreachable(e) => write!(f, "can't reach the frame ({e})"),
            ApiError::WrongFingerprint => write!(f, "something answered, but it isn't the frame you paired with"),
            ApiError::Unauthorized => write!(f, "the frame doesn't know this device anymore, pair again"),
            ApiError::Refused(_, msg) => write!(f, "{msg}"),
            ApiError::Full { need, free } => {
                write!(f, "needs {} free, there's {}", super::space::human(*need), super::space::human(*free))
            }
            ApiError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ApiError {}

pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    verifier: Arc<PinnedVerifier>,
}

impl Client {
    /// A client that only talks to the Frame with this fingerprint.
    pub fn pinned(addr: &str, fingerprint: &str) -> Result<Self, ApiError> {
        Self::build(addr, PinnedVerifier::pinned(fingerprint))
    }

    /// Accepts whatever certificate is there and tells you its fingerprint
    /// afterwards. Only for pairing with a Frame typed in by address.
    pub fn capture(addr: &str) -> Result<Self, ApiError> {
        Self::build(addr, PinnedVerifier::capture())
    }

    fn build(addr: &str, verifier: Arc<PinnedVerifier>) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(client_config(verifier.clone()))
            .connect_timeout(CONNECT_TIMEOUT)
            .no_proxy()
            .http1_only()
            .tcp_nodelay(true)
            .build()
            .map_err(|e| ApiError::Other(e.to_string()))?;
        Ok(Self { http, base: format!("https://{addr}"), token: None, verifier })
    }

    pub fn with_token(mut self, token: &str) -> Self {
        self.token = Some(token.to_string());
        self
    }

    pub fn seen_fingerprint(&self) -> Option<String> {
        self.verifier.seen()
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let rb = self.http.request(method, format!("{}{path}", self.base));
        match &self.token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response, ApiError> {
        let resp = rb.send().await.map_err(|e| self.classify(e))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }
        let code = status.as_u16();
        let msg = resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| format!("the frame said {status}"));
        Err(ApiError::Refused(code, msg))
    }

    fn classify(&self, e: reqwest::Error) -> ApiError {
        let text = error_chain(&e);
        if text.contains("ApplicationVerificationFailure") || text.contains("application verification failure") {
            return ApiError::WrongFingerprint;
        }
        if e.is_timeout() {
            return ApiError::Unreachable("timed out".into());
        }
        if e.is_connect() || e.is_request() {
            return ApiError::Unreachable(short_cause(&e));
        }
        ApiError::Other(text)
    }

    pub async fn hello(&self) -> Result<Hello, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/hello").timeout(REQUEST_TIMEOUT);
        self.send(rb).await?.json().await.map_err(|e| ApiError::Other(e.to_string()))
    }

    pub async fn pair(&self, code: &str, device_name: &str) -> Result<Paired, ApiError> {
        let body = serde_json::json!({ "code": code, "device_name": device_name });
        let rb = self.request(reqwest::Method::POST, "/pair").json(&body).timeout(REQUEST_TIMEOUT);
        self.send(rb).await?.json().await.map_err(|e| ApiError::Other(e.to_string()))
    }

    pub async fn clips(&self) -> Result<Vec<RemoteClip>, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/clips").timeout(REQUEST_TIMEOUT);
        self.send(rb).await?.json().await.map_err(|e| ApiError::Other(e.to_string()))
    }

    /// Asks the Frame to delete a clip we've safely got. It only does if
    /// "delete after sync" is on over there, otherwise this is a no.
    pub async fn delete(&self, id: &str) -> Result<bool, ApiError> {
        let rb = self.request(reqwest::Method::DELETE, &format!("/clips/{id}")).timeout(REQUEST_TIMEOUT);
        match self.send(rb).await {
            Ok(_) => Ok(true),
            Err(ApiError::Refused(403, _)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Whether there's a framecorder update for the Frame. None from Frames
    /// older than this, which don't say.
    pub async fn update_status(&self) -> Result<Option<UpdateStatus>, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/update").timeout(REQUEST_TIMEOUT);
        match self.send(rb).await {
            Ok(resp) => resp.json().await.map(Some).map_err(|e| ApiError::Other(e.to_string())),
            Err(ApiError::Refused(404, _)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Has the Frame install its update now, rather than within a few hours.
    pub async fn start_update(&self) -> Result<(), ApiError> {
        let rb = self.request(reqwest::Method::POST, "/update").timeout(REQUEST_TIMEOUT);
        self.send(rb).await.map(|_| ())
    }

    /// `None` from a Frame whose framecorder is older than these.
    pub async fn recording(&self) -> Result<Option<Recording>, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/recording").timeout(REQUEST_TIMEOUT);
        match self.send(rb).await {
            Ok(r) => r.json().await.map(Some).map_err(|e| ApiError::Other(e.to_string())),
            Err(ApiError::Refused(404, _)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn set_recording(&self, settings: &Recording) -> Result<Recording, ApiError> {
        let rb = self.request(reqwest::Method::PATCH, "/recording").json(settings).timeout(REQUEST_TIMEOUT);
        self.send(rb).await?.json().await.map_err(|e| ApiError::Other(e.to_string()))
    }

    /// `None` from a Frame whose framecorder can't take commands yet.
    pub async fn remote(&self) -> Result<Option<Remote>, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/remote").timeout(REQUEST_TIMEOUT);
        match self.send(rb).await {
            Ok(r) => r.json().await.map(Some).map_err(|e| ApiError::Other(e.to_string())),
            Err(ApiError::Refused(404, _)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Asks the Frame to `record`, `stop` or `clip`, and says how it went.
    pub async fn command(&self, what: &str) -> Result<Remote, ApiError> {
        let rb = self.request(reqwest::Method::POST, "/remote").json(&serde_json::json!({ "do": what })).timeout(REQUEST_TIMEOUT);
        self.send(rb).await?.json().await.map_err(|e| ApiError::Other(e.to_string()))
    }

    pub async fn events(&self) -> Result<Events, ApiError> {
        let rb = self.request(reqwest::Method::GET, "/events").header("Accept", "text/event-stream");
        let resp = self.send(rb).await?;
        Ok(Events { body: Box::pin(resp.bytes_stream()), parser: SseParser::default(), pending: Vec::new() })
    }

    /// Downloads `clip` into `part`, picking up where an earlier try left off.
    /// `progress` gets (bytes so far, total).
    pub async fn download(
        &self,
        clip: &RemoteClip,
        part: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(), ApiError> {
        let size = clip.size;
        let io = move |e: std::io::Error| {
            if super::space::is_full(&e) {
                // what's here stays, to pick up from once there's room
                let free = super::space::free(part).unwrap_or(0);
                return ApiError::Full { need: size, free };
            }
            ApiError::Other(format!("couldn't write {}: {e}", part.display()))
        };
        let mut have = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
        if have > clip.size {
            have = 0;
        }
        if have == clip.size && have > 0 {
            return Ok(());
        }

        let mut rb = self.request(reqwest::Method::GET, &format!("/clips/{}/file", clip.id));
        if have > 0 {
            rb = rb.header("Range", format!("bytes={have}-"));
        }
        let resp = match self.send(rb).await {
            Err(ApiError::Refused(416, _)) => {
                // what we have doesn't fit the file anymore, start over
                tokio::fs::remove_file(part).await.ok();
                return Err(ApiError::Other("partial download didn't match, restarting".into()));
            }
            other => other?,
        };

        let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if resumed {
            let range = resp.headers().get("content-range").and_then(|v| v.to_str().ok()).unwrap_or("");
            if content_range_start(range) != Some((have, clip.size)) {
                tokio::fs::remove_file(part).await.ok();
                return Err(ApiError::Other(format!("unexpected range {range:?}, restarting")));
            }
        } else {
            have = 0;
        }
        if let Some(dir) = part.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(io)?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)
            .await
            .map_err(io)?;
        if resumed {
            log::info!("resuming {} at {have} of {}", clip.name, clip.size);
        }

        let mut body = resp.bytes_stream();
        progress(have, clip.size);
        let mut last = std::time::Instant::now();
        loop {
            let chunk = match tokio::time::timeout(DOWNLOAD_IDLE, body.next()).await {
                Err(_) => return Err(ApiError::Unreachable("download stalled".into())),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(self.classify(e)),
                Ok(Some(Ok(c))) => c,
            };
            file.write_all(&chunk).await.map_err(io)?;
            have += chunk.len() as u64;
            if last.elapsed() > Duration::from_millis(250) {
                progress(have, clip.size);
                last = std::time::Instant::now();
            }
        }
        file.flush().await.map_err(io)?;
        file.sync_all().await.map_err(io)?;
        progress(have, clip.size);
        if have != clip.size {
            return Err(ApiError::Unreachable(format!("got {have} of {} bytes", clip.size)));
        }
        Ok(())
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        out.push_str(": ");
        out.push_str(&s.to_string());
        src = s.source();
    }
    out
}

/// Just the bottom of the error chain, like "connection refused", which is
/// all anyone looking at the app wants to know.
fn short_cause(e: &dyn std::error::Error) -> String {
    let mut last = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        last = s.to_string();
        src = s.source();
    }
    let last = match last.find(" (os error") {
        Some(i) => &last[..i],
        None => &last,
    };
    last.trim().to_lowercase()
}

/// `bytes 100-199/1000` -> (100, 1000)
fn content_range_start(v: &str) -> Option<(u64, u64)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()?))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: String,
    event: String,
    data: Vec<String>,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() || !self.event.is_empty() {
                    let event = std::mem::take(&mut self.event);
                    out.push(SseEvent {
                        event: if event.is_empty() { "message".into() } else { event },
                        data: std::mem::take(&mut self.data).join("\n"),
                    });
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => self.event = value.to_string(),
                "data" => self.data.push(value.to_string()),
                _ => {}
            }
        }
        out
    }
}

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>;

pub struct Events {
    body: ByteStream,
    parser: SseParser,
    pending: Vec<SseEvent>,
}

impl Events {
    /// The next event, or an error once the stream is dead or goes quiet.
    pub async fn next(&mut self) -> Result<SseEvent, ApiError> {
        loop {
            if !self.pending.is_empty() {
                return Ok(self.pending.remove(0));
            }
            match tokio::time::timeout(EVENTS_IDLE, self.body.next()).await {
                Err(_) => return Err(ApiError::Unreachable("the frame went quiet".into())),
                Ok(None) => return Err(ApiError::Unreachable("the frame closed the connection".into())),
                Ok(Some(Err(e))) => return Err(ApiError::Unreachable(e.to_string())),
                Ok(Some(Ok(chunk))) => self.pending = self.parser.feed(&chunk),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_across_chunks() {
        let mut p = SseParser::default();
        assert!(p.feed(b"retry: 3000\n\n: keepalive\n\nevent: ne").is_empty());
        let got = p.feed(b"w\ndata: {\"id\":\"c-x\"}\r\n\r\nevent: removed\ndata: {\"id\":\"c-y\"}\n\n");
        assert_eq!(
            got,
            vec![
                SseEvent { event: "new".into(), data: "{\"id\":\"c-x\"}".into() },
                SseEvent { event: "removed".into(), data: "{\"id\":\"c-y\"}".into() },
            ]
        );
        assert_eq!(p.feed(b"data: a\ndata: b\n\n"), vec![SseEvent { event: "message".into(), data: "a\nb".into() }]);
    }

    #[test]
    fn content_range() {
        assert_eq!(content_range_start("bytes 100-199/1000"), Some((100, 1000)));
        assert_eq!(content_range_start("bytes */1000"), None);
        assert_eq!(content_range_start("nope"), None);
    }
}
