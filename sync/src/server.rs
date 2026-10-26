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
use std::time::{Duration, Instant, UNIX_EPOCH};

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::Deserialize;

use crate::config::{Paths, Settings, PROTOCOL_VERSION};
use crate::devices::{Devices, PairError, Pairing};
use crate::events::{clip_json, Hub};
use crate::frame;
use crate::http::{self, parse_range, read_request, write_error, write_head, write_json, Range, ReadError, Request};
use crate::library::{Change, Library};
use crate::recording;
use crate::remote::{self, Remote};
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
    pub updates: Arc<crate::update::Updates>,
    pub remote: Remote,
    pub connections: AtomicUsize,
}

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
fn handle(req: &Request, w: &mut impl Write, state: &State) -> io::Result<bool> {
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

    state.devices.touch(&device, req.header("X-Framecorder-App"));
    match (req.method.as_str(), parts.as_slice()) {
        ("DELETE", ["pair"]) => {
            match state.devices.remove(&device) {
                Ok(()) => {
                    write_head(w, 204, &[("Content-Length", "0".into())], keep)?;
                    w.flush()?;
                }
                Err(e) => write_error(w, 500, &format!("couldn't forget this device: {e}"), keep)?,
            }
            Ok(true)
        }
        ("GET", ["clips"]) => {
            let list: Vec<String> = state.library.list().iter().map(clip_json).collect();
            write_json(w, 200, &format!("[{}]", list.join(",")), keep)?;
            Ok(true)
        }
        ("GET" | "HEAD", ["clips", id, "file"]) => send_file(req, w, state, id, &device),
        ("DELETE", ["clips", id]) => {
            delete(w, state, id, keep)?;
            Ok(true)
        }
        ("GET", ["update"]) => {
            write_json(w, 200, &state.updates.status().to_string(), keep)?;
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
        ("GET", ["recording"]) => {
            write_json(w, 200, &recording::status(&state.paths).to_string(), keep)?;
            Ok(true)
        }
        ("PATCH", ["recording"]) => {
            let changes = serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
            match recording::change(&state.paths.recording(), &changes) {
                Ok(_) => {
                    let now = recording::status(&state.paths);
                    log::info!("device {device} changed the recording settings: {changes}");
                    write_json(w, 200, &now.to_string(), keep)?;
                }
                Err(why) => write_error(w, 400, &why, keep)?,
            }
            Ok(true)
        }
        ("GET", ["frame"]) => {
            write_json(w, 200, &frame::about(&state.paths, &state.library).to_string(), keep)?;
            Ok(true)
        }
        ("GET", ["remote"]) => {
            write_json(w, 200, &remote::status(&state.paths).to_string(), keep)?;
            Ok(true)
        }
        ("POST", ["remote"]) => {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            let what = body["do"].as_str().unwrap_or("");
            let from = state.devices.name(&device).unwrap_or_else(|| "a phone".into());
            match state.remote.send(&state.paths, what, &from) {
                Ok(now) => {
                    log::info!("{from} asked the tab to {what}");
                    write_json(w, 200, &now.to_string(), keep)?;
                }
                Err((status, why)) => {
                    log::info!("{from} asked the tab to {what}: {why}");
                    write_error(w, status, &why, keep)?;
                }
            }
            Ok(true)
        }
        (_, ["hello" | "pair" | "clips" | "events" | "update" | "recording" | "remote" | "frame", ..]) => {
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

fn pair(req: &Request, w: &mut impl Write, state: &State) -> io::Result<()> {
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

fn send_file(
    req: &Request,
    w: &mut impl Write,
    state: &State,
    id: &str,
    device: &str,
) -> io::Result<bool> {
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
    let mut progress = state.devices.transfer(device, &clip.name, start, size);
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
        progress.progress(pos);
        if pos - dropped_to >= DROP_CACHE_EVERY || pos == end {
            unsafe { libc::posix_fadvise(fd, dropped_to as i64, (pos - dropped_to) as i64, libc::POSIX_FADV_DONTNEED) };
            dropped_to = pos;
        }
    }
    w.flush()?;
    Ok(true)
}

fn delete(w: &mut impl Write, state: &State, id: &str, keep: bool) -> io::Result<()> {
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

fn events(w: &mut impl Write, state: &State, device: &str, token: &str) -> io::Result<()> {
    let rx = state.hub.subscribe(device);
    let _connected = state.devices.connected(device);
    write_head(
        w,
        200,
        &[("Content-Type", "text/event-stream".into()), ("Cache-Control", "no-store".into())],
        false,
    )?;
    w.write_all(b"retry: 3000\n\n")?;
    // where the tab's at and how the headset's doing, so the phone doesn't have to ask
    w.write_all(format!("event: remote\ndata: {}\n\n", remote::status(&state.paths)).as_bytes())?;
    let mut about = frame::about(&state.paths, &state.library);
    w.write_all(format!("event: frame\ndata: {about}\n\n").as_bytes())?;
    w.flush()?;
    w.write_all(
        format!(
            "event: settings\ndata: {}\n\n",
            recording::status(&state.paths)
        )
        .as_bytes(),
    )?;
    w.flush()?;
    log::info!("device {device} is listening");
    let mut looked = Instant::now();
    loop {
        match rx.recv_timeout(KEEPALIVE) {
            Ok(msg) => w.write_all(msg.as_bytes())?,
            Err(RecvTimeoutError::Timeout) => w.write_all(b": keepalive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // the battery and the disk move slowly, a look now and then is plenty
        if looked.elapsed() >= KEEPALIVE {
            looked = Instant::now();
            let now = frame::about(&state.paths, &state.library);
            if now != about {
                w.write_all(format!("event: frame\ndata: {now}\n\n").as_bytes())?;
                about = now;
            }
        }
        w.flush()?;
        if state.devices.check(token).is_none() {
            log::info!("device {device} was removed, closing its stream");
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::Path;
    use std::sync::Barrier;

    fn setup() -> (tempfile::TempDir, State) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(&dir.path().join("videos"), &dir.path().join("sync"));
        crate::watch::ensure_dirs(&paths).unwrap();
        std::fs::write(paths.devices(), json!([
            {"id": "a", "name": "phone", "token_sha256": crate::config::sha256_hex(b"foo"), "paired_at": 0, "last_seen": 0},
            {"id": "b", "name": "desktop", "token_sha256": crate::config::sha256_hex(b"bar"), "paired_at": 0, "last_seen": 0}
        ]).to_string()).unwrap();
        let identity = crate::config::identity(&paths.state).unwrap();
        let devices = Arc::new(Devices::load(&paths.devices()));
        let state = State {
            name: "test".into(),
            fingerprint: identity.fingerprint.clone(),
            tls: crate::config::tls_config(&identity).unwrap(),
            library: Arc::new(Library::new(&paths)),
            hub: Arc::new(Hub::default()),
            devices: devices.clone(),
            pairing: Pairing::new(&paths.pairing()),
            throttle: Throttle::new(0.0),
            updates: Arc::new(crate::update::Updates::new(&paths, devices)),

            remote: Remote::default(),
            connections: AtomicUsize::new(0),
            paths,
        };
        (dir, state)
    }

    fn request(method: &str, path: &str, body: Value, app: Option<&str>) -> Request {
        let mut headers = vec![("Authorization".into(), "Bearer foo".into())];
        if let Some(app) = app {
            headers.push(("X-Framecorder-App".into(), app.into()));
        }
        Request {
            method: method.into(),
            path: path.into(),
            headers,
            body: body.to_string().into_bytes(),
            keep_alive: true,
        }
    }

    fn response(state: &State, req: &Request) -> (u16, Value) {
        let mut bytes = Vec::new();
        handle(req, &mut bytes, state).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        (
            head.split_whitespace().nth(1).unwrap().parse().unwrap(),
            serde_json::from_str(body).unwrap_or(Value::Null),
        )
    }

    fn clients(state: &State) -> Value {
        serde_json::from_slice(&std::fs::read(state.paths.state.join("clients.json")).unwrap()).unwrap()
    }

    #[test]
    fn requests_record_app_identity_and_unpair_revokes_only_the_caller() {
        let (_dir, state) = setup();
        let req = request("GET", "/clips", Value::Null, Some("android/0.2.0"));
        assert_eq!(response(&state, &req).0, 200);
        let devices: Value =
            serde_json::from_slice(&std::fs::read(state.paths.devices()).unwrap()).unwrap();
        assert_eq!(devices[0]["app"], "android/0.2.0");
        assert!(devices[0]["last_seen"].as_i64().unwrap() > 0);
        assert_eq!(clients(&state)["devices"][0]["app"], "android/0.2.0");
        assert_eq!(
            response(&state, &request("GET", "/clips", Value::Null, None)).0,
            200
        );
        assert!(clients(&state)["devices"][0]["app"].is_null());
        assert_eq!(
            response(&state, &request("DELETE", "/pair", Value::Null, None)).0,
            204
        );
        assert!(state.devices.check("foo").is_none());
        assert!(state.devices.check("bar").is_some());
        assert_eq!(clients(&state)["devices"].as_array().unwrap().len(), 1);
        assert_eq!(response(&state, &req).0, 401);
        assert_eq!(Devices::load(&state.paths.devices()).count(), 1);
    }

    #[test]
    fn recording_reports_eye_panels_and_keeps_concurrent_partial_changes() {
        let (_dir, state) = setup();
        assert_eq!(
            response(&state, &request("GET", "/recording", Value::Null, None)).1["panels"],
            true
        );
        std::fs::write(state.paths.status(), r#"{"panels":false}"#).unwrap();
        let settings = response(&state, &request("GET", "/recording", Value::Null, None)).1;
        assert_eq!(settings["eye"], "left");
        assert_eq!(settings["panels"], false);
        for _ in 0..32 {
            crate::recording::change(
                &state.paths.recording(),
                &json!({"mic": true, "eye": "left"}),
            )
            .unwrap();
            let barrier = Barrier::new(2);
            std::thread::scope(|scope| {
                for patch in [json!({"mic": false}), json!({"eye": "right"})] {
                    let (state, barrier) = (&state, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        assert_eq!(
                            response(state, &request("PATCH", "/recording", patch, None)).0,
                            200
                        );
                    });
                }
            });
            let settings = response(&state, &request("GET", "/recording", Value::Null, None)).1;
            assert_eq!(settings["mic"], false);
            assert_eq!(settings["eye"], "right");
            assert_eq!(settings["panels"], false);
        }
    }

    struct StreamProbe<'a> {
        state: &'a State,
        bytes: Vec<u8>,
    }

    impl Write for StreamProbe<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            assert_eq!(clients(self.state)["devices"][0]["connected"], true);
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self
                .bytes
                .windows(b"event: settings".len())
                .any(|b| b == b"event: settings")
            {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "client left"));
            }
            Ok(())
        }
    }

    #[test]
    fn event_stream_publishes_settings_and_clears_connected_when_the_client_leaves() {
        let (_dir, state) = setup();
        let mut probe = StreamProbe {
            state: &state,
            bytes: Vec::new(),
        };
        let err = handle(
            &request("GET", "/events", Value::Null, None),
            &mut probe,
            &state,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        let body = String::from_utf8(probe.bytes).unwrap();
        let settings = body
            .split("event: settings\ndata: ")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(settings).unwrap(),
            recording::status(&state.paths)
        );
        assert_eq!(clients(&state)["devices"][0]["connected"], false);
    }

    struct FileProbe<'a> {
        path: &'a Path,
        header: bool,
        fail: bool,
        expected_start: u64,
        expected_total: u64,
    }

    impl Write for FileProbe<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.header {
                self.header = false;
            } else {
                let clients: Value =
                    serde_json::from_slice(&std::fs::read(self.path).unwrap()).unwrap();
                let transfer = &clients["devices"][0]["transfer"];
                assert_eq!(transfer["name"], "x.mp4");
                assert_eq!(transfer["done"], self.expected_start);
                assert_eq!(transfer["total"], self.expected_total);
                if self.fail {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "client left"));
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn downloads_publish_resumed_offsets_and_clear_transfers_on_finish_or_error() {
        let (_dir, state) = setup();
        let path = state.paths.videos.join("x.mp4");
        crate::library::tests::fake_mp4(&path, 1);
        let size = std::fs::metadata(&path).unwrap().len();
        state.library.add(&path).unwrap();
        for fail in [false, true] {
            let mut req = request("GET", "/clips/r-x/file", Value::Null, None);
            req.headers.push(("Range".into(), "bytes=10-".into()));
            let clients_path = state.paths.state.join("clients.json");
            let mut probe = FileProbe {
                path: &clients_path,
                header: true,
                fail,
                expected_start: 10,
                expected_total: size,
            };
            assert_eq!(handle(&req, &mut probe, &state).is_err(), fail);
            assert!(clients(&state)["devices"][0]["transfer"].is_null());
        }
    }
}
