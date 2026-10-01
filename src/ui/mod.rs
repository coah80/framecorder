//! The dashboard tab: start and stop recordings, and clip, from inside the
//! headset.
//!
//! It stays out of the way: nothing gets drawn unless the tab is on screen
//! and something changed, and it mostly sleeps otherwise. Recording pauses
//! while this tab is showing, so the controls never end up in the video.

mod pairing;
mod paint;
mod recorder;
mod settings;
mod text;
mod texture;
mod view;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::input::Input;
use crate::openvr::{AppType, OpenVr};
use crate::overlay::{DashboardTab, Event};
use crate::toast::Toast;
use recorder::{Kind, Recorder};
use settings::Settings;
use text::Fonts;
use pairing::{Device, Pairing};
use view::{Action, Hit, Model, Screen, Section, Status, Step, SyncView};

const KEY: &str = "framecorder.dashboard";
const TOAST_KEY: &str = "framecorder.toast";
const WIDTH_METERS: f32 = 2.2;
const TOAST_METERS: f32 = 0.32;
const TOAST_FOR: Duration = Duration::from_millis(2500);
/// Wait this long after our tab goes away before recording again, so its
/// fade out doesn't make it into the video.
const RESUME_DELAY: Duration = Duration::from_millis(350);
/// Before starting the clipping recorder again after it died.
const RETRY_AFTER: Duration = Duration::from_secs(5);
const TICK_ACTIVE: Duration = Duration::from_millis(30);
const TICK_IDLE: Duration = Duration::from_millis(250);
const SERVER_POLL: Duration = Duration::from_secs(2);
/// Shortest time between two clips, however they're asked for.
const CLIP_COOLDOWN: Duration = Duration::from_secs(1);
/// How often the paired devices get looked up while the tab's on screen.
const SYNC_POLL: Duration = Duration::from_secs(3);

static QUIT: AtomicBool = AtomicBool::new(false);
/// Set when an update replaced this program, to restart into it.
static UPDATED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, Ordering::SeqCst);
}

/// What the tab knows about syncing, looked up again every so often while
/// it's on screen.
struct Sync {
    available: bool,
    devices: Vec<Device>,
    /// The code on show while pairing.
    pairing: Option<Result<Pairing, String>>,
    checked: Instant,
}

impl Sync {
    fn look() -> Self {
        Self { available: pairing::available(), devices: pairing::devices(), pairing: None, checked: Instant::now() }
    }

    fn stop_pairing(&mut self) {
        if self.pairing.take().is_some() {
            pairing::end();
        }
    }
}

/// A recording in progress, as far as the tab is concerned.
struct Recording {
    /// Recorded time, not counting pauses.
    recorded: Duration,
    running_since: Option<Instant>,
    stopping: bool,
}

impl Recording {
    fn recorded(&self) -> Duration {
        self.recorded + self.running_since.map_or(Duration::ZERO, |s| s.elapsed())
    }
}

struct App {
    settings: Settings,
    recorder: Option<Recorder>,
    recording: Option<Recording>,
    note: Option<(String, bool)>,
    hover: Option<Action>,
    hits: Vec<Hit>,
    dirty: bool,
    shown_second: u64,
    tab_hidden_at: Option<Instant>,
    retry_at: Option<Instant>,
    /// A toast to show, and until when one is up.
    toast: Option<(String, bool)>,
    toast_until: Option<Instant>,
    buzz: bool,
    screen: Screen,
    sync: Sync,
    /// When the last clip was asked for, while the button is still cooling down.
    clipped_at: Option<Instant>,
    /// Whether the recorder may read the panels, or records SteamVR's view.
    unlocked: bool,
    /// Remove was tapped once, the next tap does it.
    uninstall_armed: bool,
    /// An update took the panels' permission away.
    relocked: bool,
}

/// Draws the tab into a raw RGBA file instead of showing it, for checking
/// the look without a headset on. `state` is idle, recording, saved or toast,
/// a settings section, or a step of the setup (setup-welcome and so on).
pub fn preview(path: &Path, state: &str) -> Result<()> {
    let mut fonts = Fonts::load()?;
    if state == "toast" {
        let c = view::toast(&mut fonts, "Clipped the last 30 s", true);
        std::fs::write(path, &c.pixels)?;
        return Ok(());
    }
    let mut canvas = paint::Canvas::new(view::WIDTH, view::HEIGHT);
    let settings = Settings::load();
    let status = match state {
        "recording" | "locked" => Status::Recording { recorded: Duration::from_secs(754) },
        _ => Status::Idle,
    };
    let screen = match state {
        "video" | "locked" | "remove" => Screen::Settings(Section::Video),
        "audio" => Screen::Settings(Section::Audio),
        "clips" => Screen::Settings(Section::Clips),
        "sync" | "pair" => Screen::Settings(Section::Sync),
        "setup-welcome" | "setup-locked-out" => Screen::Onboarding(Step::Welcome),
        "setup-shape" | "setup-both-eyes" => Screen::Onboarding(Step::Shape),
        "setup-quality" => Screen::Onboarding(Step::Quality),
        "setup-audio" => Screen::Onboarding(Step::Audio),
        "setup-clips" | "setup-no-sync" => Screen::Onboarding(Step::Clips),
        "setup-sync" | "setup-pair" => Screen::Onboarding(Step::Sync),
        "setup-done" => Screen::Onboarding(Step::Done),
        _ => Screen::Home,
    };
    let settings = match state {
        "setup-both-eyes" => Settings { shape: settings::Shape::BothEyes, ..settings },
        _ => settings,
    };
    let sample = Pairing::sample()?;
    let devices = [Device { id: "a".into(), name: "Pixel 9".into() }, Device { id: "b".into(), name: "COLEPCWIN".into() }];
    let pairing = matches!(state, "pair" | "setup-pair").then_some(Ok(&sample));
    let sync = SyncView { available: state != "setup-no-sync", devices: &devices, pairing };
    let note = (state == "saved").then_some(("Saved 2026-09-23_01-43-02.mp4 · 00:12:34 · 3771.2 MB", true));
    let hover = (state == "idle").then_some(Action::Record);
    let unlocked = !matches!(state, "locked-out" | "setup-locked-out" | "relocked");
    view::draw(&mut canvas, &mut fonts, &Model { settings: &settings, screen, sync, clip_cooling: false, unlocked, status, hover, note, uninstall_armed: state == "remove", relocked: state == "relocked" });
    std::fs::write(path, &canvas.pixels)?;
    Ok(())
}

pub fn run() -> Result<()> {
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
    }
    // Only ever attach to a SteamVR that's already running, never start one.
    let mut waited = false;
    while !OpenVr::server_running() {
        if QUIT.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !waited {
            log::info!("waiting for SteamVR");
            waited = true;
        }
        std::thread::sleep(SERVER_POLL);
    }
    // Closing framecorder from the tab stops sync too, this is it coming back.
    let _ = Command::new("systemctl").args(["--user", "--no-block", "start", "framecorder-sync.service"]).status();
    let vr = OpenVr::connect_as(AppType::Overlay)?;
    let tab = DashboardTab::new(&vr, KEY, "framecorder", view::WIDTH, view::HEIGHT, WIDTH_METERS)?;
    let icon = view::icon(128);
    tab.set_thumbnail(&icon.pixels, icon.width, icon.height)?;
    if std::env::args().any(|a| a == "--show") {
        tab.show(KEY);
    }
    let input = Input::new(&vr).map_err(|e| log::warn!("no clip keybind: {e:#}")).ok();
    let toast = Toast::new(&vr, TOAST_KEY, "framecorder", TOAST_METERS).map_err(|e| log::warn!("no clip toasts: {e:#}")).ok();

    // A GPU texture swaps in cleanly; raw uploads make the tab blink.
    let mut gpu_texture = match texture::OverlayTexture::new(&vr, view::WIDTH, view::HEIGHT) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("falling back to raw uploads, the tab may flicker: {e:#}");
            None
        }
    };

    let mut fonts = Fonts::load()?;
    let mut canvas = paint::Canvas::new(view::WIDTH, view::HEIGHT);
    let settings = Settings::load();
    let mut app = App {
        settings,
        recorder: None,
        recording: None,
        note: None,
        hover: None,
        hits: Vec::new(),
        dirty: true,
        shown_second: u64::MAX,
        tab_hidden_at: None,
        retry_at: None,
        toast: None,
        toast_until: None,
        buzz: false,
        // The first time round the setup comes before anything else.
        screen: if settings.onboarded { Screen::Home } else { Screen::Onboarding(Step::Welcome) },
        sync: Sync::look(),
        clipped_at: None,
        unlocked: crate::setup::recorder().is_ok_and(|r| crate::setup::unlocked(&r)),
        uninstall_armed: false,
        relocked: crate::setup::relocked(),
    };
    log::info!("dashboard tab ready");

    while !QUIT.load(Ordering::Relaxed) {
        while let Some(event) = tab.poll() {
            if !app.handle(event) {
                QUIT.store(true, Ordering::SeqCst);
            }
        }
        if input.as_ref().is_some_and(|i| i.clip_pressed()) {
            app.clip("keybind");
        }
        // On the Frame the dashboard counts as open the whole time you're in
        // the home space, so our own tab being on screen is what matters.
        let tab_visible = tab.is_visible();
        app.manage_recorder();
        app.auto_pause(tab_visible);
        app.check_recorder();
        // An update swapped this program out. Restart into it once nothing's
        // being recorded and nobody's using the tab.
        if !tab_visible && app.recording.is_none() && replaced() {
            log::info!("framecorder was updated, restarting into the new version");
            UPDATED.store(true, Ordering::SeqCst);
            break;
        }
        if tab_visible {
            app.check_sync();
        } else if matches!(app.screen, Screen::Settings(_)) {
            // Next time the tab opens it's back on the record button.
            app.go(Screen::Home);
        } else {
            // The setup stays where it was, just without a code on show.
            app.sync.stop_pairing();
        }

        if let (Some((text, ok)), Some(t)) = (app.toast.take(), &toast) {
            let c = view::toast(&mut fonts, &text, ok);
            match t.show(&c.pixels, c.width, c.height) {
                Ok(()) => app.toast_until = Some(Instant::now() + TOAST_FOR),
                Err(e) => log::warn!("{e:#}"),
            }
        }
        if app.toast_until.is_some_and(|t| Instant::now() >= t) {
            app.toast_until = None;
            if let Some(t) = &toast {
                t.hide();
            }
        }
        if std::mem::take(&mut app.buzz) {
            if let Some(i) = &input {
                i.buzz();
            }
        }

        if tab_visible && app.needs_redraw() {
            let model = app.model();
            app.hits = view::draw(&mut canvas, &mut fonts, &model);
            let shown = match &mut gpu_texture {
                Some(t) => t
                    .upload(&canvas.pixels)
                    // The texture and the data it points at both live in `t`.
                    .and_then(|tex| unsafe { tab.set_texture((&tex as *const texture::Texture).cast()) }),
                None => tab.set_pixels(&canvas.pixels),
            };
            if let Err(e) = shown {
                log::warn!("{e:#}");
            }
            app.dirty = false;
        }

        // The keybind needs quick polling to catch a press; otherwise idle slowly.
        let busy = tab_visible || app.recording.is_some() || app.toast_until.is_some() || (input.is_some() && app.recorder.is_some());
        std::thread::sleep(if busy { TICK_ACTIVE } else { TICK_IDLE });
    }

    if let Some(rec) = app.recorder.take() {
        log::info!("shutting down, finishing up the recorder");
        rec.finish();
    }
    // SteamVR's client keeps objects on our Vulkan device for the shared
    // texture and cleans them up when we disconnect, so disconnect while the
    // device still exists.
    drop(toast);
    drop(tab);
    drop(vr);
    drop(gpu_texture);
    if UPDATED.load(Ordering::Relaxed) {
        // Not a clean exit, so systemd (Restart=on-failure) starts the new one.
        std::process::exit(75);
    }
    Ok(())
}

/// Whether the program file this was started from has been replaced, which
/// is how updates land (see setup::place).
fn replaced() -> bool {
    std::fs::read_link("/proc/self/exe").is_ok_and(|p| p.to_string_lossy().ends_with(" (deleted)"))
}

impl App {
    /// Returns false when SteamVR wants us gone.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::MouseMove { x, y } => {
                let hover = self.hit(x, y);
                if hover != self.hover {
                    self.hover = hover;
                    self.dirty = true;
                }
            }
            Event::MouseDown { x, y } => {
                if let Some(action) = self.hit(x, y) {
                    self.act(action);
                }
            }
            Event::Shown => self.dirty = true,
            Event::Quit => return false,
            _ => {}
        }
        true
    }

    fn hit(&self, x: f32, y: f32) -> Option<Action> {
        self.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.action)
    }

    fn act(&mut self, action: Action) {
        self.dirty = true;
        // Anything else tapped in between calls the removal off.
        let armed = std::mem::take(&mut self.uninstall_armed);
        let s = &mut self.settings;
        match action {
            Action::Uninstall if !armed => return self.uninstall_armed = true,
            Action::Uninstall => return self.uninstall(),
            Action::TurnOff => return self.turn_off(),
            Action::Record => return self.toggle_recording(),
            Action::ClipNow => return self.clip("tab button"),
            Action::Open(section) => return self.go(Screen::Settings(section)),
            Action::Close => return self.go(Screen::Home),
            Action::Step(step) => return self.go(Screen::Onboarding(step)),
            Action::FinishSetup => return self.finish_setup(),
            Action::Pair => return self.sync.pairing = Some(Pairing::begin().map_err(|e| format!("{e:#}"))),
            Action::ClosePair => return self.sync.stop_pairing(),
            Action::RemoveDevice(i) => {
                if let Some(d) = self.sync.devices.get(i) {
                    match pairing::remove(&d.id) {
                        Ok(()) => self.note = Some((format!("Removed {}", d.name), true)),
                        Err(e) => self.note = Some((format!("Couldn't remove {}: {e:#}", d.name), false)),
                    }
                }
                self.sync.devices = pairing::devices();
                return;
            }
            Action::Shape(shape) => s.shape = shape,
            Action::Eye(eye) => s.eye = eye,
            Action::Quality(q) => s.quality = q,
            Action::Fps(fps) => s.fps = fps,
            Action::GameAudio => s.game_audio = !s.game_audio,
            Action::Mic => s.mic = !s.mic,
            Action::Clips => {
                s.clips = !s.clips;
                self.retry_at = None;
                // The setup says how to clip itself, and nothing's running yet.
                if s.clips && s.onboarded {
                    self.toast = Some(("Clips are on · hold the left stick to clip".into(), true));
                }
            }
            Action::ClipLength(secs) => {
                s.clip_secs = secs;
                self.retry_at = None;
            }
        }
        s.save();
    }

    /// Stops the tab (and with it the recorder and the clip buffer) and sync,
    /// until SteamVR starts again. Stopping our own service ends this
    /// process, so systemd does it without waiting on us.
    fn turn_off(&mut self) {
        let stopped = Command::new("systemctl")
            .args(["--user", "--no-block", "stop", "framecorder-sync.service", "framecorder-ui.service"])
            .status();
        match stopped {
            Ok(s) if s.success() => self.note = Some(("Closing. framecorder starts again with SteamVR.".into(), true)),
            Ok(s) => self.note = Some((format!("Couldn't close framecorder: systemctl {s}"), false)),
            Err(e) => self.note = Some((format!("Couldn't close framecorder: {e}"), false)),
        }
    }

    /// Hands the removal to systemd, since it stops this tab's own service.
    fn uninstall(&mut self) {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let started = Command::new("systemd-run").args(["--user", "--collect", "--quiet", &exe, "--uninstall"]).status();
        match started {
            Ok(s) if s.success() => self.note = Some(("Removing framecorder. Your videos stay in Videos/framecorder.".into(), true)),
            Ok(s) => self.note = Some((format!("Couldn't remove framecorder: systemd-run {s}"), false)),
            Err(e) => self.note = Some((format!("Couldn't remove framecorder: {e}"), false)),
        }
    }

    /// The setup's over, finished or skipped: on to the record button, and
    /// from here on the recorder may run.
    fn finish_setup(&mut self) {
        self.settings = Settings { onboarded: true, ..self.settings };
        if !self.settings.save() {
            self.note = Some(("Couldn't save your settings, so the setup will show again next time".into(), false));
        }
        self.go(Screen::Home);
    }

    fn toggle_recording(&mut self) {
        if !self.settings.onboarded {
            return;
        }
        if let Some(rec) = &mut self.recording {
            if !rec.stopping {
                rec.stopping = true;
                if let Some(r) = &mut self.recorder {
                    r.stop_record();
                }
            }
            return;
        }
        if self.recorder.as_ref().is_some_and(|r| r.quitting()) {
            return;
        }
        if self.recorder.is_none() {
            match Recorder::start(&self.settings, self.unlocked) {
                Ok(r) => self.recorder = Some(r),
                Err(e) => {
                    self.note = Some((format!("Couldn't start: {e:#}"), false));
                    return;
                }
            }
        }
        let Some(r) = &mut self.recorder else { return };
        match r.record() {
            Ok(()) => {
                self.recording = Some(Recording { recorded: Duration::ZERO, running_since: None, stopping: false });
                self.note = None;
            }
            Err(e) => self.note = Some((format!("Couldn't start: {e:#}"), false)),
        }
    }

    fn clip(&mut self, from: &str) {
        if self.clipped_at.is_some_and(|t| t.elapsed() < CLIP_COOLDOWN) {
            return;
        }
        if !self.settings.onboarded {
            self.toast = Some(("Finish setting up in the framecorder tab first".into(), false));
            return;
        }
        log::info!("clip asked for by the {from}");
        match &mut self.recorder {
            Some(r) if r.replay.is_some() && !r.quitting() => {
                r.clip();
                self.clipped_at = Some(Instant::now());
            }
            _ => {
                let why = if self.settings.clipping().is_some() { "Clipping is starting up" } else { "Clipping is off" };
                self.toast = Some((why.into(), false));
            }
        }
    }

    /// Keeps a recorder running while clipping is on, restarts it when its
    /// settings changed (between recordings), and lets it go when neither a
    /// recording nor clipping needs it.
    fn manage_recorder(&mut self) {
        // Nothing runs until the setup's done, so picking things in it
        // doesn't start and stop the recorder over and over.
        if !self.settings.onboarded {
            return;
        }
        let wanted = self.settings.clipping().is_some();
        if let Some(r) = &mut self.recorder {
            let stale = r.replay != self.settings.clipping() || r.args != self.settings.recorder_args(self.unlocked);
            if self.recording.is_none() && !r.quitting() && (stale || (!wanted && r.replay.is_some())) {
                r.quit();
            }
            return;
        }
        if wanted && self.retry_at.is_none_or(|t| Instant::now() >= t) {
            match Recorder::start(&self.settings, self.unlocked) {
                Ok(r) => self.recorder = Some(r),
                Err(e) => {
                    self.note = Some((format!("Couldn't start clipping: {e:#}"), false));
                    self.retry_at = Some(Instant::now() + RETRY_AFTER);
                }
            }
            self.dirty = true;
        }
    }

    /// Pauses while our tab is on screen, and resumes a moment after it's
    /// gone (closed, or another tab picked).
    fn auto_pause(&mut self, tab_visible: bool) {
        if tab_visible {
            self.tab_hidden_at = None;
        } else if self.tab_hidden_at.is_none() {
            self.tab_hidden_at = Some(Instant::now());
        }
        let Some(r) = &mut self.recorder else { return };
        let want_paused = tab_visible || self.tab_hidden_at.is_some_and(|t| t.elapsed() < RESUME_DELAY);
        if r.paused() != want_paused {
            r.set_paused(want_paused);
            if let Some(rec) = &mut self.recording {
                if want_paused {
                    if let Some(since) = rec.running_since.take() {
                        rec.recorded += since.elapsed();
                    }
                } else if !rec.stopping {
                    rec.running_since = Some(Instant::now());
                }
            }
            self.dirty = true;
        }
    }

    fn check_recorder(&mut self) {
        let Some(r) = &mut self.recorder else { return };
        for event in r.events() {
            self.dirty = true;
            match event {
                recorder::Event::Started(path) => log::info!("recording to {}", path.display()),
                recorder::Event::Saved { kind: Kind::Recording, path, bytes, secs } => {
                    let length = self.recording.take().map_or(Duration::from_secs_f64(secs), |rec| rec.recorded());
                    self.note = Some((format!("Saved {} · {} · {:.1} MB", file_name(&path), view::clock(length), bytes as f64 / 1e6), true));
                }
                recorder::Event::Saved { kind: Kind::Clip, path, bytes, secs } => {
                    self.note = Some((format!("Clipped {} · {secs:.0} s · {:.1} MB", file_name(&path), bytes as f64 / 1e6), true));
                    self.toast = Some((format!("Clipped the last {secs:.0} s"), true));
                    self.buzz = true;
                }
                recorder::Event::ClipWhileRecording => {
                    self.toast = Some(("Recording, it's all in there".into(), true));
                }
                recorder::Event::Failed { kind, message } => {
                    let message = if message.contains("no video was recorded") {
                        "Nothing recorded, you never left this tab".to_string()
                    } else {
                        message
                    };
                    if kind == Some(Kind::Recording) {
                        self.recording = None;
                    }
                    if kind == Some(Kind::Clip) {
                        let why = if message.contains("nothing to clip yet") {
                            "Nothing to clip yet, give it a few seconds"
                        } else {
                            "Couldn't save that clip"
                        };
                        self.toast = Some((why.into(), false));
                    }
                    self.note = Some((message, false));
                }
            }
        }
        // Without clipping the recorder only lives for one recording.
        if self.recording.is_none() && r.replay.is_none() && !r.quitting() {
            r.quit();
        }
        let Some(exit) = r.exited() else { return };
        let r = self.recorder.take().expect("checked above");
        self.dirty = true;
        match exit {
            Ok(()) => {
                if let (Some((note, true)), Some(cost)) = (&mut self.note, r.cost()) {
                    if note.starts_with("Saved") && !note.contains("cpu") {
                        note.push_str(&format!(" · {cost}"));
                    }
                }
            }
            Err(msg) => {
                log::warn!("recorder stopped: {msg}");
                self.recording = None;
                self.note = Some((msg, false));
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }

    fn go(&mut self, screen: Screen) {
        // A pairing code is only good while it's on screen.
        if !matches!(screen, Screen::Settings(Section::Sync) | Screen::Onboarding(Step::Sync)) {
            self.sync.stop_pairing();
        }
        self.screen = screen;
        self.hover = None;
        self.dirty = true;
    }

    /// While the tab's on screen: keeps the paired devices up to date, once
    /// a second while pairing (so the countdown ticks and a new device shows
    /// up right away), every few seconds otherwise.
    fn check_sync(&mut self) {
        let pairing = self.sync.pairing.is_some();
        let every = if pairing { Duration::from_secs(1) } else { SYNC_POLL };
        if self.sync.checked.elapsed() < every {
            return;
        }
        self.sync.checked = Instant::now();
        let unlocked = crate::setup::recorder().is_ok_and(|r| crate::setup::unlocked(&r));
        let relocked = crate::setup::relocked();
        if unlocked != self.unlocked || relocked != self.relocked {
            self.unlocked = unlocked;
            self.relocked = relocked;
            self.dirty = true;
        }
        let available = pairing::available();
        let devices = pairing::devices();
        if let Some(new) = devices.iter().find(|d| !self.sync.devices.iter().any(|old| old.id == d.id)) {
            self.note = Some((format!("Paired {}", new.name), true));
            self.toast = Some((format!("Paired {}", new.name), true));
            // That's what the code was for, back to the list.
            self.sync.stop_pairing();
        }
        if pairing || available != self.sync.available || devices.len() != self.sync.devices.len() {
            self.dirty = true;
        }
        self.sync.available = available;
        self.sync.devices = devices;
        // Used up by something that didn't get paired, or run out: a fresh one.
        if self.sync.pairing.as_ref().is_some_and(|p| p.as_ref().is_ok_and(|p| !p.waiting())) {
            self.sync.pairing = Some(Pairing::begin().map_err(|e| format!("{e:#}")));
        }
    }

    fn needs_redraw(&mut self) -> bool {
        // The clip button comes back once it's cooled down.
        if self.clipped_at.is_some_and(|t| t.elapsed() >= CLIP_COOLDOWN) {
            self.clipped_at = None;
            self.dirty = true;
        }
        // The timer ticks once a second while recording.
        if let Some(rec) = &self.recording {
            let second = rec.recorded().as_secs();
            if second != self.shown_second {
                self.shown_second = second;
                self.dirty = true;
            }
        }
        self.dirty
    }

    fn model(&self) -> Model<'_> {
        let status = match &self.recording {
            None => Status::Idle,
            Some(rec) if rec.stopping => Status::Stopping,
            Some(rec) => Status::Recording { recorded: rec.recorded() },
        };
        let sync = SyncView {
            available: self.sync.available,
            devices: &self.sync.devices,
            pairing: self.sync.pairing.as_ref().map(|p| p.as_ref().map_err(|e| e.as_str())),
        };
        Model {
            settings: &self.settings,
            screen: self.screen,
            sync,
            clip_cooling: self.clipped_at.is_some(),
            unlocked: self.unlocked,
            status,
            hover: self.hover,
            note: self.note.as_ref().map(|(t, ok)| (t.as_str(), *ok)),
            uninstall_armed: self.uninstall_armed,
            relocked: self.relocked,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
