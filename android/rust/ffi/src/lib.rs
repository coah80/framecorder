//! The sync core for the Android app, over UniFFI. It's the engine the
//! desktop app runs; Kotlin says where finished files go, hears what
//! happened, and finds Frames with NSD.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use framecorder_app_lib::core::api::{self, Client, RemoteClip};
use framecorder_app_lib::core::engine::{self, Engine};
use framecorder_app_lib::core::{discover, pairlink, store, tls};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// What went wrong, in words the app can show.
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<String> for CoreError {
    fn from(s: String) -> Self {
        CoreError::Failed { reason: s }
    }
}

impl From<&str> for CoreError {
    fn from(s: &str) -> Self {
        CoreError::Failed { reason: s.into() }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for CoreError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        CoreError::Failed { reason: e.reason }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FrameState {
    Connecting,
    Connected,
    Unreachable,
    /// The Frame doesn't accept our token anymore.
    Unpaired,
    WrongFingerprint,
    /// This phone is out of space; the clips wait on the Frame.
    Full,
}

impl From<engine::State> for FrameState {
    fn from(s: engine::State) -> Self {
        match s {
            engine::State::Connecting => Self::Connecting,
            engine::State::Connected => Self::Connected,
            engine::State::Unreachable => Self::Unreachable,
            engine::State::Unpaired => Self::Unpaired,
            engine::State::WrongFingerprint => Self::WrongFingerprint,
            engine::State::Full => Self::Full,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FrameStatus {
    pub fingerprint: String,
    pub name: String,
    /// Last address that worked, `ip:port`.
    pub addr: String,
    pub state: FrameState,
    pub message: Option<String>,
}

impl From<&engine::Status> for FrameStatus {
    fn from(s: &engine::Status) -> Self {
        Self {
            fingerprint: s.fingerprint.clone(),
            name: s.name.clone(),
            addr: s.addr.clone(),
            state: s.state.into(),
            message: s.message.clone(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncProgress {
    pub fingerprint: String,
    pub id: String,
    pub name: String,
    pub done: i64,
    pub total: i64,
    /// Files waiting after this one.
    pub queued: i32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Clip {
    /// Stable across restarts: `<fingerprint, 16 chars>/<id>`.
    pub key: String,
    pub host: String,
    pub id: String,
    pub name: String,
    /// "clip" or "recording".
    pub kind: String,
    pub size: i64,
    /// Unix seconds, when the Frame saved it.
    pub created: i64,
    pub duration_s: Option<f64>,
    /// The content:// uri MediaStore gave it.
    pub location: String,
    pub synced_at: i64,
}

impl From<&store::Entry> for Clip {
    fn from(e: &store::Entry) -> Self {
        Self {
            key: e.key(),
            host: e.host.clone(),
            id: e.id.clone(),
            name: e.name.clone(),
            kind: e.kind.clone(),
            size: e.size as i64,
            created: e.created,
            duration_s: e.duration_s,
            location: e.location.clone(),
            synced_at: e.synced_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FoundFrame {
    pub name: String,
    pub fingerprint: String,
    pub addr: String,
    pub addrs: Vec<String>,
}

impl From<discover::Found> for FoundFrame {
    fn from(f: discover::Found) -> Self {
        Self { name: f.name, fingerprint: f.fingerprint, addr: f.addr, addrs: f.addrs }
    }
}

/// How the headset itself is doing. Anything it can't tell is null.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FrameInfo {
    pub battery_percent: Option<u8>,
    /// On the charger, charging or full.
    pub charging: bool,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    /// How much of it framecorder's videos take.
    pub videos_bytes: Option<u64>,
}

impl From<&api::About> for FrameInfo {
    fn from(a: &api::About) -> Self {
        Self {
            battery_percent: a.battery.as_ref().map(|b| b.percent),
            charging: a.battery.as_ref().is_some_and(|b| b.charging),
            free_bytes: a.storage.as_ref().map(|s| s.free),
            total_bytes: a.storage.as_ref().map(|s| s.total),
            videos_bytes: a.storage.as_ref().map(|s| s.videos),
        }
    }
}

/// What the Frame's dashboard tab is up to, for the remote.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RemoteState {
    /// The tab's running; it starts with SteamVR.
    pub available: bool,
    /// Its setup's been done, so it can record.
    pub ready: bool,
    pub recording: bool,
    /// Recording and not paused (it pauses while the tab's on screen).
    pub running: bool,
    /// How long it's been recording, as of when this was sent.
    pub elapsed_ms: u64,
    /// The clip length while clipping's on.
    pub clip_secs: Option<u32>,
    /// A clip can be saved right now.
    pub clip_ready: bool,
}

impl From<&api::Remote> for RemoteState {
    fn from(r: &api::Remote) -> Self {
        Self {
            available: r.available,
            ready: r.ready,
            recording: r.recording,
            running: r.running,
            elapsed_ms: r.elapsed_ms,
            clip_secs: r.clips,
            clip_ready: r.clip_ready,
        }
    }
}

/// The Frame's recording settings; they apply from its next recording.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RecordingSettings {
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
    /// 15, 30, 60 or 120
    pub clip_secs: u32,
}

impl From<api::Recording> for RecordingSettings {
    fn from(r: api::Recording) -> Self {
        Self { shape: r.shape, quality: r.quality, fps: r.fps, game_audio: r.game_audio, mic: r.mic, clips: r.clips, clip_secs: r.clip }
    }
}

impl From<RecordingSettings> for api::Recording {
    fn from(r: RecordingSettings) -> Self {
        Self { shape: r.shape, quality: r.quality, fps: r.fps, game_audio: r.game_audio, mic: r.mic, clips: r.clips, clip: r.clip_secs }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PairLinkInfo {
    pub addr: String,
    pub fingerprint: String,
    pub code: String,
    pub name: String,
}

/// Where finished downloads go: MediaStore, so they show up in the gallery.
#[uniffi::export(with_foreign)]
pub trait Gallery: Send + Sync {
    /// Moves the finished download at `path` into Movies/framecorder/`subdir`
    /// as `name` and returns its content:// uri. Called off the main thread.
    fn save(&self, path: String, name: String, subdir: String) -> Result<String, CoreError>;
}

/// What the engine has to say. Called from the engine's own threads, so
/// implementations hand things over and return quickly.
#[uniffi::export(with_foreign)]
pub trait Events: Send + Sync {
    fn status(&self, status: FrameStatus);
    fn progress(&self, progress: SyncProgress);
    fn synced(&self, clip: Clip);
    /// A download started, or everything's done (for the wake lock).
    fn busy(&self, busy: bool);
    /// A clip disappeared from the Frame.
    fn removed(&self, host: String, id: String);
    /// What the Frame's tab is up to; null once we've lost touch with it.
    fn remote(&self, host: String, state: Option<RemoteState>);
    /// The headset's battery and storage; null once we've lost touch with it.
    fn about(&self, host: String, info: Option<FrameInfo>);
}

/// Finding a Frame the platform's way (Android's NSD), so mDNS works
/// without a multicast lock.
#[uniffi::export(with_foreign)]
pub trait Finder: Send + Sync {
    /// Addresses (`ip:port`) where the Frame with this fingerprint answers,
    /// waiting up to `timeout_ms`. Empty if it isn't around.
    fn find(&self, fingerprint: String, timeout_ms: u32) -> Vec<String>;
}

struct GallerySink {
    gallery: Arc<dyn Gallery>,
    staging: PathBuf,
}

impl engine::Sink for GallerySink {
    fn staging(&self, host: &store::Host, clip: &RemoteClip) -> PathBuf {
        let short = &host.fingerprint[..8.min(host.fingerprint.len())];
        self.staging.join(format!("{short}-{}.part", engine::safe_name(&clip.name)))
    }

    /// The download in our cache, and the gallery's copy of it.
    fn copies(&self) -> u64 {
        2
    }

    fn finish(&self, staged: &Path, _host: &store::Host, clip: &RemoteClip) -> Result<String, String> {
        let subdir = if clip.kind == "clip" { "clips" } else { "" };
        let (path, name) = (staged.to_string_lossy().into_owned(), engine::safe_name(&clip.name));
        // the copy can take a while for a long recording
        tokio::task::block_in_place(|| self.gallery.save(path, name, subdir.into())).map_err(|e| e.to_string())
    }
}

struct Relay(Arc<dyn Events>);

impl engine::Listener for Relay {
    fn status(&self, s: &engine::Status) {
        self.0.status(s.into());
    }

    fn progress(&self, p: &engine::Progress) {
        self.0.progress(SyncProgress {
            fingerprint: p.fingerprint.clone(),
            id: p.id.clone(),
            name: p.name.clone(),
            done: p.done as i64,
            total: p.total as i64,
            queued: p.queued as i32,
        });
    }

    fn synced(&self, e: &store::Entry) {
        self.0.synced(e.into());
    }

    fn busy(&self, busy: bool) {
        self.0.busy(busy);
    }

    fn removed(&self, host: &str, id: &str) {
        self.0.removed(host.into(), id.into());
    }

    fn remote(&self, host: &str, remote: Option<&api::Remote>) {
        self.0.remote(host.into(), remote.map(RemoteState::from));
    }

    fn about(&self, host: &str, about: Option<&api::About>) {
        self.0.about(host.into(), about.map(FrameInfo::from));
    }
}

struct FinderRelay(Arc<dyn Finder>);

impl engine::Finder for FinderRelay {
    fn find(&self, fingerprint: &str, window: Duration) -> Vec<String> {
        self.0.find(fingerprint.into(), window.as_millis().min(u32::MAX as u128) as u32)
    }
}

fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("framecorder"),
    );
}

/// `host` or `host:port`, with the Frame's default port when none is given.
fn with_port(addr: &str) -> Result<String, CoreError> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("which frame? pick one or type its address".into());
    }
    let has_port = match addr.rsplit_once(':') {
        Some((host, port)) => !host.is_empty() && port.parse::<u16>().is_ok() && (!host.contains(':') || host.ends_with(']')),
        None => false,
    };
    Ok(if has_port { addr.to_string() } else { pairlink::join_addr(addr, discover::PORT) })
}

/// One running sync engine: owns its runtime and every paired Frame's loop.
#[derive(uniffi::Object)]
pub struct Core {
    rt: tokio::runtime::Runtime,
    engine: Arc<Engine>,
}

impl Core {
    fn status_of(&self, fingerprint: &str) -> Result<FrameStatus, CoreError> {
        self.engine
            .statuses()
            .iter()
            .find(|s| s.fingerprint == fingerprint)
            .map(FrameStatus::from)
            .ok_or_else(|| "paired, but it went missing right after".into())
    }

    async fn on_rt<T: Send + 'static>(&self, f: impl std::future::Future<Output = T> + Send + 'static) -> Result<T, CoreError> {
        self.rt.spawn(f).await.map_err(|e| CoreError::Failed { reason: format!("that fell over: {e}") })
    }
}

#[uniffi::export]
impl Core {
    /// `state_dir` keeps pairings and the index, `staging_dir` holds
    /// downloads until they're handed to the gallery. Without a `finder`
    /// the core does its own mDNS (which needs a multicast lock).
    #[uniffi::constructor]
    pub fn new(
        state_dir: String,
        staging_dir: String,
        device_name: String,
        gallery: Arc<dyn Gallery>,
        events: Arc<dyn Events>,
        finder: Option<Arc<dyn Finder>>,
    ) -> Result<Arc<Self>, CoreError> {
        init_logging();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("framecorder-sync")
            .enable_all()
            .build()
            .map_err(|e| CoreError::Failed { reason: format!("couldn't start syncing: {e}") })?;
        std::fs::create_dir_all(&state_dir).map_err(|e| CoreError::Failed { reason: format!("can't use {state_dir}: {e}") })?;
        std::fs::create_dir_all(&staging_dir).map_err(|e| CoreError::Failed { reason: format!("can't use {staging_dir}: {e}") })?;
        let sink = Arc::new(GallerySink { gallery, staging: PathBuf::from(staging_dir) });
        let engine = Engine::new(rt.handle().clone(), Path::new(&state_dir), sink, Arc::new(Relay(events)), &device_name);
        match finder {
            Some(f) => engine.set_finder(Some(Arc::new(FinderRelay(f))), false),
            None => engine.set_finder(None, true),
        }
        Ok(Arc::new(Self { rt, engine }))
    }

    /// Starts following every paired Frame. Safe to call again.
    pub fn start(&self) {
        self.engine.start_all();
    }

    pub fn statuses(&self) -> Vec<FrameStatus> {
        self.engine.statuses().iter().map(FrameStatus::from).collect()
    }

    /// Everything synced so far, newest first.
    pub fn clips(&self) -> Vec<Clip> {
        self.engine.clips().iter().map(Clip::from).collect()
    }

    /// Skips the backoff (and the sweep's cooldown) and tries every Frame now.
    pub fn retry_now(&self) {
        self.engine.retry_now();
    }

    pub fn unpair(&self, fingerprint: String) -> Result<(), CoreError> {
        Ok(self.engine.unpair(&fingerprint)?)
    }

    /// Pairs with the Frame at `addr` (`ip` or `ip:port`) using the code it
    /// shows. With a fingerprint only that exact Frame is accepted.
    /// `replaces` is a paired Frame this one stands for: its clips carry
    /// over and its old pairing goes.
    pub async fn pair(
        &self,
        addr: String,
        fingerprint: Option<String>,
        code: String,
        replaces: Option<String>,
    ) -> Result<FrameStatus, CoreError> {
        let code = code.trim().to_string();
        if !pairlink::is_code(&code) {
            return Err("the code is the 6 digits shown on your frame".into());
        }
        let addr = with_port(&addr)?;
        let fp = match fingerprint.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => Some(tls::normalize_fingerprint(f).ok_or("that fingerprint doesn't look right")?),
            None => None,
        };
        let engine = self.engine.clone();
        let host = self.on_rt(async move { engine.pair(&addr, fp.as_deref(), &code, replaces.as_deref()).await }).await??;
        self.status_of(&host.fingerprint)
    }

    /// Pairs from the `framecorder://pair?...` link in the Frame's QR code.
    pub async fn pair_link(&self, link: String, replaces: Option<String>) -> Result<FrameStatus, CoreError> {
        let l = pairlink::parse(&link)?;
        let engine = self.engine.clone();
        let host = self
            .on_rt(async move { engine.pair(&l.addr, Some(&l.fingerprint), &l.code, replaces.as_deref()).await })
            .await??;
        self.status_of(&host.fingerprint)
    }

    /// Asks every address nearby on the Frame's port, for networks where
    /// mDNS doesn't get through.
    pub async fn sweep(&self, seconds: u32) -> Vec<FoundFrame> {
        let window = Duration::from_secs(seconds.clamp(1, 30) as u64);
        self.on_rt(async move {
            let mut found = Vec::new();
            discover::sweep(window, |f| {
                found.push(FoundFrame::from(f));
                true
            })
            .await;
            found
        })
        .await
        .unwrap_or_default()
    }

    /// Who answers at `addr`, if it's a framecorder-sync: its name and
    /// fingerprint, for showing before pairing with a typed-in address.
    pub async fn identify(&self, addr: String) -> Result<FoundFrame, CoreError> {
        let addr = with_port(&addr)?;
        let shown = addr.clone();
        self.on_rt(async move { discover::identify(&addr).await })
            .await?
            .map(FoundFrame::from)
            .ok_or_else(|| CoreError::Failed { reason: format!("nothing that looks like a frame answered at {shown}") })
    }

    /// Asks the Frame to `record`, `stop` or `clip`. Fails with why not,
    /// like the tab not running.
    pub async fn command(&self, fingerprint: String, what: String) -> Result<RemoteState, CoreError> {
        let engine = self.engine.clone();
        let remote = self.on_rt(async move { engine.command(&fingerprint, &what).await }).await??;
        Ok(RemoteState::from(&remote))
    }

    /// The Frame's recording settings, or `None` when its framecorder is
    /// older than them.
    pub async fn recording(&self, fingerprint: String) -> Result<Option<RecordingSettings>, CoreError> {
        let engine = self.engine.clone();
        Ok(self.on_rt(async move { engine.recording(&fingerprint).await }).await??.map(RecordingSettings::from))
    }

    /// Saves them on the Frame, for its next recording. Returns what it kept.
    pub async fn set_recording(&self, fingerprint: String, settings: RecordingSettings) -> Result<RecordingSettings, CoreError> {
        let engine = self.engine.clone();
        let settings = api::Recording::from(settings);
        Ok(self.on_rt(async move { engine.set_recording(&fingerprint, &settings).await }).await??.into())
    }

    /// For the connection check: does the paired Frame answer at its last
    /// address with the certificate we paired with? Says why not.
    pub async fn check(&self, fingerprint: String) -> Result<(), CoreError> {
        let host = self.engine.host(&fingerprint).ok_or("that frame isn't paired")?;
        self.on_rt(async move {
            let client = Client::pinned(&host.addr, &host.fingerprint).map_err(|e| e.to_string())?.with_token(&host.token);
            client.hello().await.map(|_| ()).map_err(|e| e.to_string())
        })
        .await??;
        Ok(())
    }
}

/// Reads the link in the Frame's pairing QR code.
#[uniffi::export]
pub fn parse_pair_link(link: String) -> Result<PairLinkInfo, CoreError> {
    let l = pairlink::parse(&link)?;
    Ok(PairLinkInfo { addr: l.addr, fingerprint: l.fingerprint, code: l.code, name: l.name })
}

/// The networks a sweep covers, like "192.168.1.x", for saying so on screen.
#[uniffi::export]
pub fn sweep_networks() -> Vec<String> {
    let mut nets: Vec<String> = discover::sweep_targets()
        .iter()
        .map(|ip| {
            let o = ip.octets();
            format!("{}.{}.{}.x", o[0], o[1], o[2])
        })
        .collect();
    nets.dedup();
    nets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_get_the_default_port() {
        assert_eq!(with_port("192.168.1.42").unwrap(), "192.168.1.42:38619");
        assert_eq!(with_port(" 192.168.1.42:4000 ").unwrap(), "192.168.1.42:4000");
        assert_eq!(with_port("fe80::1").unwrap(), "[fe80::1]:38619");
        assert_eq!(with_port("[fe80::1]:4000").unwrap(), "[fe80::1]:4000");
        assert!(with_port("  ").is_err());
    }
}
