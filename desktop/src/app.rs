//! the app's state and everything it can do. the screens that draw it are in
//! sidebar.rs, clips.rs, pair.rs and settings.rs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use framecorder_app_lib::core::discover::{self, Found};
use framecorder_app_lib::core::engine::{Forgot, Progress, State, Status};
use framecorder_app_lib::core::pairlink;
use framecorder_app_lib::core::store::Entry;
use gpui::{
    div, prelude::*, px, App, Context, FocusHandle, FontWeight, KeyDownEvent, ScrollHandle, SharedString,
    SystemNotification, SystemNotificationAction, SystemNotificationResponse, Task, Window,
};

use crate::remote::Live;
use crate::selection::Selection;
use crate::selfupdate::{self, Release};
use crate::sync::{self, Clip, Core, Msg};
use crate::theme::{self, c};
use crate::tray::Tray;
use crate::{autostart, check, clips, format, frame, pair, prefs, settings, sidebar, thumbs};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Clips,
    Frame,
    Settings,
    Pair,
    /// the connection check, reached from the frame page
    Check,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Clip,
    Recording,
}

pub enum Thumb {
    Pending,
    Ready(PathBuf),
    Missing,
}

pub enum DesktopUpdate {
    None,
    Available(Release),
    Downloading { pct: f32 },
    Ready(PathBuf),
}

pub struct Pairing {
    pub found: Vec<Found>,
    pub selected: Option<String>,
    pub code: String,
    pub error: Option<String>,
    pub busy: bool,
    pub focus: FocusHandle,
    /// a paired frame this is "pair again" for, so its clips carry over
    /// instead of coming down twice
    pub replaces: Option<String>,
    /// what's typed in the address box: a frame's ip, or its pairing link
    pub addr: String,
    pub addr_focus: FocusHandle,
    /// asking a typed-in address who's there
    pub finding: bool,
    tasks: Vec<Task<()>>,
}

/// how far through a run of downloads we are, for "getting 2 of 3"
pub struct Batch {
    pub total: usize,
    pub done: usize,
}

/// how fast the download is going, smoothed so it doesn't flicker
#[derive(Clone)]
pub struct Speed {
    pub id: String,
    /// when and how far along the last sample was
    at: Instant,
    done: u64,
    pub bytes_per_s: f64,
}

/// the least time between samples, so one slow chunk doesn't swing the rate
const SPEED_SAMPLE: Duration = Duration::from_millis(400);

impl Speed {
    /// the rate after a progress report, carrying the last one on when it's the same file
    pub fn after(last: Option<Speed>, id: &str, done: u64, now: Instant) -> Speed {
        match last {
            Some(s) if s.id == id => {
                let secs = now.saturating_duration_since(s.at).as_secs_f64();
                if secs < SPEED_SAMPLE.as_secs_f64() {
                    return s;
                }
                let rate = done.saturating_sub(s.done) as f64 / secs;
                let bytes_per_s = if s.bytes_per_s == 0. { rate } else { s.bytes_per_s * 0.6 + rate * 0.4 };
                Speed { at: now, done, bytes_per_s, ..s }
            }
            _ => Speed { id: id.to_string(), at: now, done, bytes_per_s: 0. },
        }
    }
}

pub struct FrameApp {
    pub core: Core,
    pub demo: bool,
    pub page: Page,
    pub filter: Filter,
    pub grid: bool,
    /// what's typed in the library's search box
    pub query: String,
    pub search: FocusHandle,
    /// the library itself holds the keyboard when nothing else does, for ctrl+f
    pub library: FocusHandle,
    /// the gallery's scrolling, so a drag can scroll it at the edges
    pub gallery: ScrollHandle,
    pub selection: Selection,
    pub statuses: Vec<Status>,
    /// what each frame has told us since it connected, by fingerprint
    pub live: HashMap<String, Live>,
    /// the frame the frame page is about, when there's more than one
    pub frame: Option<String>,
    /// the once-a-second repaint while a recording's clock is on screen
    pub tick: Option<Task<()>>,
    /// the frame "unpair" was clicked on, while settings asks if we mean it
    pub unpairing: Option<String>,
    pub confirm: FocusHandle,
    /// the connection check that's up, while the check page is
    pub check: Option<check::Check>,
    pub clips: Vec<Clip>,
    pub progress: Option<Progress>,
    pub batch: Option<Batch>,
    pub speed: Option<Speed>,
    pub toast: Option<SharedString>,
    pub pairing: Pairing,
    /// frames we asked to update, until they say they are
    pub frame_updating: HashSet<String>,
    pub autostart: Option<bool>,
    /// closing the window leaves it syncing from the tray
    pub background: bool,
    /// a system notification per clip that lands, with open and show in folder
    pub notify: bool,
    pub tray: Option<Tray>,
    told_about_tray: bool,
    pub desktop: DesktopUpdate,
    pub thumbs: HashMap<String, Thumb>,
    ffmpeg: bool,
    toast_task: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
}

impl FrameApp {
    pub fn new(core: Core, rx: async_channel::Receiver<Msg>, demo: bool, cx: &mut Context<Self>) -> Self {
        let mut tasks = Vec::new();
        tasks.push(cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                if this.update(cx, |app, cx| app.on_msg(msg, cx)).is_err() {
                    break;
                }
            }
        }));
        if !demo {
            tasks.push(cx.spawn(async move |this, cx| {
                let have = cx.background_executor().spawn(async { thumbs::have_ffmpeg() }).await;
                let _ = this.update(cx, |app, cx| {
                    app.ffmpeg = have;
                    app.request_thumbs(cx);
                });
            }));
            tasks.push(cx.spawn(async move |this, cx| loop {
                let found = cx.background_executor().spawn(async { selfupdate::check() }).await;
                match found {
                    Ok(Some(rel)) => {
                        let _ = this.update(cx, |app, cx| {
                            if matches!(app.desktop, DesktopUpdate::None) {
                                app.desktop = DesktopUpdate::Available(rel);
                                cx.notify();
                            }
                        });
                    }
                    Ok(None) => {}
                    Err(e) => log::info!("couldn't check for a desktop update: {e}"),
                }
                cx.background_executor().timer(Duration::from_secs(6 * 60 * 60)).await;
            }));
            // clips deleted or moved out of the folder leave the library on
            // their own. a look every couple of seconds is a stat per clip
            let engine = core.engine.clone();
            tasks.push(cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let engine = engine.clone();
                let clips = cx.background_executor().spawn(async move { sync::library(&engine) }).await;
                let alive = this.update(cx, |app, cx| {
                    if clips.len() != app.clips.len() || clips.iter().zip(&app.clips).any(|(a, b)| a.key != b.key) {
                        app.clips = clips;
                        app.request_thumbs(cx);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }));
        }

        let prefs = prefs::load(&core.state_dir);
        let mut app = Self {
            core,
            demo,
            page: Page::Clips,
            filter: Filter::All,
            grid: prefs.grid,
            query: String::new(),
            search: cx.focus_handle(),
            library: cx.focus_handle(),
            gallery: ScrollHandle::new(),
            selection: Selection::new(),
            statuses: Vec::new(),
            live: HashMap::new(),
            frame: None,
            tick: None,
            unpairing: None,
            confirm: cx.focus_handle(),
            check: None,
            clips: Vec::new(),
            progress: None,
            batch: None,
            speed: None,
            toast: None,
            pairing: Pairing {
                found: Vec::new(),
                selected: None,
                code: String::new(),
                error: None,
                busy: false,
                focus: cx.focus_handle(),
                replaces: None,
                addr: String::new(),
                addr_focus: cx.focus_handle(),
                finding: false,
                tasks: Vec::new(),
            },
            frame_updating: HashSet::new(),
            autostart: if demo { Some(true) } else { autostart::is_enabled() },
            background: prefs.background,
            notify: prefs.notify,
            tray: None,
            told_about_tray: false,
            desktop: DesktopUpdate::None,
            thumbs: HashMap::new(),
            ffmpeg: false,
            toast_task: None,
            _tasks: tasks,
        };
        if !demo {
            app.refresh();
            if app.statuses.is_empty() {
                app.open_pair(None, cx);
            }
        }
        app
    }

    fn refresh(&mut self) {
        if self.demo {
            return;
        }
        self.statuses = self.core.engine.statuses();
        self.clips = sync::library(&self.core.engine);
    }

    fn on_msg(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Status => {
                self.statuses = self.core.engine.statuses();
                self.sync_tray();
                for s in &self.statuses {
                    if s.update.as_ref().is_some_and(|u| u.updating || !u.available) {
                        self.frame_updating.remove(&s.fingerprint);
                    }
                }
                self.follow_connections(cx);
            }
            Msg::Remote(fp, remote) => self.on_remote(&fp, remote, cx),
            Msg::Recording(fp, settings) => self.on_recording(&fp, settings, cx),
            Msg::About(fp, about) => self.on_about(&fp, about, cx),
            Msg::Progress(p) => {
                let switched = self.progress.as_ref().is_none_or(|old| old.id != p.id);
                match &mut self.batch {
                    None => self.batch = Some(Batch { total: p.queued + 1, done: 0 }),
                    Some(b) if switched => b.total = b.total.max(b.done + 1 + p.queued),
                    Some(_) => {}
                }
                self.speed = Some(Speed::after(self.speed.take(), &p.id, p.done, Instant::now()));
                self.progress = Some(p);
            }
            Msg::Synced(e) => {
                if let Some(b) = &mut self.batch {
                    b.done += 1;
                }
                if self.progress.as_ref().is_some_and(|p| p.id == e.id) {
                    self.progress = None;
                    self.speed = None;
                }
                if self.notify {
                    self.say_it_landed(&e, cx);
                }
                self.refresh();
                self.request_thumbs(cx);
            }
            Msg::Busy(busy) => {
                if !busy {
                    self.progress = None;
                    self.batch = None;
                    self.speed = None;
                }
            }
            Msg::Removed => self.refresh(),
        }
        cx.notify();
    }

    // thumbnails

    fn request_thumbs(&mut self, cx: &mut Context<Self>) {
        if !self.ffmpeg {
            return;
        }
        let dir = thumbs::dir();
        let jobs: Vec<_> = self
            .clips
            .iter()
            .filter(|c| !self.thumbs.contains_key(&c.key))
            .map(|c| (c.key.clone(), c.location.clone(), thumbs::path_for(&dir, &c.key)))
            .collect();
        if jobs.is_empty() {
            return;
        }
        for (key, _, _) in &jobs {
            self.thumbs.insert(key.clone(), Thumb::Pending);
        }
        cx.spawn(async move |this, cx| {
            for (key, src, dst) in jobs {
                let out = dst.clone();
                let ok = cx.background_executor().spawn(async move { thumbs::make(&src, &out) }).await;
                let alive = this.update(cx, |app, cx| {
                    app.thumbs.insert(key, if ok { Thumb::Ready(dst) } else { Thumb::Missing });
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    // notifications

    /// "your clip from cole's frame is here", with open and show in folder
    fn say_it_landed(&self, e: &Entry, cx: &mut Context<Self>) {
        let what = if e.kind == "clip" { "clip" } else { "recording" };
        let from = self.statuses.iter().find(|s| s.fingerprint == e.host).map(|s| s.name.as_str()).unwrap_or("your frame");
        let body: Vec<String> = e.duration_s.map(format::length).into_iter().chain([format::size(e.size)]).collect();
        let action = |id: &str, label: &str| SystemNotificationAction { id: id.into(), label: label.into() };
        cx.show_system_notification(SystemNotification {
            tag: e.key().into(),
            title: format!("your {what} from {from} is here").into(),
            body: body.join(" · ").into(),
            actions: vec![action("open", "open"), action("reveal", "show in folder")],
        });
    }

    /// a notification was clicked: its body or "open" plays the clip,
    /// "show in folder" shows it. the window may be closed, both work without it
    pub fn notification_response(&mut self, resp: SystemNotificationResponse, cx: &mut Context<Self>) {
        let Some(clip) = self.clips.iter().find(|c| c.key == resp.tag.as_ref()).cloned() else { return };
        match resp.action_id.as_deref() {
            Some("reveal") => self.reveal_clip(&clip, cx),
            _ => self.open_clip(&clip, cx),
        }
    }

    pub fn set_notify(&mut self, on: bool, cx: &mut Context<Self>) {
        self.notify = on;
        self.save_prefs(cx);
        cx.notify();
    }

    // little things

    pub fn toast(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(msg.into());
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |app, cx| {
                app.toast = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == Page::Pair {
            self.open_pair(None, cx);
        } else {
            self.page = page;
            self.pairing.tasks.clear();
        }
        if page != Page::Check {
            self.check = None;
        }
        cx.notify();
    }

    // gpui hands these to the system off the ui thread. windows' ShellExecute
    // called straight from a click handler never opened anything.
    pub fn open_clip(&mut self, clip: &Clip, cx: &mut Context<Self>) {
        if clip.location.exists() {
            log::info!("opening {}", clip.location.display());
            cx.open_with_system(&clip.location);
        } else {
            self.toast("that file's been moved or deleted", cx);
        }
    }

    pub fn reveal_clip(&mut self, clip: &Clip, cx: &mut Context<Self>) {
        if clip.location.exists() {
            cx.reveal_path(&clip.location);
        } else {
            self.toast("that file's been moved or deleted", cx);
        }
    }

    pub fn open_folder(&mut self, cx: &mut Context<Self>) {
        cx.open_with_system(&self.core.download_dir);
    }

    /// puts a clip in the trash (the recycle bin on windows), where it can
    /// come back from. the engine still remembers syncing it, so it never
    /// comes down again
    pub fn delete_clip(&mut self, clip: &Clip, cx: &mut Context<Self>) {
        let (key, path) = (clip.key.clone(), clip.location.clone());
        let bin = if cfg!(windows) { "the recycle bin" } else { "the trash" };
        if self.demo {
            self.clips = self.clips.iter().filter(|c| c.key != key).cloned().collect();
            self.toast(format!("moved it to {bin}"), cx);
            return;
        }
        if !path.exists() {
            self.toast("that file's been moved or deleted", cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { trash::delete(&path).map_err(|e| e.to_string()) }).await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(()) => {
                        app.clips = app.clips.iter().filter(|c| c.key != key).cloned().collect();
                        app.toast(format!("moved it to {bin}"), cx);
                    }
                    Err(e) => app.toast(format!("couldn't delete it: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn retry(&mut self, cx: &mut Context<Self>) {
        self.core.engine.retry_now();
        self.toast("trying again", cx);
    }

    // unpairing a frame

    /// "unpair" in settings asks first. the question takes the keyboard, so
    /// esc keeps the frame and enter lets it go
    pub fn ask_unpair(&mut self, fingerprint: String, window: &mut Window, cx: &mut Context<Self>) {
        self.unpairing = Some(fingerprint);
        self.confirm.focus(window, cx);
        cx.notify();
    }

    pub fn keep_frame(&mut self, cx: &mut Context<Self>) {
        self.unpairing = None;
        cx.notify();
    }

    pub fn confirm_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" => self.keep_frame(cx),
            "enter" => {
                if let Some(fp) = self.unpairing.take() {
                    self.unpair(&fp, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub fn unpair(&mut self, fingerprint: &str, cx: &mut Context<Self>) {
        self.unpairing = None;
        let name = self.statuses.iter().find(|s| s.fingerprint == fingerprint).map(|s| s.name.clone());
        if self.demo {
            self.statuses.retain(|s| s.fingerprint != fingerprint);
            self.forgot(fingerprint, name, Forgot::Both, cx);
            return;
        }
        let (engine, fp) = (self.core.engine.clone(), fingerprint.to_string());
        let job = self.core.rt.spawn(async move { engine.unpair(&fp).await });
        let fp = fingerprint.to_string();
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| match res {
                Ok(result) => app.forgot(&fp, name, result, cx),
                Err(e) => app.toast(format!("couldn't unpair it: {e}"), cx),
            });
        })
        .detach();
        cx.notify();
    }

    fn forgot(&mut self, fingerprint: &str, name: Option<String>, result: Forgot, cx: &mut Context<Self>) {
        self.refresh();
        self.live.remove(fingerprint);
        if self.frame.as_deref() == Some(fingerprint) {
            self.frame = None;
        }
        self.sync_tray();
        match result {
            Forgot::LocalOnly => self.toast(
                "forgotten here. the frame still lists this device until you remove it in framecorder's sync settings on the headset.",
                cx,
            ),
            Forgot::Both => {
                if let Some(name) = name {
                    self.toast(format!("unpaired {name}"), cx);
                }
            }
        }
        if self.statuses.is_empty() && !self.demo {
            self.open_pair(None, cx);
        }
        cx.notify();
    }

    fn save_prefs(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let p = prefs::Prefs { background: self.background, grid: self.grid, notify: self.notify };
        if let Err(e) = prefs::save(&self.core.state_dir, &p) {
            self.toast(format!("couldn't save that: {e}"), cx);
        }
    }

    // the library's search box

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.focus(window, cx);
        cx.notify();
    }

    /// what's searched or filtered changing lets the selection go, like the
    /// android app: what's picked is always on screen
    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        self.clear_selection(cx);
        cx.notify();
    }

    pub fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.clear_selection(cx);
        cx.notify();
    }

    /// keys that reach the library page itself: ctrl+f (cmd+f on a mac) goes
    /// to the search box, ctrl+a picks every clip on screen, esc answers the
    /// delete question, then lets the selection go, then clears the search
    pub fn library_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let shortcut = k.modifiers.control || k.modifiers.platform;
        if shortcut && k.key == "f" {
            self.focus_search(window, cx);
        } else if shortcut && k.key == "a" && !self.search.is_focused(window) {
            self.select_all(cx);
        } else if k.key == "enter" && self.selection.asking {
            self.delete_selected(cx);
        } else if k.key == "escape" && self.selection.asking {
            self.keep_selected(cx);
        } else if k.key == "escape" && self.selecting() {
            self.clear_selection(cx);
        } else if k.key == "escape" && !self.query.is_empty() {
            self.clear_search(cx);
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// typing in the search box. esc clears it, and leaves it once it's empty
    pub fn search_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let was = self.query.clone();
        let shortcut = k.modifiers.control || k.modifiers.platform;
        if shortcut && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.query.push_str(text.split_whitespace().collect::<Vec<_>>().join(" ").as_str());
            }
        } else if shortcut && k.key == "backspace" {
            self.query.clear();
        } else if k.key == "backspace" {
            self.query.pop();
        } else if k.key == "escape" && !self.query.is_empty() {
            self.query.clear();
        } else if k.key == "escape" || k.key == "enter" {
            self.library.focus(window, cx);
        } else if let Some(ch) = k.key_char.as_deref().filter(|_| !shortcut) {
            if ch.chars().all(|c| !c.is_control()) {
                self.query.push_str(ch);
            }
        } else {
            return;
        }
        if self.query != was {
            self.clear_selection(cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub fn set_grid(&mut self, grid: bool, cx: &mut Context<Self>) {
        self.grid = grid;
        self.save_prefs(cx);
        cx.notify();
    }

    /// whether closing the window keeps it syncing from the tray, or quits
    pub fn set_background(&mut self, on: bool, cx: &mut Context<Self>) {
        self.background = on;
        if let Some(t) = &self.tray {
            t.set_visible(on);
        }
        self.save_prefs(cx);
        cx.notify();
    }

    pub fn attach_tray(&mut self, tray: Tray) {
        tray.set_visible(self.background);
        self.tray = Some(tray);
        self.sync_tray();
    }

    /// whether the window closing should leave the app running. the first
    /// time, a notification says where it went
    pub fn keep_running_on_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.demo || !self.background || self.tray.is_none() {
            return false;
        }
        if !self.told_about_tray {
            self.told_about_tray = true;
            cx.show_system_notification(SystemNotification {
                tag: "framecorder-tray".into(),
                title: "framecorder is still syncing".into(),
                body: if cfg!(target_os = "macos") {
                    "it's in the menu bar, so new clips keep coming in. to close it for real, click it there and pick quit, or turn off \"keep running\" in settings."
                } else {
                    "it's in the tray, so new clips keep coming in. to close it for real, right-click it there and pick quit, or turn off \"keep running\" in settings."
                }
                .into(),
                actions: Vec::new(),
            });
        }
        true
    }

    pub fn set_autostart(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.demo {
            self.autostart = Some(on);
        } else {
            match autostart::set(on) {
                Ok(now) => self.autostart = Some(now),
                Err(e) => self.toast(format!("couldn't change that: {e}"), cx),
            }
        }
        cx.notify();
    }

    /// has a frame install its framecorder update
    pub fn update_frame(&mut self, fingerprint: String, cx: &mut Context<Self>) {
        self.frame_updating.insert(fingerprint.clone());
        cx.notify();
        if self.demo {
            return;
        }
        let (engine, fp) = (self.core.engine.clone(), fingerprint.clone());
        let job = self.core.rt.spawn(async move { engine.start_update(&fp).await });
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            if let Err(e) = res {
                let _ = this.update(cx, |app, cx| {
                    app.frame_updating.remove(&fingerprint);
                    app.toast(format!("the frame couldn't start its update: {e}"), cx);
                });
            }
        })
        .detach();
    }

    /// the desktop update pill: download, then restart into it
    pub fn desktop_update_clicked(&mut self, cx: &mut Context<Self>) {
        match std::mem::replace(&mut self.desktop, DesktopUpdate::None) {
            DesktopUpdate::Available(rel) => {
                let (url, size) = (rel.url.clone(), rel.size);
                self.download_desktop(rel, url, size, cx)
            }
            DesktopUpdate::Ready(path) => {
                if self.demo {
                    self.desktop = DesktopUpdate::None;
                    self.toast("this is where it'd restart", cx);
                    return;
                }
                let job = cx.background_executor().spawn(async move {
                    let result = selfupdate::apply_and_restart(&path);
                    (path, result)
                });
                cx.spawn(async move |this, cx| {
                    let (path, result) = job.await;
                    let _ = this.update(cx, |app, cx| match result {
                        Ok(()) => cx.quit(),
                        Err(e) => {
                            app.desktop = DesktopUpdate::Ready(path);
                            app.toast(e, cx);
                        }
                    });
                })
                .detach();
            }
            other => self.desktop = other,
        }
        cx.notify();
    }

    fn download_desktop(&mut self, rel: Release, url: String, size: u64, cx: &mut Context<Self>) {
        self.desktop = DesktopUpdate::Downloading { pct: 0.0 };
        let (tx, rx) = async_channel::unbounded::<f32>();
        let demo = self.demo;
        let job = cx.background_executor().spawn(async move {
            if demo {
                for i in 1..=40 {
                    std::thread::sleep(Duration::from_millis(60));
                    let _ = tx.try_send(i as f32 / 40.0);
                }
                return Ok(PathBuf::new());
            }
            selfupdate::download(&url, size, |p| {
                let _ = tx.try_send(p);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(pct) = rx.recv().await {
                if this
                    .update(cx, |app, cx| {
                        app.desktop = DesktopUpdate::Downloading { pct };
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let res = job.await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(path) => app.desktop = DesktopUpdate::Ready(path),
                    Err(e) => {
                        app.desktop = DesktopUpdate::Available(rel);
                        app.toast(format!("the update didn't download: {e}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // pairing

    /// "pair again" for a frame that forgot us or got a new certificate: the
    /// new pairing takes over its clips, so nothing comes down twice
    pub fn open_pair_for(&mut self, fingerprint: String, cx: &mut Context<Self>) {
        self.open_pair(Some(fingerprint), cx);
    }

    pub fn open_pair(&mut self, replaces: Option<String>, cx: &mut Context<Self>) {
        self.page = Page::Pair;
        let p = &mut self.pairing;
        p.code.clear();
        p.addr.clear();
        p.error = None;
        p.busy = false;
        p.finding = false;
        p.replaces = replaces;
        p.tasks.clear();
        if self.demo {
            return;
        }
        let (tx, rx) = async_channel::unbounded::<Found>();
        p.tasks.push(cx.spawn(async move |this, cx| {
            while let Ok(found) = rx.recv().await {
                if this.update(cx, |app, cx| app.found(found, cx)).is_err() {
                    break;
                }
            }
        }));
        // mdns and a knock on every nearby address (for routers and firewalls
        // that drop multicast), a few seconds at a time, for as long as this
        // screen's up
        let rt = self.core.rt.clone();
        let app = self.core.engine.app().to_string();
        p.tasks.push(cx.spawn(async move |_, cx| loop {
            let tx = tx.clone();
            let app = app.clone();
            let job = rt.spawn(async move {
                discover::look(Duration::from_secs(6), true, &app, move |f| tx.try_send(f).is_ok()).await;
            });
            let _ = job.await;
            cx.background_executor().timer(Duration::from_secs(1)).await;
        }));
    }

    fn found(&mut self, found: Found, cx: &mut Context<Self>) {
        let p = &mut self.pairing;
        match p.found.iter_mut().find(|f| f.fingerprint == found.fingerprint) {
            Some(f) => *f = found,
            None => p.found.push(found),
        }
        if p.selected.is_none() && p.found.len() == 1 {
            p.selected = Some(p.found[0].fingerprint.clone());
        }
        cx.notify();
    }

    pub fn code_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let p = &mut self.pairing;
        if p.busy {
            return;
        }
        if (k.modifiers.control || k.modifiers.platform) && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let digits: String = text.chars().filter(|c| c.is_ascii_digit()).take(6).collect();
                p.code = digits;
            }
        } else if k.key == "backspace" {
            p.code.pop();
        } else if k.key == "enter" {
            self.submit_pair(cx);
            return;
        } else if k.key.len() == 1 && k.key.chars().all(|c| c.is_ascii_digit()) && p.code.len() < 6 {
            p.code.push_str(&k.key);
        } else {
            return;
        }
        p.error = None;
        // the sixth digit pairs by itself, like the android app
        if self.can_pair() {
            self.submit_pair(cx);
        }
        cx.notify();
    }

    pub fn can_pair(&self) -> bool {
        let p = &self.pairing;
        !p.busy && p.selected.is_some() && pairlink::is_code(&p.code)
    }

    pub fn submit_pair(&mut self, cx: &mut Context<Self>) {
        let p = &mut self.pairing;
        let Some(found) = p.selected.as_ref().and_then(|fp| p.found.iter().find(|f| &f.fingerprint == fp)).cloned()
        else {
            p.error = Some("pick your frame first".into());
            cx.notify();
            return;
        };
        if !pairlink::is_code(&p.code) {
            p.error = Some("the code is the 6 digits shown on your frame".into());
            cx.notify();
            return;
        }
        let code = p.code.clone();
        self.pair_with(with_port(&found.addr), found.fingerprint, code, cx);
    }

    /// pairs with one frame, by address and fingerprint, then goes to the library
    fn pair_with(&mut self, addr: String, fingerprint: String, code: String, cx: &mut Context<Self>) {
        let p = &mut self.pairing;
        p.busy = true;
        p.error = None;
        cx.notify();
        let (engine, replaces) = (self.core.engine.clone(), p.replaces.clone());
        let job = self.core.rt.spawn(async move { engine.pair(&addr, Some(&fingerprint), &code, replaces.as_deref()).await });
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                app.pairing.busy = false;
                match res {
                    Ok(host) => {
                        if let Some(old) = app.pairing.replaces.take() {
                            app.live.remove(&old);
                        }
                        app.refresh();
                        app.go(Page::Clips, cx);
                        app.toast(format!("paired with {}", host.name), cx);
                    }
                    Err(e) => app.pairing.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// typing in the address box. enter asks who's there, esc clears it
    pub fn addr_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let p = &mut self.pairing;
        if p.busy || p.finding {
            return;
        }
        let shortcut = k.modifiers.control || k.modifiers.platform;
        if shortcut && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                p.addr.push_str(text.trim());
            }
        } else if shortcut && k.key == "backspace" {
            p.addr.clear();
        } else if k.key == "backspace" {
            p.addr.pop();
        } else if k.key == "enter" {
            self.submit_addr(window, cx);
            return;
        } else if k.key == "escape" && !p.addr.is_empty() {
            p.addr.clear();
        } else if k.key == "escape" {
            p.focus.focus(window, cx);
        } else if let Some(ch) = k.key_char.as_deref().filter(|_| !shortcut) {
            if ch.chars().all(|c| !c.is_control() && !c.is_whitespace()) {
                p.addr.push_str(ch);
            }
        } else {
            return;
        }
        p.error = None;
        cx.stop_propagation();
        cx.notify();
    }

    /// what's in the address box: a pairing link pairs right away, an
    /// address gets asked who's there, and that frame goes in the list, picked
    pub fn submit_addr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.pairing.addr.trim().to_string();
        if typed.is_empty() {
            return;
        }
        if typed.starts_with("framecorder://") {
            match pairlink::parse(&typed) {
                Ok(link) => self.pair_with(link.addr, link.fingerprint, link.code, cx),
                Err(e) => self.pairing.error = Some(e),
            }
            cx.notify();
            return;
        }
        let addr = with_port(&typed);
        self.pairing.finding = true;
        self.pairing.error = None;
        cx.notify();
        let shown = addr.clone();
        let app = self.core.engine.app().to_string();
        let job = self.core.rt.spawn(async move { discover::identify(&addr, &app).await });
        cx.spawn_in(window, async move |this, cx| {
            let found = job.await.ok().flatten();
            let _ = this.update_in(cx, |app, window, cx| {
                app.pairing.finding = false;
                match found {
                    Some(f) => {
                        app.pairing.addr.clear();
                        app.pairing.selected = Some(f.fingerprint.clone());
                        app.found(f, cx);
                        // the code comes next
                        app.pairing.focus.focus(window, cx);
                    }
                    None => app.pairing.error = Some(format!("nothing that looks like a frame answered at {shown}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// something wrong with a frame, worth a whole panel
    pub fn problems(&self) -> Vec<&Status> {
        self.statuses.iter().filter(|s| !matches!(s.state, State::Connected | State::Connecting)).collect()
    }
}

impl Render for FrameApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.page == Page::Pair {
            pair::render(self, window, cx).into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .child(sidebar::render(self, cx))
                .child(match self.page {
                    Page::Settings => settings::render(self, cx).into_any_element(),
                    Page::Frame => frame::render(self, cx).into_any_element(),
                    Page::Check => check::render(self, cx).into_any_element(),
                    _ => clips::render(self, window, cx).into_any_element(),
                })
                .into_any_element()
        };
        div()
            .size_full()
            .relative()
            .bg(c(theme::BASE))
            .text_color(c(theme::TEXT))
            .font_family(theme::BODY)
            .text_size(px(14.))
            .child(body)
            .when_some(self.toast.clone(), |d, msg| {
                d.child(
                    div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(
                        div()
                            .max_w(px(520.))
                            .px(px(16.))
                            .py(px(12.))
                            .rounded(px(12.))
                            .bg(c(theme::SURFACE0))
                            .border_1()
                            .border_color(c(theme::SURFACE1))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .shadow_lg()
                            .child(msg),
                    ),
                )
            })
    }
}

/// an address with framecorder-sync's port on it, unless one was typed
fn with_port(addr: &str) -> String {
    let addr = addr.trim();
    let has_port = addr.rsplit_once(':').is_some_and(|(host, port)| {
        port.parse::<u16>().is_ok() && (!host.contains(':') || host.ends_with(']'))
    });
    if has_port {
        addr.to_string()
    } else {
        pairlink::join_addr(addr.trim_matches(['[', ']']), discover::PORT)
    }
}

/// what a frame's status line says
pub fn status_text(s: &Status) -> &'static str {
    match s.state {
        State::Connected => "connected",
        State::Connecting => "looking for it...",
        State::Unreachable => "can't reach it",
        State::Full => "out of space here",
        State::Unpaired => "forgot this device",
        State::WrongFingerprint => "something else answered",
    }
}

pub fn status_color(s: &Status) -> u32 {
    match s.state {
        State::Connected => theme::GREEN,
        State::Connecting => theme::YELLOW,
        State::Unreachable | State::Full => theme::RED,
        State::Unpaired | State::WrongFingerprint => theme::PEACH,
    }
}

pub fn quit(cx: &mut App) {
    cx.quit();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_download_rate_settles_and_starts_over_per_file() {
        let t0 = Instant::now();
        let s = Speed::after(None, "a", 0, t0);
        assert_eq!(s.bytes_per_s, 0.);
        // too soon for a sample, nothing changes
        let s = Speed::after(Some(s), "a", 1_000_000, t0 + Duration::from_millis(100));
        assert_eq!(s.bytes_per_s, 0.);
        let s = Speed::after(Some(s), "a", 4_200_000, t0 + Duration::from_secs(1));
        assert_eq!(s.bytes_per_s, 4_200_000.);
        // then it eases towards the new rate
        let s = Speed::after(Some(s), "a", 6_200_000, t0 + Duration::from_secs(2));
        assert_eq!(s.bytes_per_s, 4_200_000. * 0.6 + 2_000_000. * 0.4);
        // the next file starts from nothing
        let s = Speed::after(Some(s), "b", 500, t0 + Duration::from_secs(3));
        assert_eq!(s.bytes_per_s, 0.);
    }

    #[test]
    fn a_typed_address_gets_the_port_it_needs() {
        assert_eq!(with_port("192.168.1.42"), "192.168.1.42:38619");
        assert_eq!(with_port(" 192.168.1.42:4000 "), "192.168.1.42:4000");
        assert_eq!(with_port("fe80::1"), "[fe80::1]:38619");
        assert_eq!(with_port("[fe80::1]:4000"), "[fe80::1]:4000");
    }
}
