//! Keeps each paired Frame in sync: connect, catch up on anything missed,
//! then follow its event stream. Reconnects with backoff whenever the Frame
//! goes away (asleep, off, other Wi-Fi), which is most of the time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

use super::api::{About, ApiError, Client, Recording, RecordingPatch, Remote, RemoteClip, UpdateStatus};
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
/// How often to ask a connected Frame whether there's a framecorder update,
/// and how often while one's being installed.
const UPDATE_CHECK: Duration = Duration::from_secs(30 * 60);
const UPDATE_WATCH: Duration = Duration::from_secs(4);

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
    /// Whether there's a framecorder update for it, once it's said.
    pub update: Option<UpdateStatus>,
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
    fn completed(&self, host: &Host, clip: &RemoteClip) -> Result<Option<String>, String>;
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
    fn recording(&self, _host: &str, _recording: &Recording) {}
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forgot {
    Both,
    LocalOnly,
}

pub struct Engine {
    rt: Handle,
    hosts: Mutex<Hosts>,
    index: Mutex<Index>,
    sink: Arc<dyn Sink>,
    listener: Arc<dyn Listener>,
    device_name: String,
    app: String,
    running: Mutex<HashMap<String, Running>>,
    transfers: Mutex<HashMap<String, (u64, Progress)>>,
    next_transfer: AtomicU64,
    statuses: Mutex<HashMap<String, Status>>,
    updates: Mutex<HashMap<String, UpdateStatus>>,
    requested_updates: Mutex<HashMap<String, Option<String>>>,
    /// Wakes the update checks, after asking for an update.
    update_nudge: Arc<Notify>,
    finder: Mutex<Option<Arc<dyn Finder>>>,
    /// Off where the platform finder covers mDNS (Android).
    mdns: AtomicBool,
    enabled: AtomicBool,
    /// Per Frame, when it was last swept for.
    swept: Mutex<HashMap<String, Instant>>,
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

struct Transfer<'a> {
    engine: &'a Engine,
    fingerprint: String,
    generation: u64,
}

impl Drop for Transfer<'_> {
    fn drop(&mut self) {
        self.engine.clear_transfer(&self.fingerprint, Some(self.generation));
    }
}

impl Engine {
    pub fn new(
        rt: Handle,
        state_dir: &Path,
        sink: Arc<dyn Sink>,
        listener: Arc<dyn Listener>,
        device_name: &str,
        app: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            rt,
            hosts: Mutex::new(Hosts::load(state_dir)),
            index: Mutex::new(Index::load(state_dir)),
            sink,
            listener,
            device_name: device_name.to_string(),
            app,
            running: Mutex::new(HashMap::new()),
            transfers: Mutex::new(HashMap::new()),
            next_transfer: AtomicU64::new(0),
            statuses: Mutex::new(HashMap::new()),
            updates: Mutex::new(HashMap::new()),
            requested_updates: Mutex::new(HashMap::new()),
            update_nudge: Arc::new(Notify::new()),
            finder: Mutex::new(None),
            mdns: AtomicBool::new(true),
            enabled: AtomicBool::new(true),
            swept: Mutex::new(HashMap::new()),
        })
    }

    /// Lets the platform find Frames its own way; `mdns` says whether our
    /// own mDNS browsing should still run too.
    pub fn set_finder(&self, finder: Option<Arc<dyn Finder>>, mdns: bool) {
        *self.finder.lock().unwrap() = finder;
        self.mdns.store(mdns, Ordering::SeqCst);
    }

    pub fn app(&self) -> &str {
        &self.app
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
                let mut s = statuses.get(&h.fingerprint).cloned().unwrap_or_else(|| Status {
                    fingerprint: h.fingerprint.clone(),
                    name: h.name.clone(),
                    addr: h.addr.clone(),
                    state: State::Connecting,
                    message: None,
                    update: None,
                });
                s.update = self.updates.lock().unwrap().get(&h.fingerprint).cloned();
                s
            })
            .collect()
    }

    pub fn start_all(self: &Arc<Self>) {
        self.enabled.store(true, Ordering::SeqCst);
        for host in self.hosts() {
            self.start(&host.fingerprint);
        }
    }

    pub fn start(self: &Arc<Self>, fingerprint: &str) {
        let mut running = self.running.lock().unwrap();
        if !self.enabled.load(Ordering::SeqCst) || running.get(fingerprint).is_some_and(|r| !r.task.is_finished()) {
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

    pub async fn stop(&self) {
        let tasks: Vec<_> = {
            let mut running = self.running.lock().unwrap();
            self.enabled.store(false, Ordering::SeqCst);
            running
                .drain()
                .map(|(_, running)| {
                    running.task.abort();
                    running.task
                })
                .collect()
        };
        for task in tasks {
            let _ = task.await;
        }
        let active: Vec<_> = self.transfers.lock().unwrap().keys().cloned().collect();
        for fingerprint in active {
            self.clear_transfer(&fingerprint, None);
        }
        for host in self.hosts() {
            self.listener.remote(&host.fingerprint, None);
            self.listener.about(&host.fingerprint, None);
        }
    }

    /// Skip the backoff and try every Frame right now, sweep included.
    pub fn retry_now(&self) {
        self.swept.lock().unwrap().clear();
        for r in self.running.lock().unwrap().values() {
            r.nudge.notify_one();
        }
    }

    pub async fn unpair(&self, fingerprint: &str) -> Result<Forgot, String> {
        let host = self.host(fingerprint);
        if let Some(r) = self.running.lock().unwrap().remove(fingerprint) {
            r.task.abort();
        }
        self.clear_transfer(fingerprint, None);
        self.listener.remote(fingerprint, None);
        self.listener.about(fingerprint, None);
        let remote = async {
            let host = host.as_ref().ok_or_else(|| ApiError::Other("that frame isn't paired".into()))?;
            Client::pinned(&host.addr, fingerprint, &self.app)?.with_token(&host.token).unpair().await
        };
        let forgot = match tokio::time::timeout(Duration::from_secs(3), remote).await {
            Ok(Ok(())) => Forgot::Both,
            _ => Forgot::LocalOnly,
        };
        self.statuses.lock().unwrap().remove(fingerprint);
        self.updates.lock().unwrap().remove(fingerprint);
        self.requested_updates.lock().unwrap().remove(fingerprint);
        self.hosts.lock().unwrap().remove(fingerprint).map_err(|e| e.to_string())?;
        Ok(forgot)
    }

    /// Pairs with the Frame at `addr`. With a fingerprint (QR code, mDNS)
    /// only that exact Frame is accepted, and if `addr` doesn't answer we
    /// look for it by fingerprint. Without one we learn it now and pin it
    /// from then on. `replaces` is a paired Frame this one is (it got a new
    /// certificate): its clips carry over and its old pairing goes.
    pub async fn pair(self: &Arc<Self>, addr: &str, fingerprint: Option<&str>, code: &str, replaces: Option<&str>) -> Result<Host, String> {
        let (client, addr) = match fingerprint {
            Some(fp) => match reach(&[addr.to_string()], fp, None, &self.app).await {
                Ok(v) => v,
                Err(e @ ApiError::Unreachable(_)) => match self.locate(fp, true).await {
                    Some(addrs) => reach(&addrs, fp, None, &self.app).await.map_err(|e| e.to_string())?,
                    None => return Err(e.to_string()),
                },
                Err(e) => return Err(e.to_string()),
            },
            None => {
                let client = Client::capture(addr, &self.app).map_err(|e| e.to_string())?;
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
            if let Err(e) = self.unpair(old).await {
                log::warn!("couldn't drop its old pairing: {e}");
            }
        }
        self.start(&fingerprint);
        // already following it, maybe waiting minutes after the frame forgot
        // us: the new pairing has to show up now, not on the next retry
        if let Some(r) = self.running.lock().unwrap().get(&fingerprint) {
            r.nudge.notify_one();
        }
        Ok(host)
    }

    /// The Frame's recording settings, `None` if its framecorder predates them.
    pub async fn recording(&self, fingerprint: &str) -> Result<Option<Recording>, String> {
        let host = self.host(fingerprint).ok_or("that frame isn't paired")?;
        let client = self.connect(&host).await.map_err(|e| e.to_string())?;
        client.recording().await.map_err(|e| e.to_string())
    }

    pub async fn set_recording(&self, fingerprint: &str, settings: &RecordingPatch) -> Result<Recording, String> {
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
            update: self.updates.lock().unwrap().get(&host.fingerprint).cloned(),
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
            discover::find(fingerprint, window, self.mdns(), &self.app).await
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
        let err = match reach(std::slice::from_ref(&host.addr), &host.fingerprint, token, &self.app).await {
            Ok((client, _)) => return Ok(client),
            Err(e @ ApiError::Unreachable(_)) | Err(e @ ApiError::WrongFingerprint) => e,
            Err(e) => return Err(e),
        };
        let Some(addrs) = self.locate(&host.fingerprint, false).await else { return Err(err) };
        let (client, addr) = reach(&addrs, &host.fingerprint, token, &self.app).await?;
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
                    "settings" => match serde_json::from_str::<Recording>(&ev.data) {
                        Ok(r) => self.listener.recording(&host.fingerprint, &r),
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
            e = self.watch_updates(&client, &host) => e,
        };
        self.listener.remote(&host.fingerprint, None);
        self.listener.about(&host.fingerprint, None);
        (true, err)
    }

    /// Keeps asking the Frame whether there's a framecorder update, while
    /// connected. Frames too old to say get asked once.
    async fn watch_updates(&self, client: &Client, host: &Host) -> ApiError {
        loop {
            let wait = match client.update_status().await {
                Ok(Some(update)) => {
                    let watching = update.updating;
                    self.set_update(host, update);
                    if watching {
                        UPDATE_WATCH
                    } else {
                        UPDATE_CHECK
                    }
                }
                Ok(None) => return std::future::pending().await,
                Err(e) => {
                    log::info!("{}: couldn't check for a framecorder update: {e}", host.name);
                    self.update_wait(&host.fingerprint)
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.update_nudge.notified() => tokio::time::sleep(UPDATE_WATCH).await,
            }
        }
    }

    fn update_wait(&self, fingerprint: &str) -> Duration {
        if self.requested_updates.lock().unwrap().contains_key(fingerprint)
            || self.updates.lock().unwrap().get(fingerprint).is_some_and(|u| u.updating)
        {
            UPDATE_WATCH
        } else {
            UPDATE_CHECK
        }
    }

    fn set_update(&self, host: &Host, mut update: UpdateStatus) {
        {
            let mut requested = self.requested_updates.lock().unwrap();
            if let Some(installed) = requested.get(&host.fingerprint) {
                let changed = installed.as_ref().is_some_and(|old| *old != update.installed);
                if changed || !update.updating {
                    requested.remove(&host.fingerprint);
                    update.updating = false;
                }
            }
        }
        let changed = self.updates.lock().unwrap().insert(host.fingerprint.clone(), update.clone()) != Some(update);
        if changed {
            if let Some(status) = self.statuses().into_iter().find(|s| s.fingerprint == host.fingerprint) {
                self.statuses.lock().unwrap().insert(host.fingerprint.clone(), status.clone());
                self.listener.status(&status);
            }
        }
    }

    /// Has the Frame install its framecorder update now. It restarts its
    /// sync service on the way, so the connection drops and comes back on
    /// the new version.
    pub async fn start_update(&self, fingerprint: &str) -> Result<(), String> {
        let host = self.host(fingerprint).ok_or("that frame isn't paired")?;
        let client = self.connect(&host).await.map_err(|e| e.to_string())?;
        client.start_update().await.map_err(|e| e.to_string())?;
        let current = self.updates.lock().unwrap().get(fingerprint).cloned();
        self.requested_updates.lock().unwrap().insert(fingerprint.to_string(), current.as_ref().map(|u| u.installed.clone()));
        if let Some(update) = current {
            self.set_update(&host, UpdateStatus { updating: true, ..update });
        }
        self.update_nudge.notify_waiters();
        Ok(())
    }

    fn begin_transfer(&self, progress: Progress) -> Transfer<'_> {
        let generation = self.next_transfer.fetch_add(1, Ordering::Relaxed);
        let fingerprint = progress.fingerprint.clone();
        let mut transfers = self.transfers.lock().unwrap();
        let was_empty = transfers.is_empty();
        transfers.insert(fingerprint.clone(), (generation, progress));
        if was_empty {
            self.listener.busy(true);
        }
        Transfer { engine: self, fingerprint, generation }
    }

    fn transfer_progress(&self, progress: &Progress, generation: u64) {
        let mut transfers = self.transfers.lock().unwrap();
        if let Some((active, current)) = transfers.get_mut(&progress.fingerprint) {
            if *active != generation {
                return;
            }
            *current = progress.clone();
            self.listener.progress(progress);
        }
    }

    fn clear_transfer(&self, fingerprint: &str, generation: Option<u64>) {
        let mut transfers = self.transfers.lock().unwrap();
        if transfers.get(fingerprint).is_none_or(|(current, _)| generation.is_some_and(|g| g != *current)) {
            return;
        }
        transfers.remove(fingerprint);
        if let Some((_, progress)) = transfers.values().next() {
            self.listener.progress(progress);
        } else {
            self.listener.busy(false);
        }
    }

    async fn download_all(&self, client: &Client, host: &Host, mut rx: mpsc::UnboundedReceiver<RemoteClip>) -> ApiError {
        // the smallest file that didn't fit, while the rest still get their turn
        let mut full: Option<ApiError> = None;
        while let Some(clip) = rx.recv().await {
            if self.index.lock().unwrap().contains(&host.fingerprint, &clip.id) {
                continue;
            }
            match self.download_one(client, host, &clip, rx.len()).await {
                Ok(()) => {}
                // gone from the Frame in the meantime, nothing to get
                Err(ApiError::Refused(404, _)) => log::info!("{} is gone, skipping", clip.name),
                Err(e @ ApiError::Full { .. }) => {
                    log::info!("no room for {} ({e})", clip.name);
                    let smaller =
                        |old: &ApiError| matches!((old, &e), (ApiError::Full { need: a, .. }, ApiError::Full { need: b, .. }) if b < a);
                    if full.as_ref().is_none_or(smaller) {
                        full = Some(e);
                    }
                }
                Err(e) => return e,
            }
            if rx.is_empty() {
                if let Some(e) = full.take() {
                    return e;
                }
            }
        }
        ApiError::Other("download queue closed".into())
    }

    async fn download_one(&self, client: &Client, host: &Host, clip: &RemoteClip, queued: usize) -> Result<(), ApiError> {
        let entry = |location| Entry {
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
        let recovered = match self.sink.completed(host, clip).map_err(ApiError::Other)? {
            Some(location) => {
                let mut index = self.index.lock().unwrap();
                // Claim and persist together so two Frames can't recover the same file.
                if index.location_taken(&location, &host.fingerprint, &clip.id) {
                    None
                } else {
                    let entry = entry(location);
                    index.insert(entry.clone()).map_err(|e| ApiError::Other(format!("couldn't save the index: {e}")))?;
                    Some(entry)
                }
            }
            None => None,
        };
        let entry = match recovered {
            Some(entry) => entry,
            None => {
                let part = self.sink.staging(host, clip);
                let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0).min(clip.size);
                space::room_for(clip.size, have, self.sink.copies(), space::free(&part))?;
                let mut progress = Progress {
                    fingerprint: host.fingerprint.clone(),
                    id: clip.id.clone(),
                    name: clip.name.clone(),
                    done: 0,
                    total: clip.size,
                    queued,
                };
                let transfer = self.begin_transfer(progress.clone());
                client
                    .download(clip, &part, |done, total| {
                        progress.done = done;
                        progress.total = total;
                        self.transfer_progress(&progress, transfer.generation);
                    })
                    .await?;

                let entry = entry(self.sink.finish(&part, host, clip).map_err(ApiError::Other)?);
                self.index.lock().unwrap().insert(entry.clone()).map_err(|e| ApiError::Other(format!("couldn't save the index: {e}")))?;
                entry
            }
        };
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
async fn reach(addrs: &[String], fingerprint: &str, token: Option<&str>, app: &str) -> Result<(Client, String), ApiError> {
    if addrs.is_empty() {
        return Err(ApiError::Unreachable("no address to try".into()));
    }
    let tries = addrs.iter().map(|addr| {
        Box::pin(async move {
            let client = Client::pinned(addr, fingerprint, app)?;
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

pub fn host_folder(fingerprint: &str) -> String {
    super::tls::normalize_fingerprint(fingerprint).unwrap_or_else(|| super::tls::sha256_hex(fingerprint.as_bytes()))
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

fn split_name(name: &str) -> (&str, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) => (stem, format!(".{ext}")),
        None => (name, String::new()),
    }
}

impl Sink for DirSink {
    fn completed(&self, _host: &Host, clip: &RemoteClip) -> Result<Option<String>, String> {
        let path = self.dir(clip).join(safe_name(&clip.name));
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() == clip.size => Ok(Some(path.to_string_lossy().into_owned())),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("couldn't check the saved video: {e}")),
        }
    }

    fn staging(&self, host: &Host, clip: &RemoteClip) -> PathBuf {
        let short = &host.fingerprint[..8.min(host.fingerprint.len())];
        self.dir(clip).join(format!("{}.{short}.part", safe_name(&clip.name)))
    }

    fn finish(&self, staged: &Path, _host: &Host, clip: &RemoteClip) -> Result<String, String> {
        let dir = self.dir(clip);
        let name = safe_name(&clip.name);
        let (stem, ext) = split_name(&name);
        let mut target = dir.join(&name);
        let mut n = 2;
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
                Ok(reserved) => {
                    drop(reserved);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    target = dir.join(format!("{stem} ({n}){ext}"));
                    n += 1;
                }
                Err(e) => return Err(format!("couldn't reserve a place for the video: {e}")),
            }
        }
        if let Err(e) = std::fs::rename(staged, &target) {
            let _ = std::fs::remove_file(&target);
            return Err(format!("couldn't move the download into place: {e}"));
        }
        Ok(target.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestListener {
        busy: Mutex<Vec<bool>>,
        synced: Mutex<Vec<Entry>>,
    }

    impl Listener for TestListener {
        fn status(&self, _: &Status) {}
        fn progress(&self, _: &Progress) {}
        fn synced(&self, entry: &Entry) {
            self.synced.lock().unwrap().push(entry.clone());
        }
        fn busy(&self, busy: bool) {
            self.busy.lock().unwrap().push(busy);
        }
    }

    fn host(fingerprint: &str) -> Host {
        Host {
            fingerprint: fingerprint.into(),
            name: "frame".into(),
            addr: "127.0.0.1:1".into(),
            token: "t".into(),
            device_id: "d".into(),
            paired_at: 0,
        }
    }

    fn engine(dir: &Path, listener: Arc<TestListener>) -> Arc<Engine> {
        Engine::new(Handle::current(), dir, Arc::new(DirSink { root: dir.join("videos") }), listener, "test", "desktop/test".into())
    }

    #[tokio::test]
    async fn unresolved_updates_keep_polling_until_service_or_version_settles() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), Arc::new(TestListener::default()));
        let host = host("frame");
        let update = UpdateStatus { installed: "0.1.1".into(), latest: None, available: false, updating: true };
        engine.requested_updates.lock().unwrap().insert(host.fingerprint.clone(), Some("0.1.1".into()));
        engine.set_update(&host, update.clone());
        assert_eq!(engine.update_wait(&host.fingerprint), Duration::from_secs(4));
        engine.set_update(&host, UpdateStatus { updating: false, ..update.clone() });
        assert!(!engine.updates.lock().unwrap()[&host.fingerprint].updating);
        assert_eq!(engine.update_wait(&host.fingerprint), Duration::from_secs(1800));
        engine.requested_updates.lock().unwrap().insert(host.fingerprint.clone(), Some("0.1.1".into()));
        engine.set_update(&host, UpdateStatus { installed: "0.2.0".into(), ..update });
        assert!(!engine.updates.lock().unwrap()[&host.fingerprint].updating);
        assert_eq!(engine.update_wait(&host.fingerprint), Duration::from_secs(1800));
        engine.requested_updates.lock().unwrap().insert(host.fingerprint.clone(), None);
        assert_eq!(engine.update_wait(&host.fingerprint), Duration::from_secs(4));
    }

    fn progress(fingerprint: &str) -> Progress {
        Progress { fingerprint: fingerprint.into(), id: "r-test".into(), name: "test.mp4".into(), done: 0, total: 10, queued: 0 }
    }

    #[tokio::test]
    async fn cancelling_one_frame_keeps_the_other_transfer_busy() {
        let dir = tempfile::tempdir().unwrap();
        let listener = Arc::new(TestListener::default());
        let engine = engine(dir.path(), listener.clone());
        let (ready, started) = tokio::sync::oneshot::channel();
        let task_engine = engine.clone();
        let task = tokio::spawn(async move {
            let _transfer = task_engine.begin_transfer(progress("first"));
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        started.await.unwrap();
        let second = engine.begin_transfer(progress("second"));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(*listener.busy.lock().unwrap(), [true]);
        assert_eq!(engine.transfers.lock().unwrap().len(), 1);
        drop(second);
        assert_eq!(*listener.busy.lock().unwrap(), [true, false]);
        assert!(engine.transfers.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unpair_clears_busy_and_late_cancellation_keeps_a_new_transfer() {
        let dir = tempfile::tempdir().unwrap();
        let listener = Arc::new(TestListener::default());
        let engine = engine(dir.path(), listener.clone());
        let old = engine.begin_transfer(progress("frame"));
        assert_eq!(engine.unpair("frame").await.unwrap(), Forgot::LocalOnly);
        assert_eq!(*listener.busy.lock().unwrap(), [true, false]);
        let new = engine.begin_transfer(progress("frame"));
        drop(old);
        assert_eq!(*listener.busy.lock().unwrap(), [true, false, true]);
        drop(new);
        assert_eq!(*listener.busy.lock().unwrap(), [true, false, true, false]);
    }

    #[tokio::test]
    async fn completed_video_restores_a_missing_index_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let listener = Arc::new(TestListener::default());
        let engine = engine(dir.path(), listener.clone());
        let host = host(&"ab".repeat(32));
        let clip =
            RemoteClip { id: "c-test".into(), kind: "clip".into(), name: "test.mp4".into(), size: 5, duration_s: Some(2.0), created: 42 };
        let sink = DirSink { root: dir.path().join("videos") };
        let path = dir.path().join("videos/clips/test.mp4");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"video").unwrap();
        let client = Client::pinned("127.0.0.1:1", &host.fingerprint, "desktop/test").unwrap();
        engine.download_one(&client, &host, &clip, 0).await.unwrap();
        let entries = Index::load(dir.path()).list();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "c-test");
        assert_eq!(entries[0].location, path.to_string_lossy());
        assert_eq!(entries[0].size, 5);
        assert_eq!(listener.synced.lock().unwrap().len(), 1);
        assert!(listener.busy.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"video");
        // A different Frame or clip may have the same name and size.
        let other = Host { fingerprint: "cd".repeat(32), ..host.clone() };
        assert!(engine.download_one(&client, &other, &clip, 0).await.is_err());
        let other_clip = RemoteClip { id: "c-other".into(), ..clip.clone() };
        assert!(engine.download_one(&client, &host, &other_clip, 0).await.is_err());
        assert_eq!(Index::load(dir.path()).list().len(), 1);
        std::fs::write(&path, b"wrong size").unwrap();
        assert_eq!(sink.completed(&host, &clip).unwrap(), None);
        let numbered = path.with_file_name("test (2).mp4");
        std::fs::write(&numbered, b"video").unwrap();
        assert_eq!(sink.completed(&host, &clip).unwrap(), None);

        // Recordings recover in the root, including after a real index loss.
        let recording = RemoteClip { id: "r-test".into(), kind: "recording".into(), ..clip };
        let recording_path = dir.path().join("videos/test.mp4");
        std::fs::write(&recording_path, b"video").unwrap();
        engine.download_one(&client, &host, &recording, 0).await.unwrap();
        std::fs::remove_file(dir.path().join("index.json")).unwrap();
        let restarted = Engine::new(Handle::current(), dir.path(), Arc::new(sink), listener, "test", "desktop/test".into());
        restarted.download_one(&client, &host, &recording, 0).await.unwrap();
        let entries = Index::load(dir.path()).list();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].location, recording_path.to_string_lossy());
        assert_eq!(std::fs::read(&recording_path).unwrap(), b"video");
        assert!(!recording_path.with_file_name("test (2).mp4").exists());
    }

    #[tokio::test]
    async fn stop_cancels_all_hosts_and_start_restarts_them() {
        let dir = tempfile::tempdir().unwrap();
        let listener = Arc::new(TestListener::default());
        let engine = engine(dir.path(), listener.clone());
        for fingerprint in ["first", "second"] {
            engine.hosts.lock().unwrap().upsert(host(fingerprint)).unwrap();
        }
        engine.start_all();
        let old: Vec<_> = engine.running.lock().unwrap().values().map(|r| r.task.abort_handle()).collect();
        assert_eq!(old.len(), 2);
        let transfer = engine.begin_transfer(progress("first"));
        engine.stop().await;
        assert!(old.iter().all(|task| task.is_finished()));
        assert!(engine.running.lock().unwrap().is_empty());
        assert_eq!(*listener.busy.lock().unwrap(), [true, false]);
        engine.start("first");
        assert!(engine.running.lock().unwrap().is_empty());
        engine.start_all();
        let new: Vec<_> = engine.running.lock().unwrap().values().map(|r| r.task.abort_handle()).collect();
        assert_eq!(new.len(), 2);
        drop(transfer);
        engine.stop().await;
        assert!(new.iter().all(|task| task.is_finished()));
        assert_eq!(engine.hosts().len(), 2);
    }

    #[test]
    fn names_stay_in_the_folder() {
        assert_eq!(safe_name("2026-09-26_14-00-00.mp4"), "2026-09-26_14-00-00.mp4");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("..\\..\\x.mp4"), "x.mp4");
        assert_eq!(safe_name(".."), "clip.mp4");
        assert_eq!(safe_name("a:b?.mp4"), "ab.mp4");
    }

    #[test]
    fn concurrent_downloads_keep_every_completed_video() {
        let dir = tempfile::tempdir().unwrap();
        let sink = DirSink { root: dir.path().to_path_buf() };
        let host = host(&"ab".repeat(32));
        let clip =
            RemoteClip { id: "r-test".into(), kind: "recording".into(), name: "test.mp4".into(), size: 1, duration_s: None, created: 0 };
        std::fs::create_dir_all(sink.dir(&clip)).unwrap();
        let barrier = std::sync::Barrier::new(32);
        std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..32u8)
                .map(|n| {
                    let (sink, host, clip, barrier) = (&sink, &host, &clip, &barrier);
                    let staged = dir.path().join(format!("{n}.part"));
                    std::fs::write(&staged, [n]).unwrap();
                    scope.spawn(move || {
                        barrier.wait();
                        sink.finish(&staged, host, clip).unwrap()
                    })
                })
                .collect();
            let places: std::collections::HashSet<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
            assert_eq!(places.len(), 32);
            let contents: std::collections::HashSet<_> = places.into_iter().map(|p| std::fs::read(p).unwrap()).collect();
            assert_eq!(contents.len(), 32);
        });
        assert!(sink.finish(&dir.path().join("missing.part"), &host, &clip).is_err());
        assert_eq!(std::fs::read_dir(sink.dir(&clip)).unwrap().count(), 32);
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
