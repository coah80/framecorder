//! The HTTPS side. One thread per connection, all blocking: there are only
//! ever a few devices, and a thread parked in read() costs nothing.

use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::Deserialize;

use crate::config::{Paths, Settings, PROTOCOL_VERSION};
use crate::devices::{Devices, PairError, Pairing};
use crate::events::{clip_json, Hub};
use crate::http::{self, parse_range, read_request, write_error, write_head, write_json, Range, ReadError, Request};
use crate::library::{Change, Library};
use crate::throttle::Throttle;

const MAX_CONNECTIONS: usize = 32;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
/// Unacknowledged data older than this kills the connection, so streams to
/// a phone that walked out of Wi-Fi range don't hang around for 15 minutes.
const TCP_USER_TIMEOUT_MS: libc::c_uint = 60_000;
const KEEPALIVE: Duration = Duration::from_secs(25);
const CHUNK: usize = 256 * 1024;
/// Drop what we've sent from the page cache every so often, so a big
/// recording doesn't push the game's files out of memory.
const DROP_CACHE_EVERY: u64 = 8 * 1024 * 1024;

pub struct State {
    pub paths: Paths,
    pub name: String,
    pub fingerprint: String,
    pub tls: Arc<ServerConfig>,
    pub library: Arc<Library>,
    pub hub: Arc<Hub>,
    pub devices: Arc<Devices>,
    pub pairing: Pairing,
    pub throttle: Arc<Throttle>,
    pub updates: crate::update::Updates,
    pub connections: AtomicUsize,
}

type Tls = StreamOwned<ServerConnection, TcpStream>;

pub fn serve(listener: TcpListener, state: Arc<State>) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log::warn!("accept failed: {e}");
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if state.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            state.connections.fetch_sub(1, Ordering::SeqCst);
            log::warn!("too many connections, dropping one");
            continue;
        }
        let st = state.clone();
        let spawned = std::thread::Builder::new().name("conn".into()).stack_size(256 * 1024).spawn(move || {
            if let Err(e) = connection(stream, &st) {
                log::debug!("connection ended: {e}");
            }
            st.connections.fetch_sub(1, Ordering::SeqCst);
        });
        if let Err(e) = spawned {
            log::error!("couldn't start a connection thread: {e}");
            state.connections.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn connection(stream: TcpStream, state: &State) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    unsafe {
        let t = TCP_USER_TIMEOUT_MS;
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::IPPROTO_TCP,
            libc::TCP_USER_TIMEOUT,
            (&t as *const libc::c_uint).cast(),
            std::mem::size_of_val(&t) as libc::socklen_t,
        );
    }
    let conn = ServerConnection::new(state.tls.clone()).map_err(io::Error::other)?;
    let mut reader = BufReader::with_capacity(4096, StreamOwned::new(conn, stream));
    loop {
        let req = match read_request(&mut reader) {
            Ok(r) => r,
            Err(ReadError::Closed) => break,
            Err(ReadError::Bad(status)) => {
                let _ = write_error(reader.get_mut(), status, http::reason(status), false);
                break;
            }
        };
        let keep = handle(&req, reader.get_mut(), state)? && req.keep_alive;
        if !keep {
            break;
        }
    }
    let tls = reader.get_mut();
    tls.conn.send_close_notify();
    let _ = tls.flush();
    Ok(())
}

/// Returns whether the connection can take another request.
fn handle(req: &Request, w: &mut Tls, state: &State) -> io::Result<bool> {
    let path = req.path.split('?').next().unwrap_or("");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let keep = req.keep_alive;
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["hello"]) => {
            let body = serde_json::json!({
                "name": state.name,
                "version": PROTOCOL_VERSION,
                "fingerprint": state.fingerprint,
            });
            write_json(w, 200, &body.to_string(), keep)?;
            return Ok(true);
        }
        ("POST", ["pair"]) => {
            pair(req, w, state)?;
            return Ok(true);
        }
        _ => {}
    }

    let Some(device) = req.bearer().and_then(|t| state.devices.check(t)) else {
        write_head(
            w,
            401,
            &[("WWW-Authenticate", "Bearer".into()), ("Content-Length", "0".into())],
            keep,
        )?;
        w.flush()?;
        return Ok(true);
    };

    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["clips"]) => {
            let list: Vec<String> = state.library.list().iter().map(clip_json).collect();
            write_json(w, 200, &format!("[{}]", list.join(",")), keep)?;
            Ok(true)
        }
        ("GET" | "HEAD", ["clips", id, "file"]) => send_file(req, w, state, id),
        ("DELETE", ["clips", id]) => {
            delete(w, state, id, keep)?;
            Ok(true)
        }
        ("GET", ["update"]) => {
            match state.updates.status() {
                Ok(status) => write_json(w, 200, &status.to_string(), keep)?,
                Err(e) => write_json(w, 502, &serde_json::json!({ "error": e }).to_string(), keep)?,
            }
            Ok(true)
        }
        ("POST", ["update"]) => {
            log::info!("{device} asked for an update");
            match state.updates.start() {
                Ok(()) => write_json(w, 202, r#"{"started":true}"#, keep)?,
                Err(e) => write_json(w, 500, &serde_json::json!({ "error": e }).to_string(), keep)?,
            }
            Ok(true)
        }
        ("GET", ["events"]) => {
            events(w, state, &device, req.bearer().unwrap_or(""))?;
            Ok(false)
        }
        (_, ["hello" | "pair" | "clips" | "events" | "update", ..]) => {
            write_error(w, 405, "method not allowed", keep)?;
            Ok(true)
        }
        _ => {
            write_error(w, 404, "not found", keep)?;
            Ok(true)
        }
    }
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
    #[serde(default)]
    device_name: String,
}

fn pair(req: &Request, w: &mut Tls, state: &State) -> io::Result<()> {
    let keep = req.keep_alive;
    let Ok(body) = serde_json::from_slice::<PairBody>(&req.body) else {
        return write_error(w, 400, "expected {\"code\", \"device_name\"}", keep);
    };
    match state.pairing.pair(&body.code, &body.device_name, &state.devices) {
        Ok(p) => {
            log::info!("paired {:?} as {}", body.device_name, p.device_id);
            let out = serde_json::json!({ "token": p.token, "device_id": p.device_id });
            write_json(w, 200, &out.to_string(), keep)
        }
        Err(PairError::Limited(secs)) => {
            let body = serde_json::json!({ "error": "too many wrong codes, wait a minute" }).to_string();
            write_head(
                w,
                429,
                &[
                    ("Retry-After", secs.to_string()),
                    ("Content-Type", "application/json".into()),
                    ("Content-Length", body.len().to_string()),
                ],
                keep,
            )?;
            w.write_all(body.as_bytes())?;
            w.flush()
        }
        Err(e) => {
            log::info!("pairing refused: {e:?}");
            let msg = match e {
                PairError::NoCode => "not pairing right now, open \"pair a device\" on the frame first",
                PairError::Expired => "that code expired, get a new one on the frame",
                _ => "wrong code",
            };
            write_error(w, 403, msg, keep)
        }
    }
}

fn etag(size: u64, meta: &std::fs::Metadata) -> String {
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("\"{size:x}-{mtime:x}\"")
}

fn send_file(req: &Request, w: &mut Tls, state: &State, id: &str) -> io::Result<bool> {
    let keep = req.keep_alive;
    let Some(clip) = state.library.get(id) else {
        write_error(w, 404, "no such clip", keep)?;
        return Ok(true);
    };
    let (file, meta) = match File::open(&clip.path).and_then(|f| f.metadata().map(|m| (f, m))) {
        Ok(v) => v,
        Err(_) => {
            write_error(w, 404, "that clip is gone", keep)?;
            return Ok(true);
        }
    };
    let size = meta.len();
    let tag = etag(size, &meta);
    // a Range only applies if the file is still the one the client started on
    let range_header = match req.header("if-range") {
        Some(v) if v != tag => None,
        _ => req.header("range"),
    };
    let (status, start, len) = match parse_range(range_header, size) {
        Range::Full => (200, 0, size),
        Range::Part(s, e) => (206, s, e - s + 1),
        Range::Unsatisfiable => {
            write_head(
                w,
                416,
                &[("Content-Range", format!("bytes */{size}")), ("Content-Length", "0".into())],
                keep,
            )?;
            w.flush()?;
            return Ok(true);
        }
    };

    let mut headers = vec![
        ("Content-Type", "video/mp4".to_string()),
        ("Content-Length", len.to_string()),
        ("Accept-Ranges", "bytes".into()),
        ("ETag", tag),
        ("Content-Disposition", format!("attachment; filename=\"{}\"", clip.name.replace('"', ""))),
    ];
    if status == 206 {
        headers.push(("Content-Range", format!("bytes {start}-{}/{size}", start + len - 1)));
    }
    write_head(w, status, &headers, keep)?;
    if req.method == "HEAD" {
        w.flush()?;
        return Ok(true);
    }

    let _transfer = state.throttle.start();
    let fd = file.as_raw_fd();
    unsafe { libc::posix_fadvise(fd, start as i64, len as i64, libc::POSIX_FADV_SEQUENTIAL) };
    let mut buf = vec![0u8; CHUNK];
    let (mut pos, end) = (start, start + len);
    let mut dropped_to = start;
    while pos < end {
        let want = CHUNK.min((end - pos) as usize);
        let n = file.read_at(&mut buf[..want], pos)?;
        if n == 0 {
            // shrank under us; the client sees a short body and retries
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "file got shorter"));
        }
        state.throttle.pace(n);
        w.write_all(&buf[..n])?;
        pos += n as u64;
        if pos - dropped_to >= DROP_CACHE_EVERY || pos == end {
            unsafe { libc::posix_fadvise(fd, dropped_to as i64, (pos - dropped_to) as i64, libc::POSIX_FADV_DONTNEED) };
            dropped_to = pos;
        }
    }
    w.flush()?;
    Ok(true)
}

fn delete(w: &mut Tls, state: &State, id: &str, keep: bool) -> io::Result<()> {
    if !Settings::load(&state.paths).delete_after_sync {
        return write_error(w, 403, "deleting after sync is turned off on the frame", keep);
    }
    let Some(clip) = state.library.get(id) else {
        return write_error(w, 404, "no such clip", keep);
    };
    if let Err(e) = std::fs::remove_file(&clip.path) {
        log::warn!("couldn't delete {}: {e}", clip.path.display());
        return write_error(w, 500, "couldn't delete it", keep);
    }
    log::info!("deleted {} after sync", clip.name);
    if let Some(id) = state.library.remove(&clip.path) {
        state.hub.send(&Change::Removed(id));
    }
    write_head(w, 204, &[("Content-Length", "0".into())], keep)?;
    w.flush()
}

fn events(w: &mut Tls, state: &State, device: &str, token: &str) -> io::Result<()> {
    write_head(
        w,
        200,
        &[("Content-Type", "text/event-stream".into()), ("Cache-Control", "no-store".into())],
        false,
    )?;
    w.write_all(b"retry: 3000\n\n")?;
    w.flush()?;
    let rx = state.hub.subscribe(device);
    state.devices.touch(device);
    log::info!("device {device} is listening");
    loop {
        match rx.recv_timeout(KEEPALIVE) {
            Ok(msg) => w.write_all(msg.as_bytes())?,
            Err(RecvTimeoutError::Timeout) => w.write_all(b": keepalive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => break,
        }
        w.flush()?;
        if state.devices.check(token).is_none() {
            log::info!("device {device} was removed, closing its stream");
            break;
        }
    }
    Ok(())
}
