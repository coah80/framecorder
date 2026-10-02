//! Keeps each paired Frame in sync: connect, catch up on anything missed,
//! then follow its event stream. Reconnects with backoff whenever the Frame
//! goes away (asleep, off, other Wi-Fi), which is most of the time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

use super::api::{About, ApiError, Client, Recording, Remote, RemoteClip};
use super::discover;
use super::space;
use super::store::{Entry, Host, Hosts, Index};

const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A Frame that dropped our token won't change its mind by itself.
const UNPAIRED_RETRY: Duration = Duration::from_secs(300);
/// mDNS plus a sweep of the local network, stopping as soon as it's found.
const FIND_WINDOW: Duration = Duration::from_secs(6);
/// A Frame that's asleep doesn't answer anyway, so the sweep for one that
/// might have moved runs this often at most (unless someone hits retry).
const SWEEP_EVERY: Duration = Duration::from_secs(120);
/// How often to look whether there's room again on a full device.
const FULL_RETRY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Connecting,
    Connected,
    Unreachable,
    /// The Frame doesn't accept our token anymore.
    Unpaired,
    WrongFingerprint,
    /// This device is out of space; the clips wait on the Frame.
    Full,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub fingerprint: String,
    pub name: String,
    pub addr: String,
    pub state: State,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub fingerprint: String,
    pub id: String,
    pub name: String,
    pub done: u64,
    pub total: u64,
    pub queued: usize,
}

/// Where downloads go. Plain folders on desktop, MediaStore on Android.
pub trait Sink: Send + Sync {
    fn staging(&self, host: &Host, clip: &RemoteClip) -> PathBuf;
    /// Moves a complete download to its final place and says where that is.
    fn finish(&self, staged: &Path, host: &Host, clip: &RemoteClip) -> Result<String, String>;
    /// How many copies of a file exist at once while it's put in place.
    fn copies(&self) -> u64 {
        1
    }
}

/// Whoever wants to hear about it: the UI, or the log in headless mode.
pub trait Listener: Send + Sync {
    fn status(&self, status: &Status);
    fn progress(&self, progress: &Progress);
    fn synced(&self, entry: &Entry);
    /// A download started or everything's done, for wake locks.
    fn busy(&self, _busy: bool) {}
    /// A clip disappeared from the Frame.
    fn removed(&self, _host: &str, _id: &str) {}
    /// What the Frame's tab is up to; `None` once we've lost touch.
    fn remote(&self, _host: &str, _remote: Option<&Remote>) {}
    /// The headset's battery and storage; `None` once we've lost touch.
    fn about(&self, _host: &str, _about: Option<&About>) {}
}

/// The platform's own way to find a Frame by fingerprint, tried before
/// mDNS and the sweep. Android asks its system NSD service, which works
/// without holding a multicast lock.
pub trait Finder: Send + Sync {
    /// Addresses (`ip:port`) the Frame with this fingerprint answers at,
    /// waiting up to `window`. Empty if it isn't around. May block.
    fn find(&self, fingerprint: &str, window: Duration) -> Vec<String>;
}

struct Running {
    task: JoinHandle<()>,
    nudge: Arc<Notify>,
}

pub struct Engine {
    rt: Handle,
    hosts: Mutex<Hosts>,
    index: Mutex<Index>,
    sink: Arc<dyn Sink>,
    listener: Arc<dyn Listener>,
    device_name: String,
    running: Mutex<HashMap<String, Running>>,
    statuses: Mutex<HashMap<String, Status>>,
    finder: Mutex<Option<Arc<dyn Finder>>>,
    /// Off where the platform finder covers mDNS (Android).
    mdns: AtomicBool,
    /// Per Frame, when it was last swept for.
    swept: Mutex<HashMap<String, Instant>>,
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Engine {
    pub fn new(
        rt: Handle,
        state_dir: &Path,
        sink: Arc<dyn Sink>,
        listener: Arc<dyn Listener>,
        device_name: &str,
    ) -> Arc<Self> {
        Arc::new(Self {
            rt,
            hosts: Mutex::new(Hosts::load(state_dir)),
            index: Mutex::new(Index::load(state_dir)),
            sink,
            listener,
            device_name: device_name.to_string(),
            running: Mutex::new(HashMap::new()),
            statuses: Mutex::new(HashMap::new()),
            finder: Mutex::new(None),
            mdns: AtomicBool::new(true),
            swept: Mutex::new(HashMap::new()),
        })
    }

    /// Lets the platform find Frames its own way; `mdns` says whether our
    /// own mDNS browsing should still run too.
    pub fn set_finder(&self, finder: Option<Arc<dyn Finder>>, mdns: bool) {
        *self.finder.lock().unwrap() = finder;
        self.mdns.store(mdns, Ordering::SeqCst);
    }

    pub fn mdns(&self) -> bool {
        self.mdns.load(Ordering::SeqCst)
    }

    pub fn hosts(&self) -> Vec<Host> {
        self.hosts.lock().unwrap().list().to_vec()
    }

    pub fn host(&self, fingerprint: &str) -> Option<Host> {
        self.hosts.lock().unwrap().get(fingerprint).cloned()
    }

    pub fn clips(&self) -> Vec<Entry> {
        self.index.lock().unwrap().list()
    }

    pub fn clip(&self, key: &str) -> Option<Entry> {
        self.index.lock().unwrap().get(key).cloned()
    }

    pub fn statuses(&self) -> Vec<Status> {
        let statuses = self.statuses.lock().unwrap();
        self.hosts()
            .iter()
            .map(|h| {
                statuses.get(&h.fingerprint).cloned().unwrap_or_else(|| Status {
                    fingerprint: h.fingerprint.clone(),
                    name: h.name.clone(),
                    addr: h.addr.clone(),
                    state: State::Connecting,
                    message: None,
                })
            })
            .collect()
    }

    pub fn start_all(self: &Arc<Self>) {
        for host in self.hosts() {
            self.start(&host.fingerprint);
        }
    }

    pub fn start(self: &Arc<Self>, fingerprint: &str) {
        let mut running = self.running.lock().unwrap();
        if running.get(fingerprint).is_some_and(|r| !r.task.is_finished()) {
            return;
        }
        let nudge = Arc::new(Notify::new());
        let (engine, fp, n) = (self.clone(), fingerprint.to_string(), nudge.clone());
        let task = self.rt.spawn(async move {
            use futures_util::FutureExt;
            // a panic somewhere down in a platform call shouldn't end syncing for good
            while std::panic::AssertUnwindSafe(engine.clone().host_loop(fp.clone(), n.clone())).catch_unwind().await.is_err() {
                log::error!("sync with {fp} fell over, starting again");
                tokio::time::sleep(MAX_BACKOFF).await;
            }
        });
        running.insert(fingerprint.to_string(), Running { task, nudge });
    }

    /// Skip the backoff and try every Frame right now, sweep included.
    pub fn retry_now(&self) {
        self.swept.lock().unwrap().clear();
        for r in self.running.lock().unwrap().values() {
            r.nudge.notify_one();
        }
    }

    pub fn unpair(&self, fingerprint: &str) -> Result<(), String> {
        if let Some(r) = self.running.lock().unwrap().remove(fingerprint) {
            r.task.abort();
        }
        self.statuses.lock().unwrap().remove(fingerprint);
        self.hosts.lock().unwrap().remove(fingerprint).map_err(|e| e.to_string())
    }

    /// Pairs with the Frame at `addr`. With a fingerprint (QR code, mDNS)
    /// only that exact Frame is accepted, and if `addr` doesn't answer we
    /// look for it by fingerprint. Without one we learn it now and pin it
    /// from then on. `replaces` is a paired Frame this one is (it got a new
    /// certificate): its clips carry over and its old pairing goes.
    pub async fn pair(
        self: &Arc<Self>,
        addr: &str,
        fingerprint: Option<&str>,
        code: &str,
        replaces: Option<&str>,
    ) -> Result<Host, String> {
        let (client, addr) = match fingerprint {
            Some(fp) => match reach(&[addr.to_string()], fp, None).await {
                Ok(v) => v,
                Err(e @ ApiError::Unreachable(_)) => match self.locate(fp, true).await {
                    Some(addrs) => reach(&addrs, fp, None).await.map_err(|e| e.to_string())?,
                    None => return Err(e.to_string()),
                },
                Err(e) => return Err(e.to_string()),
            },
            None => {
                let client = Client::capture(addr).map_err(|e| e.to_string())?;
                client.hello().await.map_err(|e| e.to_string())?;
                (client, addr.to_string())
            }
        };
        let hello = client.hello().await.map_err(|e| e.to_string())?;
        let fingerprint = client.seen_fingerprint().ok_or("the frame didn't show a certificate")?;
        if hello.fingerprint != fingerprint {
            return Err("the frame's certificate doesn't match what it says it is".into());
        }
        let paired = client.pair(code, &self.device_name).await.map_err(|e| e.to_string())?;
        let host = Host {
            fingerprint: fingerprint.clone(),
            name: hello.name,
            addr: addr.clone(),
            token: paired.token,
            device_id: paired.device_id,
            paired_at: now_unix(),
        };
        self.hosts.lock().unwrap().upsert(host.clone()).map_err(|e| format!("couldn't save the pairing: {e}"))?;
        log::info!("paired with {} at {addr}", host.name);
        // before syncing starts, or everything it already sent comes over again
        if let Some(old) = replaces.filter(|old| *old != fingerprint) {
            match self.index.lock().unwrap().adopt(old, &fingerprint) {
                Ok(n) => log::info!("{n} clips carried over from its old pairing"),
                Err(e) => log::warn!("couldn't carry its clips over: {e}"),
            }
            if let Err(e) = self.unpair(old) {
                log::warn!("couldn't drop its old pairing: {e}");
            }
        }
        self.start(&fingerprint);
        Ok(host)
    }

    /// The Frame's recording settings, `None` if its framecorder predates them.
    pub async fn recording(&self, fingerprint: &str) -> Result<Option<Recording>, String> {
        let host = self.host(fingerprint).ok_or("that frame isn't paired")?;
        let client = self.connect(&host).await.map_err(|e| e.to_string())?;
        client.recording().await.map_err(|e| e.to_string())
    }

    pub async fn set_recording(&self, fingerprint: &str, settings: &Recording) -> Result<Recording, String> {
        let host = self.host(fingerprint).ok_or("that frame isn't paired")?;
        let client = self.connect(&host).await.map_err(|e| e.to_string())?;
        client.set_recording(settings).await.map_err(|e| e.to_string())
    }

    /// Asks the Frame to `record`, `stop` or `clip`.
    pub async fn command(&self, fingerprint: &str, what: &str) -> Result<Remote, String> {
        let host = self.host(fingerprint).ok_or("that frame isn't paired")?;
        let client = self.connect(&host).await.map_err(|e| e.to_string())?;
        client.command(what).await.map_err(|e| e.to_string())
    }

    fn set_status(&self, host: &Host, state: State, message: Option<String>) {
        let status = Status {
            fingerprint: host.fingerprint.clone(),
            name: host.name.clone(),
            addr: host.addr.clone(),
            state,
            message,
        };
        self.statuses.lock().unwrap().insert(host.fingerprint.clone(), status.clone());
        self.listener.status(&status);
    }

    async fn host_loop(self: Arc<Self>, fingerprint: String, nudge: Arc<Notify>) {
        let mut backoff = MIN_BACKOFF;
        loop {
            let Some(host) = self.host(&fingerprint) else { return };
            self.set_status(&host, State::Connecting, None);
            let (was_connected, err) = self.session(&host).await;
            let host = self.host(&fingerprint).unwrap_or(host);
            let wait = match &err {
                ApiError::Unauthorized => {
                    self.set_status(&host, State::Unpaired, Some(err.to_string()));
                    UNPAIRED_RETRY
                }
                ApiError::WrongFingerprint => {
                    self.set_status(&host, State::WrongFingerprint, Some(err.to_string()));
                    MAX_BACKOFF
                }
                ApiError::Full { .. } => {
                    log::info!("{}: this device is full ({err})", host.name);
                    self.set_status(&host, State::Full, Some(err.to_string()));
                    backoff = MIN_BACKOFF;
                    FULL_RETRY
                }
                _ => {
                    log::info!("{}: {err}", host.name);
                    self.set_status(&host, State::Unreachable, Some(err.to_string()));
                    if was_connected {
                        backoff = MIN_BACKOFF;
                    }
                    let wait = backoff;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    wait
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = nudge.notified() => backoff = MIN_BACKOFF,
            }
        }
    }

    /// Where the Frame with this fingerprint is now: the platform finder
    /// first, then mDNS and a sweep of the local network. `force` sweeps even
    /// if it was swept for recently.
    async fn locate(&self, fingerprint: &str, force: bool) -> Option<Vec<String>> {
        let finder = self.finder.lock().unwrap().clone();
        if let Some(finder) = finder {
            let fp = fingerprint.to_string();
            let addrs = tokio::task::spawn_blocking(move || finder.find(&fp, FIND_WINDOW)).await.unwrap_or_default();
            if !addrs.is_empty() {
                return Some(addrs);
            }
        }
        let due = {
            let mut swept = self.swept.lock().unwrap();
            let due = force || swept.get(fingerprint).is_none_or(|t| t.elapsed() >= SWEEP_EVERY);
            if due {
                swept.insert(fingerprint.to_string(), Instant::now());
            }
            due
        };
        if !due && !self.mdns() {
            return None;
        }
        // between sweeps, mDNS alone (where it runs) still gets a look
        let window = if due { FIND_WINDOW } else { Duration::from_secs(3) };
        let found = if due {
            discover::find(fingerprint, window, self.mdns()).await
        } else {
            let mut hit = None;
            let _ = discover::browse(window, |f| {
                let it = f.fingerprint == fingerprint;
                if it {
                    hit = Some(f);
                }
                !it
            })
            .await;
            hit
        };
        found.map(|f| f.addrs)
    }

    /// Finds the Frame, at its last address or wherever it is now.
    async fn connect(&self, host: &Host) -> Result<Client, ApiError> {
        let token = Some(host.token.as_str());
        let err = match reach(std::slice::from_ref(&host.addr), &host.fingerprint, token).await {
            Ok((client, _)) => return Ok(client),
            Err(e @ ApiError::Unreachable(_)) | Err(e @ ApiError::WrongFingerprint) => e,
            Err(e) => return Err(e),
        };
        let Some(addrs) = self.locate(&host.fingerprint, false).await else { return Err(err) };
        let (client, addr) = reach(&addrs, &host.fingerprint, token).await?;
        if addr != host.addr {
            log::info!("{} moved to {addr}", host.name);
            if let Err(e) = self.hosts.lock().unwrap().set_addr(&host.fingerprint, &addr) {
                log::warn!("couldn't save the new address: {e}");
            }
        }
        Ok(client)
    }

    /// One connected stretch. Returns whether we got connected at all and
    /// why it ended.
    async fn session(&self, host: &Host) -> (bool, ApiError) {
        let client = match self.connect(host).await {
            Ok(c) => c,
            Err(e) => return (false, e),
        };
        let host = self.host(&host.fingerprint).unwrap_or_else(|| host.clone());
        // subscribe before listing, so nothing lands in between unseen
        let mut events = match client.events().await {
            Ok(e) => e,
            Err(e) => return (false, e),
        };
        let remote = match client.clips().await {
            Ok(c) => c,
            Err(e) => return (false, e),
        };
        self.set_status(&host, State::Connected, None);

        let (tx, rx) = mpsc::unbounded_channel();
        let missing = self.index.lock().unwrap().missing(&host.fingerprint, &remote);
        if !missing.is_empty() {
            log::info!("{}: {} to catch up on", host.name, missing.len());
        }
        for clip in missing {
            let _ = tx.send(clip);
        }

        let listen = async {
            loop {
                let ev = match events.next().await {
                    Ok(ev) => ev,
                    Err(e) => return e,
                };
                match ev.event.as_str() {
                    "new" => match serde_json::from_str::<RemoteClip>(&ev.data) {
                        Ok(clip) => {
                            log::info!("{}: new {}", host.name, clip.name);
                            let _ = tx.send(clip);
                        }
                        Err(e) => log::warn!("odd event from the frame: {e}"),
                    },
                    "removed" => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&ev.data) {
                            if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                                self.listener.removed(&host.fingerprint, id);
                            }
                        }
                    }
                    "remote" => match serde_json::from_str::<Remote>(&ev.data) {
                        Ok(r) => self.listener.remote(&host.fingerprint, Some(&r)),
                        Err(e) => log::warn!("odd event from the frame: {e}"),
                    },
                    "frame" => match serde_json::from_str::<About>(&ev.data) {
                        Ok(a) => self.listener.about(&host.fingerprint, Some(&a)),
                        Err(e) => log::warn!("odd event from the frame: {e}"),
                    },
                    _ => {}
                }
            }
        };
        let err = tokio::select! {
            e = listen => e,
            e = self.download_all(&client, &host, rx) => e,
        };
        self.listener.busy(false);
        self.listener.remote(&host.fingerprint, None);
        self.listener.about(&host.fingerprint, None);
        (true, err)
    }

    async fn download_all(&self, client: &Client, host: &Host, mut rx: mpsc::UnboundedReceiver<RemoteClip>) -> ApiError {
        // the smallest file that didn't fit, while the rest still get their turn
        let mut full: Option<ApiError> = None;
        while let Some(clip) = rx.recv().await {
            if self.index.lock().unwrap().contains(&host.fingerprint, &clip.id) {
                continue;
            }
            self.listener.busy(true);
            match self.download_one(client, host, &clip, rx.len()).await {
                Ok(()) => {}
                // gone from the Frame in the meantime, nothing to get
                Err(ApiError::Refused(404, _)) => log::info!("{} is gone, skipping", clip.name),
                Err(e @ ApiError::Full { .. }) => {
                    log::info!("no room for {} ({e})", clip.name);
                    let smaller = |old: &ApiError| matches!((old, &e), (ApiError::Full { need: a, .. }, ApiError::Full { need: b, .. }) if b < a);
                    if full.as_ref().is_none_or(smaller) {
                        full = Some(e);
                    }
                }
                Err(e) => return e,
            }
            if rx.is_empty() {
                self.listener.busy(false);
                if let Some(e) = full.take() {
                    return e;
                }
            }
        }
        ApiError::Other("download queue closed".into())
    }

    async fn download_one(&self, client: &Client, host: &Host, clip: &RemoteClip, queued: usize) -> Result<(), ApiError> {
        let part = self.sink.staging(host, clip);
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0).min(clip.size);
        space::room_for(clip.size, have, self.sink.copies(), space::free(&part))?;
        let listener = self.listener.clone();
        let mut progress = Progress {
            fingerprint: host.fingerprint.clone(),
            id: clip.id.clone(),
            name: clip.name.clone(),
            done: 0,
            total: clip.size,
            queued,
        };
        client
            .download(clip, &part, |done, total| {
                progress.done = done;
                progress.total = total;
                listener.progress(&progress);
            })
            .await?;

        let location = self.sink.finish(&part, host, clip).map_err(ApiError::Other)?;
        let entry = Entry {
            host: host.fingerprint.clone(),
            id: clip.id.clone(),
            name: clip.name.clone(),
            kind: clip.kind.clone(),
            size: clip.size,
            created: clip.created,
            duration_s: clip.duration_s,
            location,
            synced_at: now_unix(),
        };
        self.index
            .lock()
            .unwrap()
            .insert(entry.clone())
            .map_err(|e| ApiError::Other(format!("couldn't save the index: {e}")))?;
        log::info!("synced {} -> {}", entry.name, entry.location);
        self.listener.synced(&entry);
        match client.delete(&clip.id).await {
            Ok(true) => log::info!("{} deleted from the frame", clip.name),
            Ok(false) => {}
            Err(e) => log::debug!("delete after sync failed: {e}"),
        }
        Ok(())
    }
}

/// Says hello at every address at once and keeps the first that answers
/// with the right certificate.
async fn reach(addrs: &[String], fingerprint: &str, token: Option<&str>) -> Result<(Client, String), ApiError> {
    if addrs.is_empty() {
        return Err(ApiError::Unreachable("no address to try".into()));
    }
    let tries = addrs.iter().map(|addr| {
        Box::pin(async move {
            let client = Client::pinned(addr, fingerprint)?;
            let client = match token {
                Some(t) => client.with_token(t),
                None => client,
            };
            client.hello().await?;
            Ok::<_, ApiError>((client, addr.clone()))
        })
    });
    futures_util::future::select_ok(tries).await.map(|(v, _)| v)
}

/// Downloads into a folder, clips in `clips/` like on the Frame.
pub struct DirSink {
    pub root: PathBuf,
}

impl DirSink {
    fn dir(&self, clip: &RemoteClip) -> PathBuf {
        if clip.kind == "clip" {
            self.root.join("clips")
        } else {
            self.root.clone()
        }
    }
}

/// Keeps a file name to its last path component, so a hostile name can't
/// climb out of the folder.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c)).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() {
        "clip.mp4".into()
    } else {
        clean
    }
}

impl Sink for DirSink {
    fn staging(&self, host: &Host, clip: &RemoteClip) -> PathBuf {
        let short = &host.fingerprint[..8.min(host.fingerprint.len())];
        self.dir(clip).join(format!("{}.{short}.part", safe_name(&clip.name)))
    }

    fn finish(&self, staged: &Path, _host: &Host, clip: &RemoteClip) -> Result<String, String> {
        let dir = self.dir(clip);
        let name = safe_name(&clip.name);
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.clone(), String::new()),
        };
        let mut target = dir.join(&name);
        let mut n = 2;
        while target.exists() {
            target = dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
        std::fs::rename(staged, &target).map_err(|e| format!("couldn't move the download into place: {e}"))?;
        Ok(target.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_the_folder() {
        assert_eq!(safe_name("2026-09-26_14-00-00.mp4"), "2026-09-26_14-00-00.mp4");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("..\\..\\x.mp4"), "x.mp4");
        assert_eq!(safe_name(".."), "clip.mp4");
        assert_eq!(safe_name("a:b?.mp4"), "ab.mp4");
    }

    #[test]
    fn dir_sink_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let sink = DirSink { root: dir.path().to_path_buf() };
        let host = Host {
            fingerprint: "ab".repeat(32),
            name: "f".into(),
            addr: "x".into(),
            token: "t".into(),
            device_id: "d".into(),
            paired_at: 0,
        };
        let clip = RemoteClip { id: "c-a".into(), kind: "clip".into(), name: "a.mp4".into(), size: 1, duration_s: None, created: 0 };
        let mut places = Vec::new();
        for _ in 0..2 {
            let part = sink.staging(&host, &clip);
            assert!(part.starts_with(dir.path().join("clips")));
            std::fs::create_dir_all(part.parent().unwrap()).unwrap();
            std::fs::write(&part, b"x").unwrap();
            places.push(sink.finish(&part, &host, &clip).unwrap());
        }
        assert!(places[0].ends_with("a.mp4"));
        assert!(places[1].ends_with("a (2).mp4"));
    }
}
