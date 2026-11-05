//! The dashboard tab: start and stop recordings, and clip, from inside the
//! headset.
//!
//! It stays out of the way: nothing gets drawn unless the tab is on screen
//! and something changed, and it mostly sleeps otherwise. Recording pauses
//! while this tab is showing, so the controls never end up in the video.

mod mock;
mod pairing;
mod paint;
mod recorder;
mod remote;
mod report;
mod settings;
mod storage;
mod text;
mod texture;
mod view;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;

use crate::input::Input;
use crate::openvr::{AppType, OpenVr};
use crate::overlay::{DashboardTab, Event};
use crate::toast::Toast;
use recorder::{Kind, Recorder};
use remote::Remote;
use report::{Outcome, Report, Summary};
use settings::Settings;
use text::Fonts;
use pairing::clients::{Transfer, Update};
use pairing::{Device, Pairing, Presence};
use view::{Action, Hit, Model, Phase, ReportView, Screen, Section, Status, Step, SyncView};

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
/// How often to look for settings a phone changed.
const SETTINGS_EVERY: Duration = Duration::from_secs(1);
const SERVER_POLL: Duration = Duration::from_secs(2);
/// Shortest time between two clips, however they're asked for.
const CLIP_COOLDOWN: Duration = Duration::from_secs(1);
/// How often the paired devices get looked up while the tab's on screen.
const SYNC_POLL: Duration = Duration::from_secs(3);
/// And while something's moving: a download, an update, a code that's out.
const SYNC_POLL_BUSY: Duration = Duration::from_secs(1);

static QUIT: AtomicBool = AtomicBool::new(false);
/// Set when an update replaced this program, to restart into it.
static UPDATED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, Ordering::SeqCst);
}

/// Set when another launch asked to see the tab (SIGUSR1).
static SHOW: AtomicBool = AtomicBool::new(false);

extern "C" fn on_show(_: libc::c_int) {
    SHOW.store(true, Ordering::SeqCst);
}

/// Makes this the one framecorder tab. If one's running already, asks it to
/// open its tab and says so, without this one ever talking to SteamVR: SteamVR
/// counts every copy as the same app, and one leaving takes the other's tab
/// down with it. The lock goes when the process does.
fn only_one() -> Result<Option<std::fs::File>> {
    use std::io::{Read, Seek, Write};
    use std::os::fd::AsRawFd;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join("framecorder-ui.lock");
    let mut file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let mut pid = String::new();
        let _ = file.read_to_string(&mut pid);
        if let Ok(pid) = pid.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGUSR1) };
        }
        return Ok(None);
    }
    file.set_len(0)?;
    file.rewind()?;
    write!(file, "{}", std::process::id())?;
    Ok(Some(file))
}

/// What the tab knows about syncing, looked up again every so often while
/// it's on screen.
struct Sync {
    available: bool,
    /// Why the service couldn't start, when it said.
    error: Option<String>,
    /// Whether the tab has tried starting the service itself.
    started: bool,
    devices: Vec<Device>,
    /// An update of framecorder an app or the timer started.
    update: Update,
    /// Which page of the devices list the sync section is on.
    page: usize,
    /// The code on show while pairing.
    pairing: Option<Result<Pairing, String>>,
    checked: Instant,
}

impl Sync {
    fn look() -> Self {
        let paired = pairing::paired();
        Self {
            available: pairing::available(),
            error: pairing::start_error(),
            started: false,
            devices: paired.devices,
            update: paired.update,
            page: 0,
            pairing: None,
            checked: Instant::now(),
        }
    }

    /// Whether something's changing by the second: worth looking more often.
    fn busy(&self) -> bool {
        self.pairing.is_some() || self.update == Update::Updating || self.devices.iter().any(|d| d.transfer.is_some())
    }

    /// Keeps the page on the list after a device went.
    fn clamp_page(&mut self) {
        let pages = self.devices.len().div_ceil(view::DEVICES_PER_PAGE).max(1);
        self.page = self.page.min(pages - 1);
    }

    fn stop_pairing(&mut self) {
        if self.pairing.take().is_some() {
            pairing::end();
        }
    }
}

/// Something for SteamVR's keyboard, done on the next tick by whoever
/// holds the tab.
enum Keyboard {
    Show,
    Hide,
}

/// A recording in progress, as far as the tab is concerned.
struct Recording {
    /// Recorded time, not counting pauses.
    recorded: Duration,
    running_since: Option<Instant>,
    stopping: bool,
}

impl Recording {
    fn new(paused: bool) -> Self {
        Self { recorded: Duration::ZERO, running_since: (!paused).then(Instant::now), stopping: false }
    }

    fn recorded(&self) -> Duration {
        self.recorded + self.running_since.map_or(Duration::ZERO, |s| s.elapsed())
    }
}

struct App {
    settings: Settings,
    /// What phones ask for, through framecorder-sync.
    remote: Remote,
    live_viewers: usize,
    /// When ui.conf last changed as far as we know, and when we last looked.
    settings_at: Option<SystemTime>,
    settings_checked: Instant,
    recorder: Option<Recorder>,
    recording: Option<Recording>,
    note: Option<(String, bool)>,
    pointer: Option<(f32, f32)>,
    /// Which hit the pointer's over, to redraw only when that changes.
    over: Option<usize>,
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
    /// An update took the panels' permission away.
    relocked: bool,
    /// Bytes free for recordings, measured while the home screen's up.
    free: Option<u64>,
    /// An issue being reported, or the last one sent.
    report: Report,
    keyboard: Option<Keyboard>,
    /// Whether the tab starts with SteamVR, looked up when settings open.
    autostart: bool,
}

/// Draws the tab into a raw RGBA file instead of showing it, for checking
/// the look without a headset on. `state` is idle, recording, saved or toast,
/// a settings section, or a step of the setup (setup-shape and so on).
/// For what sync shows: live and sync-live (a download, a device away, one
/// on the old app), updating, update-failed, low-space, long-name, many and
/// many-2 (the second page), each with -home for the home screen's card,
/// and unpaired, no-sync and sync-error for the home screen with nothing
/// paired.
/// watching and watching-2 are the home screen with apps on the live view,
/// video-both the video section with both eyes blended, and reporting an
/// issue is report, report-gathering, report-typing, report-written,
/// report-sending, report-sent and report-failed.
pub use mock::serve as demo;

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
        "recording" | "locked" | "watching-2" => Status::Recording { recorded: Duration::from_secs(754) },
        _ => Status::Idle,
    };
    let screen = match state {
        "video" | "locked" | "video-both" => Screen::Settings(Section::Video),
        "audio" => Screen::Settings(Section::Audio),
        "clips" => Screen::Settings(Section::Clips),
        "general" | "general-off" => Screen::Settings(Section::General),
        "sync" | "pair" | "sync-live" | "long-name" | "many" | "many-2" => Screen::Settings(Section::Sync),
        "setup-shape" | "setup-locked-out" | "setup-both-eyes" => Screen::Onboarding(Step::Shape),
        "setup-quality" => Screen::Onboarding(Step::Quality),
        "setup-audio" => Screen::Onboarding(Step::Audio),
        "setup-clips" | "setup-no-sync" => Screen::Onboarding(Step::Clips),
        "setup-sync" | "setup-pair" => Screen::Onboarding(Step::Sync),
        s if s.starts_with("report") => Screen::Report,
        _ => Screen::Home,
    };
    let settings = match state {
        "setup-both-eyes" => Settings { shape: settings::Shape::BothEyes, ..settings },
        "video-both" => Settings { eye: settings::Eye::Both, ..settings },
        _ => settings,
    };
    let sample = Pairing::sample()?;
    // "many-home" and the like: the same devices, on the home screen.
    let devices = sample_devices(state.trim_end_matches("-home"));
    let pairing = matches!(state, "pair" | "setup-pair").then_some(Ok(&sample));
    let update = match state {
        "updating" => Update::Updating,
        "update-failed" => Update::Failed(Some("couldn't download the update, the site isn't reachable".into())),
        _ => Update::Idle,
    };
    let page = usize::from(state == "many-2");
    let error = (state == "sync-error").then_some("making the videos folder: Permission denied (os error 13)");
    let available = !matches!(state, "setup-no-sync" | "no-sync" | "sync-error");
    let sync = SyncView { available, error, devices: &devices, page, pairing, update: &update };
    let note = (state == "saved").then_some(("Saved 2026-09-23_01-43-02.mp4 · 00:12:34 · 3771.2 MB", true));
    let hover_on = match state {
        "idle" => Some(Action::Record),
        "report-written" => Some(Action::Send),
        _ => None,
    };
    let unlocked = !matches!(state, "locked-out" | "setup-locked-out" | "relocked");
    let free = Some(if state == "low-space" { 1_400_000_000 } else { 128_400_000_000 });
    let live_viewers = match state {
        "watching" => 1,
        "watching-2" => 2,
        _ => 0,
    };
    let summary = sample_summary();
    let report = sample_report(state, &summary);
    let model = Model {
        settings: &settings,
        screen,
        sync,
        live_viewers,
        clip_cooling: false,
        unlocked,
        status,
        pointer: None,
        note,
        relocked: state == "relocked",
        free,
        report,
        autostart: state != "general-off",
    };
    let hits = view::draw(&mut canvas, &mut fonts, &model);
    // a hover state: the pointer in the middle of that control, drawn again
    if let Some(h) = hover_on.and_then(|a| hits.iter().find(|h| h.action == a)) {
        let model = Model { pointer: Some((h.rect.x + h.rect.w / 2.0, h.rect.y + h.rect.h / 2.0)), ..model };
        view::draw(&mut canvas, &mut fonts, &model);
    }
    std::fs::write(path, &canvas.pixels)?;
    Ok(())
}

/// What a report's summary looks like on a healthy headset.
fn sample_summary() -> Summary {
    Summary {
        framecorder: "0.2.0 · panels unlocked".into(),
        os: "SteamOS 3.7.8 · build 20251001.1".into(),
        steamvr: "2.12.5 · public".into(),
        errors: 12,
        logs: ["recorder", "framecorder-ui", "framecorder-sync", "framecorder-update", "vrserver", "vrcompositor"].map(String::from).to_vec(),
    }
}

/// The report screen at each point along the way.
fn sample_report<'a>(state: &str, summary: &'a Summary) -> ReportView<'a> {
    const WRITTEN: &str = "Clips from the last hour came out tilted to the left, maybe 10 degrees, after the update this morning. Recordings I start by hand are fine.\n\nIt's the same in every game, I tried Beat Saber and the home space.";
    let (text, typing, phase) = match state {
        "report-typing" => ("Clips from the last hour came out tilted", true, Phase::Writing),
        "report-written" | "report-sending" | "report-failed" => (WRITTEN, false, Phase::Writing),
        _ => ("", false, Phase::Writing),
    };
    let phase = match state {
        "report-sending" => Phase::Sending,
        "report-sent" => Phase::Sent { number: 42, url: "https://github.com/coah80/framecorder/issues/42" },
        "report-failed" => Phase::Failed("couldn't reach the issue service, check your connection and try again"),
        _ => phase,
    };
    ReportView { text, typing, summary: (state != "report-gathering").then_some(summary), phase }
}

/// Made up paired devices for the previews: a couple that are just there,
/// or for the live states, one downloading, one away and one on the old app.
fn sample_devices(state: &str) -> Vec<Device> {
    let live = |id: &str, name: &str, presence, old_app, transfer| Device { presence, old_app, transfer, ..Device::paired(id, name) };
    let getting = Some(Transfer { name: "2026-10-15_12-27-10.mp4".into(), done: 19_200_000, total: 48_000_000 });
    match state {
        "live" | "sync-live" | "updating" | "update-failed" | "low-space" => vec![
            live("a", "Pixel 9", Presence::Connected, false, getting),
            live("b", "COLEPCWIN", Presence::Away, false, None),
            live("c", "Sam's MacBook Pro", Presence::Connected, true, None),
        ],
        "long-name" => vec![
            live("a", &"W".repeat(64), Presence::Connected, false, None),
            live("b", "Sam's work laptop with the long hostname nobody ever shortened", Presence::Away, false, getting),
        ],
        "many" | "many-2" => ["Pixel 9", "COLEPCWIN", "Sam's MacBook Pro", "Living room PC", "Galaxy S25", "Work laptop"]
            .iter()
            .enumerate()
            .map(|(i, name)| live(&i.to_string(), name, if i % 2 == 0 { Presence::Connected } else { Presence::Away }, false, None))
            .collect(),
        "unpaired" | "no-sync" | "sync-error" | "setup-no-sync" => Vec::new(),
        _ => vec![Device::paired("a", "Pixel 9"), Device::paired("b", "COLEPCWIN")],
    }
}

pub fn run() -> Result<()> {
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
        libc::signal(libc::SIGUSR1, on_show as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    // started again (from the library, say) while it's running: that's
    // asking to see it
    let Some(_lock) = only_one()? else {
        log::info!("framecorder's running already, opening its tab");
        return Ok(());
    };
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
    match crate::apps::register(&vr) {
        Ok(()) => log::info!("registered with SteamVR as {}", crate::apps::KEY),
        Err(e) => log::warn!("not registered with SteamVR: {e:#}"),
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
    let mut app = App::new(settings, crate::setup::panels_unlocked(), crate::setup::relocked());
    log::info!("dashboard tab ready");

    while !QUIT.load(Ordering::Relaxed) {
        if SHOW.swap(false, Ordering::SeqCst) {
            tab.show(KEY);
        }
        while let Some(event) = tab.poll() {
            let handled = match event {
                Event::KeyboardChar | Event::KeyboardDone | Event::KeyboardClosed => app.typed(event, tab.keyboard_text()),
                event => app.handle(event),
            };
            if !handled {
                QUIT.store(true, Ordering::SeqCst);
            }
        }
        if let Some(wish) = app.keyboard.take() {
            match wish {
                Keyboard::Show => {
                    if let Err(e) = tab.show_keyboard("What went wrong?", report::MAX_CHARS as u32, &app.report.text) {
                        log::warn!("{e:#}");
                        app.report.typing = false;
                        app.note = Some((format!("Couldn't open SteamVR's keyboard: {e:#}"), false));
                        app.dirty = true;
                    }
                }
                Keyboard::Hide => tab.hide_keyboard(),
            }
        }
        if input.as_ref().is_some_and(|i| i.clip_pressed()) {
            app.clip("keybind");
        }
        // On the Frame the dashboard counts as open the whole time you're in
        // the home space, so our own tab being on screen is what matters.
        let tab_visible = tab.is_visible();
        app.check_settings();
        app.manage_recorder();
        app.auto_pause(tab_visible);
        app.check_recorder();
        app.check_remote();
        app.check_report();
        // An update swapped this program out. Restart into it once nothing's
        // being recorded and nobody's using the tab.
        if !tab_visible && app.recording.is_none() && replaced() {
            log::info!("framecorder was updated, restarting into the new version");
            UPDATED.store(true, Ordering::SeqCst);
            break;
        }
        if tab_visible {
            app.check_sync(true);
        } else if app.sync.pairing.as_ref().is_some_and(|p| p.is_ok()) {
            // A code that's out keeps working with the tab closed, since the
            // headset's usually off by the time someone types it on a phone.
            // It ends when it's used or runs out, and stays on screen till then.
            app.check_sync(false);
        } else {
            app.sync.stop_pairing();
            if matches!(app.screen, Screen::Settings(_) | Screen::Report) {
                // Next time the tab opens it's back on the record button. A
                // report on its way keeps going, and lands as a note.
                app.go(Screen::Home);
            }
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
        // Becomes the new version in place, however it was started (systemd
        // or SteamVR, from the library).
        use std::os::unix::process::CommandExt;
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        let err = Command::new(home.join(".local/bin/framecorder-ui")).args(std::env::args_os().skip(1)).exec();
        log::error!("couldn't start the new version: {err}");
        // systemd (Restart=on-failure) has a go at it then
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
    fn new(settings: Settings, unlocked: bool, relocked: bool) -> Self {
        Self {
            settings,
            remote: Remote::new(),
            live_viewers: 0,
            settings_at: Settings::changed_at(),
            settings_checked: Instant::now(),
            recorder: None,
            recording: None,
            note: None,
            pointer: None,
            over: None,
            hits: Vec::new(),
            dirty: true,
            shown_second: u64::MAX,
            tab_hidden_at: None,
            retry_at: None,
            toast: None,
            toast_until: None,
            buzz: false,
            // The first time round the setup comes before anything else.
            screen: if settings.onboarded { Screen::Home } else { Screen::Onboarding(Step::Shape) },
            sync: Sync::look(),
            clipped_at: None,
            unlocked,
            relocked,
            free: storage::free_for_recordings(),
            report: Report::new(),
            keyboard: None,
            autostart: true,
        }
    }

    /// What SteamVR's keyboard did, with what it holds.
    fn typed(&mut self, event: Event, text: String) -> bool {
        match event {
            Event::KeyboardChar => self.report.typed(text),
            Event::KeyboardDone => self.report.finished(text),
            Event::KeyboardClosed => self.report.closed(text),
            _ => {}
        }
        self.dirty = true;
        true
    }

    /// Returns false when SteamVR wants us gone.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::MouseMove { x, y } => {
                self.pointer = Some((x, y));
                let over = self.hits.iter().position(|h| h.rect.contains(x, y));
                if over != self.over {
                    self.over = over;
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
        let s = &mut self.settings;
        match action {
            Action::TurnOff => return self.turn_off(),
            Action::Report => return self.go(Screen::Report),
            Action::Type => return self.start_typing(),
            Action::Send => return self.report.send(),
            Action::Autostart => {
                let want = !self.autostart;
                if crate::setup::set_autostart(want) {
                    self.autostart = want;
                } else {
                    self.note = Some(("Couldn't change that, try again".into(), false));
                }
                return;
            }
            Action::Record => return self.toggle_recording(),
            Action::ClipNow => return self.clip("tab button"),
            Action::Open(section) => return self.go(Screen::Settings(section)),
            Action::Close => return self.go(Screen::Home),
            Action::Step(step) => return self.go(Screen::Onboarding(step)),
            Action::FinishSetup => return self.finish_setup(),
            Action::Pair => return self.sync.pairing = Some(Pairing::begin().map_err(|e| format!("{e:#}"))),
            Action::ClosePair => return self.sync.stop_pairing(),
            Action::DevicePage(page) => {
                self.sync.page = page;
                self.sync.clamp_page();
                return;
            }
            Action::RemoveDevice(i) => {
                if let Some(d) = self.sync.devices.get(i) {
                    match pairing::remove(&d.id) {
                        Ok(()) => self.note = Some((format!("Removed {}", d.name), true)),
                        Err(e) => self.note = Some((format!("Couldn't remove {}: {e:#}", d.name), false)),
                    }
                }
                self.sync.devices = pairing::paired().devices;
                self.sync.clamp_page();
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
        self.save_settings();
    }

    /// Brings up SteamVR's keyboard over the report, with the draft in it.
    fn start_typing(&mut self) {
        if matches!(self.report.phase(), report::Phase::Writing | report::Phase::Failed(_)) && !self.report.typing {
            self.report.typing = true;
            self.keyboard = Some(Keyboard::Show);
        }
    }

    /// Picks up what the report's threads finished: the details to show,
    /// or how the send went, which lands as a note (and a toast, in case
    /// the tab's been closed meanwhile).
    fn check_report(&mut self) {
        match self.report.poll() {
            None => {}
            Some(Outcome::Gathered) => self.dirty = true,
            Some(Outcome::Sent { number, url }) => {
                self.note = Some((format!("Sent as issue #{number} · {url}"), true));
                self.toast = Some((format!("Report sent, it's issue #{number}"), true));
                self.dirty = true;
            }
            Some(Outcome::Failed(why)) => {
                self.note = Some((format!("Couldn't send the report: {why}"), false));
                self.toast = Some(("Couldn't send the report".into(), false));
                self.dirty = true;
            }
        }
    }

    fn save_settings(&mut self) -> bool {
        let saved = self.settings.save();
        self.settings_at = Settings::changed_at();
        saved
    }

    /// Picks up settings a phone changed. Like ones picked here, they take
    /// effect from the next recording.
    fn check_settings(&mut self) {
        if self.settings_checked.elapsed() < SETTINGS_EVERY {
            return;
        }
        self.settings_checked = Instant::now();
        let at = Settings::changed_at();
        if at == self.settings_at {
            return;
        }
        self.reload_settings();
    }

    fn reload_settings(&mut self) {
        self.settings_checked = Instant::now();
        self.settings_at = Settings::changed_at();
        let theirs = Settings { onboarded: self.settings.onboarded, ..Settings::load() };
        if theirs != self.settings {
            log::info!("the recording settings changed from a phone");
            self.settings = theirs;
            self.retry_at = None;
            self.dirty = true;
        }
    }

    /// Stops the tab (and with it the recorder and the clip buffer) and sync,
    /// until SteamVR starts again. Stopping our own service ends this
    /// process, so systemd does it without waiting on us.
    fn turn_off(&mut self) {
        let stopped = Command::new("systemctl")
            .args(["--user", "--no-block", "stop", "framecorder-sync.service", "framecorder-ui.service"])
            .status();
        // A tab opened from the app bar runs outside the service, where
        // stopping the service ends nothing, so it quits itself too.
        if stopped.as_ref().is_ok_and(|s| s.success()) {
            QUIT.store(true, Ordering::SeqCst);
        }
        match stopped {
            Ok(s) if s.success() => {
                let back = if crate::setup::autostart() {
                    "Closing. framecorder starts again with SteamVR."
                } else {
                    "Closing. Open framecorder from the dashboard when you want it."
                };
                self.note = Some((back.into(), true));
            }
            Ok(s) => self.note = Some((format!("Couldn't close framecorder: systemctl {s}"), false)),
            Err(e) => self.note = Some((format!("Couldn't close framecorder: {e}"), false)),
        }
    }

    /// The setup's over, finished or skipped: on to the record button, and
    /// from here on the recorder may run.
    fn finish_setup(&mut self) {
        self.settings = Settings { onboarded: true, ..self.settings };
        if !self.save_settings() {
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
                self.recording = Some(Recording::new(r.paused()));
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
        let wanted = self.settings.clipping().is_some() || self.live_viewers > 0;
        if let Some(r) = &mut self.recorder {
            let stale = r.replay != self.settings.clipping() || r.args != self.settings.recorder_args(self.unlocked);
            if self.recording.is_none() && !r.quitting() && (stale || !wanted) {
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
                    self.recording = None;
                    let length = Duration::from_secs_f64(secs);
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
        r.set_live(self.live_viewers > 0);
        // Without clipping the recorder only lives for one recording.
        if self.recording.is_none() && self.live_viewers == 0 && r.replay.is_none() && !r.quitting() {
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

    /// Does what a phone asked for, and keeps phones posted on how things stand.
    fn check_remote(&mut self) {
        let viewers = remote::live_viewers();
        if viewers != self.live_viewers {
            self.live_viewers = viewers;
            self.dirty = true;
        }
        if let Some((command, from)) = self.remote.poll() {
            log::info!("{from} asked to {command:?}");
            let result = self.handle_remote(command, &from);
            self.remote.answer(result);
            self.dirty = true;
        }
        let rec = self.recording.as_ref().filter(|r| !r.stopping);
        let status = remote::Status {
            ready: self.settings.onboarded,
            live_viewers: self.live_viewers,
            live_capture: self.live_viewers > 0 && self.recorder.as_ref().is_some_and(|r| !r.quitting()),
            panels: self.unlocked,
            recording: rec.is_some(),
            running: rec.is_some_and(|r| r.running_since.is_some()),
            recorded_ms: rec.map_or(0, |r| r.recorded().as_millis() as u64),
            clips: self.settings.clipping(),
            clip_ready: self.recording.is_none() && self.recorder.as_ref().is_some_and(|r| r.replay.is_some() && !r.quitting()),
        };
        self.remote.publish(&status);
    }

    fn prepare_recording(&mut self) -> std::result::Result<(), &'static str> {
        self.reload_settings();
        if let Some(r) = &mut self.recorder {
            if r.replay != self.settings.clipping() || r.args != self.settings.recorder_args(self.unlocked) {
                r.quit();
            }
            if r.quitting() {
                return Err("applying your settings, try Record again in a moment");
            }
        }
        Ok(())
    }

    /// "ok", or why not, in words for the phone.
    fn handle_remote(&mut self, command: remote::Command, from: &str) -> String {
        if !self.settings.onboarded {
            return "finish setting up framecorder on the headset first".into();
        }
        match command {
            remote::Command::Live => {
                self.manage_recorder();
                let Some(r) = &mut self.recorder else {
                    return "couldn't start the recorder, try again in a moment".into();
                };
                if r.quitting() {
                    return "the recorder is restarting, try again in a moment".into();
                }
                r.set_live(false);
                r.set_live(self.live_viewers > 0);
                r.keyframe();
                "ok".into()
            }
            remote::Command::Record => {
                if self.recording.is_none() {
                    if let Err(why) = self.prepare_recording() {
                        return why.into();
                    }
                    self.toggle_recording();
                    if self.recording.is_none() {
                        return self.note.as_ref().map_or("couldn't start recording".into(), |(n, _)| n.to_lowercase());
                    }
                    self.toast = Some((format!("Recording, started from {from}"), true));
                }
                "ok".into()
            }
            remote::Command::Stop => {
                if self.recording.as_ref().is_some_and(|r| !r.stopping) {
                    self.toggle_recording();
                }
                "ok".into()
            }
            remote::Command::Clip => {
                if self.recording.is_some() {
                    return "it's recording, so that's all in there already".into();
                }
                if self.clipped_at.is_some_and(|t| t.elapsed() < CLIP_COOLDOWN) {
                    return "just saved one".into();
                }
                if !self.recorder.as_ref().is_some_and(|r| r.replay.is_some() && !r.quitting()) {
                    let why = if self.settings.clipping().is_some() { "clips are still starting up" } else { "clips are off on the headset" };
                    return why.into();
                }
                self.clip(from);
                "ok".into()
            }
        }
    }

    fn go(&mut self, screen: Screen) {
        // Leaving the pairing screen takes the code back.
        if !matches!(screen, Screen::Settings(Section::Sync) | Screen::Onboarding(Step::Sync)) {
            self.sync.stop_pairing();
        }
        if screen == Screen::Settings(Section::General) {
            self.autostart = crate::setup::autostart();
        }
        if screen == Screen::Home {
            self.free = storage::free_for_recordings();
        }
        // Leaving the report takes the keyboard down, and once one's been
        // sent, the next starts fresh. Opening it starts gathering details.
        if self.screen == Screen::Report && screen != Screen::Report {
            if self.report.typing {
                self.report.typing = false;
                self.keyboard = Some(Keyboard::Hide);
            }
            if matches!(self.report.phase(), report::Phase::Sent { .. }) {
                self.report = Report::new();
            }
        }
        if screen == Screen::Report {
            self.note = None;
            self.report.open();
        }
        self.screen = screen;
        self.over = None;
        self.dirty = true;
    }

    /// Keeps the paired devices up to date, once a second while pairing (so
    /// the countdown ticks and a new device shows up right away) or while a
    /// download or an update is moving, every few seconds otherwise. Runs
    /// while the tab's on screen, or with a code out.
    fn check_sync(&mut self, visible: bool) {
        let pairing = self.sync.pairing.is_some();
        let every = if self.sync.busy() { SYNC_POLL_BUSY } else { SYNC_POLL };
        if self.sync.checked.elapsed() < every {
            return;
        }
        self.sync.checked = Instant::now();
        if visible && self.screen == Screen::Home {
            let free = storage::free_for_recordings();
            let shown = |f: Option<u64>| f.map(|b| view::free_space(b, self.settings.quality.mbps()));
            if shown(free) != shown(self.free) {
                self.dirty = true;
            }
            self.free = free;
        }
        let unlocked = crate::setup::panels_unlocked();
        let relocked = crate::setup::relocked();
        if unlocked != self.unlocked || relocked != self.relocked {
            self.unlocked = unlocked;
            self.relocked = relocked;
            self.dirty = true;
        }
        let available = pairing::available();
        // It's set up by the installer and every update, so missing means it
        // got stopped or never came up: start it rather than send anyone off
        // to reinstall. Once a run, in the background, so the tab doesn't wait.
        if !available && !self.sync.started {
            self.sync.started = true;
            std::thread::spawn(crate::setup::start_sync);
        }
        let error = if available { None } else { pairing::start_error() };
        if error != self.sync.error {
            self.sync.error = error;
            self.dirty = true;
        }
        let pairing::Paired { devices, update } = pairing::paired();
        if let Some(new) = devices.iter().find(|d| !self.sync.devices.iter().any(|old| old.id == d.id)) {
            self.note = Some((format!("Paired {}", new.name), true));
            self.toast = Some((format!("Paired {}", new.name), true));
            // That's what the code was for, back to the list.
            self.sync.stop_pairing();
        }
        // A download's progress or a device coming and going repaints too,
        // at most once a second, and only while someone's looking.
        if pairing || available != self.sync.available || devices != self.sync.devices || update != self.sync.update {
            self.dirty = true;
        }
        self.sync.available = available;
        self.sync.devices = devices;
        self.sync.update = update;
        self.sync.clamp_page();
        // Used up by something that didn't get paired, or run out: a fresh
        // one if someone's looking, otherwise that's the end of it.
        if self.sync.pairing.as_ref().is_some_and(|p| p.as_ref().is_ok_and(|p| !p.waiting())) {
            if visible {
                self.sync.pairing = Some(Pairing::begin().map_err(|e| format!("{e:#}")));
            } else {
                self.sync.stop_pairing();
            }
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
            error: self.sync.error.as_deref(),
            devices: &self.sync.devices,
            page: self.sync.page,
            pairing: self.sync.pairing.as_ref().map(|p| p.as_ref().map_err(|e| e.as_str())),
            update: &self.sync.update,
        };
        let phase = match self.report.phase() {
            report::Phase::Writing => Phase::Writing,
            report::Phase::Sending => Phase::Sending,
            report::Phase::Sent { number, url } => Phase::Sent { number: *number, url },
            report::Phase::Failed(why) => Phase::Failed(why),
        };
        let report = ReportView { text: &self.report.text, typing: self.report.typing, summary: self.report.summary.as_ref(), phase };
        Model {
            live_viewers: self.live_viewers,
            settings: &self.settings,
            screen: self.screen,
            sync,
            clip_cooling: self.clipped_at.is_some(),
            unlocked: self.unlocked,
            status,
            pointer: self.pointer,
            note: self.note.as_ref().map(|(t, ok)| (t.as_str(), *ok)),
            relocked: self.relocked,
            free: self.free,
            report,
            autostart: self.autostart,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
