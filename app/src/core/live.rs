use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, watch};
use tokio::task::{JoinHandle, JoinSet};

use super::api::{ApiError, Client};

pub const DEFAULT_PORT: u16 = 38620;
const QUEUE: usize = 64;
const CHUNK: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LivePhase {
    Connecting,
    Live,
    Stopped,
    Error,
}

#[derive(Clone, Debug)]
pub struct LiveStatus {
    pub phase: LivePhase,
    pub url: String,
    pub bytes_per_second: u64,
    pub viewers: u32,
    pub error: Option<String>,
}

pub struct LiveRelay {
    state: watch::Sender<LiveStatus>,
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl LiveRelay {
    pub async fn start(client: Client) -> Result<Self, ApiError> {
        let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, DEFAULT_PORT)).await
        {
            Ok(l) => l,
            Err(_) => TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .map_err(local_error)?,
        };
        let port = listener.local_addr().map_err(local_error)?.port();
        let (status, _) = watch::channel(LiveStatus {
            phase: LivePhase::Connecting,
            url: format!("http://127.0.0.1:{port}/live.ts"),
            bytes_per_second: 0,
            viewers: 0,
            error: None,
        });
        let state = status.clone();
        let (cancel, mut cancelled) = watch::channel(false);
        let task = tokio::spawn(async move {
            let (data, _) = broadcast::channel::<Option<Bytes>>(QUEUE);
            let mut players = JoinSet::new();
            let upstream = pull(client, data.clone(), status.clone());
            tokio::pin!(upstream);
            loop {
                tokio::select! {
                    _ = cancelled.changed() => break,
                    _ = &mut upstream => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((socket, _)) => {
                                players.spawn(serve(socket, data.subscribe(), status.clone()));
                            }
                            Err(e) => {
                                status.send_modify(|s| { s.phase = LivePhase::Error; s.error = Some(format!("can't accept a local player: {e}")); });
                                break;
                            }
                        }
                    }
                    _ = players.join_next(), if !players.is_empty() => {}
                }
            }
            players.abort_all();
            while players.join_next().await.is_some() {}
            status.send_modify(|s| {
                s.viewers = 0;
                s.bytes_per_second = 0;
                if *cancelled.borrow() {
                    s.phase = LivePhase::Stopped;
                    s.error = None;
                }
            });
        });
        Ok(Self {
            state,
            cancel,
            task: Some(task),
        })
    }

    pub fn status(&self) -> LiveStatus {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<LiveStatus> {
        self.state.subscribe()
    }

    pub async fn stop(&mut self) {
        let _ = self.cancel.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.state.send_modify(|s| {
            s.phase = LivePhase::Stopped;
            s.error = None;
            s.viewers = 0;
            s.bytes_per_second = 0;
        });
    }
}

impl Drop for LiveRelay {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

fn local_error(e: std::io::Error) -> ApiError {
    ApiError::Other(format!("can't open live view on this device: {e}"))
}

async fn pull(
    client: Client,
    data: broadcast::Sender<Option<Bytes>>,
    state: watch::Sender<LiveStatus>,
) {
    loop {
        state.send_modify(|s| {
            s.phase = LivePhase::Connecting;
            s.bytes_per_second = 0;
        });
        let result = async {
            let mut stream = client.live().await?;
            let mut bytes = 0;
            let mut since = tokio::time::Instant::now();
            loop {
                let next = tokio::time::timeout(Duration::from_secs(30), stream.next())
                    .await
                    .map_err(|_| {
                        ApiError::Unreachable(
                            "live view stopped sending video; reconnecting".into(),
                        )
                    })?;
                let chunk = next.ok_or_else(|| {
                    ApiError::Unreachable("live view disconnected; reconnecting".into())
                })??;
                bytes += chunk.len() as u64;
                for part in chunk.chunks(CHUNK) {
                    let _ = data.send(Some(Bytes::copy_from_slice(part)));
                }
                let elapsed = since.elapsed().as_secs_f64();
                state.send_modify(|s| {
                    s.phase = LivePhase::Live;
                    s.error = None;
                    if elapsed >= 1.0 {
                        s.bytes_per_second = (bytes as f64 / elapsed) as u64;
                    }
                });
                if elapsed >= 1.0 {
                    bytes = 0;
                    since = tokio::time::Instant::now();
                }
            }
        }
        .await;
        let Err(e): Result<(), ApiError> = result else {
            return;
        };
        let _ = data.send(None);
        state.send_modify(|s| {
            s.phase = LivePhase::Error;
            s.error = Some(e.to_string());
            s.bytes_per_second = 0;
        });
        if matches!(
            e,
            ApiError::Refused(404, _) | ApiError::Unauthorized | ApiError::WrongFingerprint
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

struct Viewer(watch::Sender<LiveStatus>);
impl Drop for Viewer {
    fn drop(&mut self) {
        self.0
            .send_modify(|s| s.viewers = s.viewers.saturating_sub(1));
    }
}

async fn serve(
    mut socket: TcpStream,
    mut data: broadcast::Receiver<Option<Bytes>>,
    state: watch::Sender<LiveStatus>,
) {
    let request = async {
        let mut header = Vec::new();
        while header.len() < 8192 {
            header.push(socket.read_u8().await?);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        Ok::<_, std::io::Error>(header)
    };
    let Ok(Ok(header)) = tokio::time::timeout(Duration::from_secs(5), request).await else {
        return;
    };
    if !header.starts_with(b"GET /live.ts HTTP/1.") || !header.ends_with(b"\r\n\r\n") {
        let _ = socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    }
    if socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: video/mp2t\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").await.is_err() { return; }
    state.send_modify(|s| s.viewers += 1);
    let _viewer = Viewer(state);
    loop {
        tokio::select! {
            next = data.recv() => {
                let Ok(Some(chunk)) = next else { break };
                if !matches!(tokio::time::timeout(Duration::from_secs(2), socket.write_all(&chunk)).await, Ok(Ok(()))) { break; }
            }
            _ = socket.read_u8() => break,
        }
    }
}

pub struct LiveController {
    relay: tokio::sync::Mutex<Option<LiveRelay>>,
    status: Mutex<Option<watch::Receiver<LiveStatus>>>,
}

impl Default for LiveController {
    fn default() -> Self {
        Self {
            relay: tokio::sync::Mutex::new(None),
            status: Mutex::new(None),
        }
    }
}

impl LiveController {
    pub async fn start(&self, client: Client) -> Result<LiveStatus, ApiError> {
        let mut slot = self.relay.lock().await;
        if let Some(mut old) = slot.take() {
            old.stop().await;
        }
        let relay = LiveRelay::start(client).await?;
        let status = relay.status();
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = Some(relay.subscribe());
        *slot = Some(relay);
        Ok(status)
    }

    pub async fn stop(&self) {
        if let Some(mut relay) = self.relay.lock().await.take() {
            relay.stop().await;
        }
    }

    pub fn status(&self) -> Option<LiveStatus> {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.borrow().clone())
    }
}
