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
/// Pieces held for players: one per read from the headset, a few seconds of
/// video at the highest bitrate.
const QUEUE: usize = 512;
const TS: usize = 188;

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
            let (data, _) = broadcast::channel::<Option<(Bytes, bool)>>(QUEUE);
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
    data: broadcast::Sender<Option<(Bytes, bool)>>,
    state: watch::Sender<LiveStatus>,
) {
    loop {
        state.send_modify(|s| {
            s.phase = LivePhase::Connecting;
            s.bytes_per_second = 0;
        });
        let result = async {
            let mut stream = client.live().await?;
            let mut keyframes = Keyframes::default();
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
                keyframes.push(&chunk, &mut |piece, key| {
                    let _ = data.send(Some((piece, key)));
                });
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
    mut data: broadcast::Receiver<Option<(Bytes, bool)>>,
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
    // players start on a keyframe, and one that falls behind (OBS while it opens
    // its decoder) skips to the next keyframe rather than being hung up on
    let mut waiting = true;
    loop {
        tokio::select! {
            next = data.recv() => {
                let chunk = match next {
                    Ok(Some((chunk, key))) if key || !waiting => chunk,
                    Ok(Some(_)) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        waiting = true;
                        continue;
                    }
                    _ => break,
                };
                waiting = false;
                if !matches!(tokio::time::timeout(Duration::from_secs(2), socket.write_all(&chunk)).await, Ok(Ok(()))) { break; }
            }
            _ = socket.read_u8() => break,
        }
    }
}

/// Cuts the headset's MPEG-TS where a player can start: at each video
/// keyframe, along with the tables (PAT, PMT, SDT) the muxer writes just
/// before it.
/// Audio packets carry the same random access flag, so the PMT says which
/// PID is the video.
#[derive(Default)]
struct Keyframes {
    partial: Vec<u8>,
    pmt: Option<u16>,
    video: Option<u16>,
    pending: Vec<u8>,
    /// where a run of tables starts in `pending`, while nothing else has come after it
    tables: Option<usize>,
    key: bool,
}

impl Keyframes {
    fn push(&mut self, data: &[u8], out: &mut impl FnMut(Bytes, bool)) {
        let mut buf = std::mem::take(&mut self.partial);
        buf.extend_from_slice(data);
        let mut at = 0;
        while buf.len() - at >= TS {
            if buf[at] != 0x47 {
                at += 1;
                continue;
            }
            self.packet(&buf[at..at + TS], out);
            at += TS;
        }
        buf.drain(..at);
        self.partial = buf;
        let ready = self.tables.unwrap_or(self.pending.len());
        self.emit(ready, out);
        if self.tables.is_some() {
            self.tables = Some(0);
        }
    }

    fn packet(&mut self, p: &[u8], out: &mut impl FnMut(Bytes, bool)) {
        let pid = u16::from(p[1] & 0x1f) << 8 | u16::from(p[2]);
        let start = p[1] & 0x40 != 0;
        let adaptation = p[3] & 0x20 != 0;
        let payload = if adaptation { 5 + usize::from(p[4]) } else { 4 };
        let table = || start.then(|| section(p, payload)).flatten();
        if pid < 0x20 || Some(pid) == self.pmt {
            if pid == 0 {
                self.pmt = table().and_then(pat_pmt).or(self.pmt);
            } else if Some(pid) == self.pmt {
                self.video = table().and_then(pmt_video).or(self.video);
            }
            self.tables.get_or_insert(self.pending.len());
        } else {
            let random_access = adaptation && p[4] > 0 && p[5] & 0x40 != 0;
            if start && random_access && Some(pid) == self.video {
                let cut = self.tables.unwrap_or(self.pending.len());
                self.emit(cut, out);
                self.key = true;
            }
            self.tables = None;
        }
        self.pending.extend_from_slice(p);
    }

    fn emit(&mut self, upto: usize, out: &mut impl FnMut(Bytes, bool)) {
        if upto > 0 {
            out(Bytes::copy_from_slice(&self.pending[..upto]), std::mem::take(&mut self.key));
            self.pending.drain(..upto);
        }
    }
}

/// The PSI section a packet starts, up to its CRC.
fn section(p: &[u8], payload: usize) -> Option<&[u8]> {
    let s = p.get(payload + 1 + usize::from(*p.get(payload)?)..)?;
    let length = usize::from(s.get(1)? & 0x0f) << 8 | usize::from(*s.get(2)?);
    s.get(..(3 + length).checked_sub(4)?)
}

fn pat_pmt(s: &[u8]) -> Option<u16> {
    if s[0] != 0x00 {
        return None;
    }
    s.get(8..)?
        .chunks_exact(4)
        .find(|e| e[0] != 0 || e[1] != 0)
        .map(|e| u16::from(e[2] & 0x1f) << 8 | u16::from(e[3]))
}

fn pmt_video(s: &[u8]) -> Option<u16> {
    if s[0] != 0x02 {
        return None;
    }
    let mut at = 12 + (usize::from(s.get(10)? & 0x0f) << 8 | usize::from(*s.get(11)?));
    while let Some(e) = s.get(at..at + 5) {
        // h.264 and hevc, what the headset's encoder makes
        if matches!(e[0], 0x1b | 0x24) {
            return Some(u16::from(e[1] & 0x1f) << 8 | u16::from(e[2]));
        }
        at += 5 + (usize::from(e[3] & 0x0f) << 8 | usize::from(e[4]));
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(pid: u16, start: bool, random_access: bool, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![0x47, u8::from(start) << 6 | (pid >> 8) as u8, pid as u8, 0x30, 1, if random_access { 0x40 } else { 0 }];
        p.extend_from_slice(payload);
        p.resize(TS, 0xff);
        p
    }

    fn table(pid: u16, id: u8, body: &[u8]) -> Vec<u8> {
        let length = (5 + body.len() + 4) as u8;
        let mut section = vec![0, id, 0xb0, length, 0, 1, 0xc1, 0, 0];
        section.extend_from_slice(body);
        section.extend_from_slice(&[0; 4]);
        packet(pid, true, false, &section)
    }

    /// The muxer's shape: PAT and PMT before every video keyframe and now and
    /// then before audio, and every AAC packet flagged random access.
    #[test]
    fn players_start_at_the_tables_before_a_video_keyframe() {
        let pat = table(0, 0x00, &[0, 1, 0xf0, 0x00]);
        let pmt = table(0x1000, 0x02, &[0xe1, 0x00, 0xf0, 0x00, 0x24, 0xe1, 0x00, 0xf0, 0x00, 0x0f, 0xe1, 0x01, 0xf0, 0x00]);
        let video = |key| packet(0x100, true, key, &[0, 0, 1, 0xe0]);
        let audio = packet(0x101, true, true, &[0, 0, 1, 0xc0]);
        let sdt = table(0x11, 0x42, &[]);
        let packets = [&sdt, &pat, &pmt, &video(true), &video(false), &audio, &pat, &pmt, &audio, &video(false), &pat, &pmt, &video(true), &audio];
        let stream: Vec<u8> = packets.iter().flat_map(|p| p.iter().copied()).collect();
        for read in [stream.len(), TS, 100, 7] {
            let mut keyframes = Keyframes::default();
            let (mut joined, mut keys) = (Vec::new(), Vec::new());
            for part in stream.chunks(read) {
                keyframes.push(part, &mut |piece, key| {
                    if key {
                        keys.push(joined.len() / TS);
                    }
                    joined.extend_from_slice(&piece);
                });
            }
            assert_eq!(joined, stream, "reads of {read}");
            assert_eq!(keys, [0, 10], "reads of {read}");
        }
    }

    /// OBS stops reading for a moment while it opens its decoder. That used to
    /// hang up on it, and OBS showed nothing.
    #[tokio::test]
    async fn a_player_that_falls_behind_picks_up_at_the_next_keyframe() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut player = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let (data, rx) = broadcast::channel::<Option<(Bytes, bool)>>(QUEUE);
        let (state, _) = watch::channel(LiveStatus { phase: LivePhase::Live, url: String::new(), bytes_per_second: 0, viewers: 0, error: None });
        data.send(Some((Bytes::from_static(b"first"), true))).unwrap();
        for _ in 0..QUEUE {
            data.send(Some((Bytes::from_static(b"behind"), false))).unwrap();
        }
        data.send(Some((Bytes::from_static(b"next"), true))).unwrap();
        data.send(Some((Bytes::from_static(b"after"), false))).unwrap();
        drop(data);
        player.write_all(b"GET /live.ts HTTP/1.1\r\n\r\n").await.unwrap();
        serve(socket, rx, state).await;
        let mut response = Vec::new();
        player.read_to_end(&mut response).await.unwrap();
        assert!(response.ends_with(b"\r\n\r\nnextafter"), "{}", String::from_utf8_lossy(&response));
    }
}
