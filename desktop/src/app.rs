//! the app's state and everything it can do. the screens that draw it are in
//! sidebar.rs, clips.rs, pair.rs and settings.rs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use framecorder_app_lib::core::discover::{self, Found};
use framecorder_app_lib::core::engine::{Progress, State, Status};
use framecorder_app_lib::core::pairlink;
use gpui::{
    div, prelude::*, px, App, Context, FocusHandle, FontWeight, KeyDownEvent, SharedString, SystemNotification, Task,
    Window,
};

use crate::remote::Live;
use crate::selfupdate::{self, Release};
use crate::sync::{self, Clip, Core, Msg};
use crate::theme::{self, c};
use crate::tray::Tray;
use crate::{autostart, clips, frame, pair, prefs, settings, sidebar, thumbs};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Clips,
    Frame,
    Settings,
    Pair,
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
    tasks: Vec<Task<()>>,
}

/// how far through a run of downloads we are, for "getting 2 of 3"
pub struct Batch {
    pub total: usize,
    pub done: usize,
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
    pub clips: Vec<Clip>,
    pub progress: Option<Progress>,
    pub batch: Option<Batch>,
    pub toast: Option<SharedString>,
    pub pairing: Pairing,
    /// frames we asked to update, until they say they are
    pub frame_updating: HashSet<String>,
    pub autostart: Option<bool>,
    /// closing the window leaves it syncing from the tray
    pub background: bool,
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
            statuses: Vec::new(),
            live: HashMap::new(),
            frame: None,
            tick: None,
            unpairing: None,
            confirm: cx.focus_handle(),
            clips: Vec::new(),
            progress: None,
            batch: None,
            toast: None,
            pairing: Pairing {
                found: Vec::new(),
                selected: None,
                code: String::new(),
                error: None,
                busy: false,
                focus: cx.focus_handle(),
                replaces: None,
                tasks: Vec::new(),
            },
            frame_updating: HashSet::new(),
            autostart: if demo { Some(true) } else { autostart::is_enabled() },
            background: prefs.background,
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
            Msg::About(fp, about) => self.on_about(&fp, about, cx),
            Msg::Progress(p) => {
                let switched = self.progress.as_ref().is_none_or(|old| old.id != p.id);
                match &mut self.batch {
                    None => self.batch = Some(Batch { total: p.queued + 1, done: 0 }),
                    Some(b) if switched => b.total = b.total.max(b.done + 1 + p.queued),
                    Some(_) => {}
                }
                self.progress = Some(p);
            }
            Msg::Synced(e) => {
                if let Some(b) = &mut self.batch {
                    b.done += 1;
                }
                if self.progress.as_ref().is_some_and(|p| p.id == e.id) {
                    self.progress = None;
                }
                let what = if e.kind == "clip" { "new clip" } else { "new recording" };
                cx.show_system_notification(SystemNotification {
                    tag: e.key().into(),
                    title: format!("{what} from your frame").into(),
                    body: e.name.clone().into(),
                    actions: Vec::new(),
                });
                self.refresh();
                self.request_thumbs(cx);
            }
            Msg::Busy(busy) => {
                if !busy {
                    self.progress = None;
                    self.batch = None;
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
        let done = if self.demo {
            self.statuses.retain(|s| s.fingerprint != fingerprint);
            true
        } else {
            match self.core.engine.unpair(fingerprint) {
                Ok(()) => true,
                Err(e) => {
                    self.toast(format!("couldn't unpair it: {e}"), cx);
                    false
                }
            }
        };
        self.refresh();
        if done {
            self.live.remove(fingerprint);
            if self.frame.as_deref() == Some(fingerprint) {
                self.frame = None;
            }
            self.sync_tray();
        }
        if let Some(name) = name.filter(|_| done) {
            self.toast(format!("unpaired {name}"), cx);
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
        let p = prefs::Prefs { background: self.background, grid: self.grid };
        if let Err(e) = prefs::save(&self.core.state_dir, &p) {
            self.toast(format!("couldn't save that: {e}"), cx);
        }
    }

    // the library's search box

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.focus(window, cx);
        cx.notify();
    }

    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        cx.notify();
    }

    /// keys that reach the library page itself: ctrl+f (cmd+f on a mac) goes
    /// to the search box, esc clears what was searched
    pub fn library_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if (k.modifiers.control || k.modifiers.platform) && k.key == "f" {
            self.focus_search(window, cx);
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
            DesktopUpdate::Available(rel) => match rel.asset.clone() {
                None => {
                    cx.open_url(&rel.page);
                    self.desktop = DesktopUpdate::Available(rel);
                }
                Some((url, size)) => self.download_desktop(rel, url, size, cx),
            },
            DesktopUpdate::Ready(path) => {
                if self.demo {
                    self.desktop = DesktopUpdate::None;
                    self.toast("this is where it'd restart", cx);
                    return;
                }
                match selfupdate::apply_and_restart(&path) {
                    Ok(()) => cx.quit(),
                    Err(e) => {
                        self.desktop = DesktopUpdate::Ready(path);
                        self.toast(e, cx);
                    }
                }
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
        p.error = None;
        p.busy = false;
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
        // mdns, a few seconds at a time, for as long as this screen's up
        let rt = self.core.rt.clone();
        p.tasks.push(cx.spawn(async move |_, cx| loop {
            let tx = tx.clone();
            let job = rt.spawn(async move {
                let _ = discover::browse(Duration::from_secs(6), move |f| {
                    let _ = tx.try_send(f);
                    true
                })
                .await;
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
        p.busy = true;
        p.error = None;
        cx.notify();
        let (engine, code, replaces) = (self.core.engine.clone(), p.code.clone(), p.replaces.clone());
        let addr = if found.addr.contains(':') && !found.addr.ends_with(']') {
            found.addr.clone()
        } else {
            format!("{}:38619", found.addr)
        };
        let job = self.core.rt.spawn(async move {
            engine.pair(&addr, Some(&found.fingerprint), &code, replaces.as_deref()).await
        });
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
