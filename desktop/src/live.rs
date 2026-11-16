//! the live page: what the headset shows, as it shows it. the app core pulls
//! the stream from the frame once and serves it on this computer, so obs (or
//! any player) can record it, and a small picture of it sits on the page.

use std::sync::Arc;
use std::time::{Duration, Instant};

use framecorder_app_lib::core::engine::{State, Status};
use framecorder_app_lib::core::live::{LivePhase, LiveStatus};
use gpui::{
    div, img, prelude::*, px, AnyElement, ClickEvent, ClipboardItem, Context, Div, FocusHandle, FontWeight, KeyDownEvent,
    ObjectFit, RenderImage, SharedString, Task, Window,
};

use crate::app::{narrow, FrameApp, Page};
use crate::preview::{self, Decoder, Note};
use crate::sound::Sound;
use crate::theme::{self, a, c};
use crate::widgets::{card, danger, data, dot, icon, label, outline, primary, spinner, title};
use crate::{format, frame};

pub struct View {
    /// the frame it's streaming from, while it is
    pub frame: Option<String>,
    /// asked the core to start, not heard back yet
    pub starting: bool,
    /// the demo's made up status. the real one is read off the engine
    pub demo_status: Option<LiveStatus>,
    pub picture: Option<Arc<RenderImage>>,
    pub note: Note,
    decoder: Option<Decoder>,
    /// which decoder the pictures coming in are from, so an old one's
    /// leftovers don't land after a switch
    generation: u64,
    tasks: Vec<Task<()>>,
    /// the picture fills the screen, with sound, for casting it
    pub full: bool,
    /// the window has gone fullscreen since, so leaving it means leaving this
    seen_full: bool,
    /// the mouse last moved, the exit button shows for a moment after
    moved: Instant,
    sound: Option<Sound>,
    /// the sound couldn't start, said once and not tried again until next time
    no_sound: bool,
    focus: FocusHandle,
}

impl View {
    pub fn new(focus: FocusHandle) -> Self {
        Self {
            frame: None,
            starting: false,
            demo_status: None,
            picture: None,
            note: Note::Waiting,
            decoder: None,
            generation: 0,
            tasks: Vec::new(),
            full: false,
            seen_full: false,
            moved: Instant::now(),
            sound: None,
            no_sound: false,
            focus,
        }
    }
}

/// how long the exit button stays after the mouse stops
const CONTROLS_FOR: Duration = Duration::from_secs(3);

/// the three obs steps, in order
const OBS_STEPS: [&str; 3] = [
    "in obs, under sources, press + and pick media source",
    "untick \"local file\"",
    "paste the address above into \"input\", then ok",
];

impl FrameApp {
    pub fn live_status(&self) -> Option<LiveStatus> {
        if self.demo {
            self.live_view.demo_status.clone()
        } else {
            self.core.engine.live_status()
        }
    }

    /// streaming, or about to be
    pub fn live_on(&self) -> bool {
        self.live_view.starting
            || (self.live_view.frame.is_some()
                && self.live_status().is_some_and(|s| matches!(s.phase, LivePhase::Connecting | LivePhase::Live)))
    }

    pub fn start_live(&mut self, fingerprint: String, cx: &mut Context<Self>) {
        if self.live_view.starting {
            return;
        }
        self.live_view.starting = true;
        self.live_view.note = Note::Waiting;
        cx.notify();
        if self.demo {
            self.live_view.starting = false;
            self.live_view.frame = Some(fingerprint);
            self.live_view.demo_status = Some(crate::demo::live_status(LivePhase::Connecting));
            self.live_view.tasks.push(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let _ = this.update(cx, |app, cx| {
                    if app.live_view.frame.is_some() {
                        app.live_view.demo_status = Some(crate::demo::live_status(LivePhase::Live));
                        app.live_view.picture = Some(preview::demo_frame());
                        app.live_view.note = Note::Showing;
                    }
                    cx.notify();
                });
            }));
            return;
        }
        let (engine, fp) = (self.core.engine.clone(), fingerprint.clone());
        let job = self.core.rt.spawn(async move { engine.start_live(&fp).await });
        self.live_view.tasks.push(cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                app.live_view.starting = false;
                match res {
                    Ok(_) => app.live_view.frame = Some(fingerprint),
                    Err(e) => app.toast(format!("couldn't start live view: {e}"), cx),
                }
                cx.notify();
            });
        }));
    }

    pub fn stop_live(&mut self, cx: &mut Context<Self>) {
        self.stop_preview(cx);
        let was_on = self.live_view.frame.take().is_some() || self.live_view.starting;
        self.live_view.starting = false;
        self.live_view.tasks.clear();
        if self.demo {
            if let Some(s) = &mut self.live_view.demo_status {
                s.phase = LivePhase::Stopped;
                s.bytes_per_second = 0;
                s.viewers = 0;
            }
        } else if was_on {
            let engine = self.core.engine.clone();
            self.core.rt.spawn(async move { engine.stop_live().await });
        }
        cx.notify();
    }

    /// stops everything on the way out, so nothing streams with no window to see it in
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        self.stop_live(cx);
        cx.quit();
    }

    /// keeps the picture in step with the stream: decoding while it's live and
    /// the page is up, not otherwise. the page calls this as it draws
    fn sync_preview(&mut self, window: &Window, cx: &mut Context<Self>) {
        let live = self.live_status().filter(|s| s.phase == LivePhase::Live && self.live_view.frame.is_some());
        let Some(status) = live else {
            if self.live_view.decoder.is_some() || self.live_view.picture.is_some() || self.live_view.sound.is_some() {
                self.stop_preview(cx);
            }
            return;
        };
        self.sync_sound(&status.url, cx);
        let size = if self.live_view.full { full_size(window, cx) } else { preview::SMALL };
        if self.live_view.decoder.as_ref().is_some_and(|d| d.size != size) {
            self.live_view.decoder = None;
            self.live_view.note = Note::Waiting;
        }
        if self.demo || !self.ffmpeg || self.live_view.decoder.is_some() || self.live_view.note == Note::Gone {
            return;
        }
        self.live_view.generation += 1;
        let generation = self.live_view.generation;
        let (tx, rx) = async_channel::bounded::<Arc<RenderImage>>(1);
        match Decoder::start(&status.url, size, move |frame| {
            // a frame the page hasn't drawn yet gets replaced, never queued
            let _ = tx.try_send(frame);
            !tx.is_closed()
        }) {
            Ok(decoder) => self.live_view.decoder = Some(decoder),
            Err(e) => {
                log::warn!("no live preview: {e}");
                self.live_view.note = Note::Gone;
                return;
            }
        }
        self.live_view.tasks.push(cx.spawn(async move |this, cx| {
            while let Ok(frame) = rx.recv().await {
                let alive = this.update(cx, |app, cx| {
                    if app.live_view.generation != generation {
                        cx.drop_image(frame, None);
                        return;
                    }
                    if let Some(old) = app.live_view.picture.replace(frame) {
                        cx.drop_image(old, None);
                    }
                    app.live_view.note = Note::Showing;
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            // ffmpeg stopped on its own: the stream ended, or it couldn't read it
            let _ = this.update(cx, |app, cx| {
                if app.live_view.generation == generation && app.live_view.decoder.take().is_some() {
                    app.live_view.note = Note::Gone;
                }
                cx.notify();
            });
        }));
    }

    /// sound plays only fullscreen
    fn sync_sound(&mut self, url: &str, cx: &mut Context<Self>) {
        if !self.live_view.full {
            self.live_view.sound = None;
            self.live_view.no_sound = false;
            return;
        }
        if self.live_view.sound.is_some() || self.live_view.no_sound || self.demo || !self.ffmpeg {
            return;
        }
        match Sound::start(url) {
            Ok(sound) => self.live_view.sound = Some(sound),
            Err(e) => {
                log::warn!("no live sound: {e}");
                self.live_view.no_sound = true;
                self.toast(format!("no sound: {e}"), cx);
            }
        }
    }

    pub fn enter_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.live_view.full = true;
        self.live_view.seen_full = false;
        self.live_view.moved = Instant::now();
        if !window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.live_view.focus.focus(window, cx);
        cx.notify();
    }

    pub fn exit_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.live_view.full = false;
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    /// whether to draw the fullscreen picture. it ends with live view, or when
    /// the window leaves fullscreen some other way (the system's own button)
    pub fn showing_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.live_view.full {
            return false;
        }
        if window.is_fullscreen() {
            self.live_view.seen_full = true;
        }
        if !self.live_on() || (self.live_view.seen_full && !window.is_fullscreen()) {
            self.exit_fullscreen(window, cx);
            return false;
        }
        true
    }

    pub fn stop_preview(&mut self, cx: &mut Context<Self>) {
        self.live_view.sound = None;
        self.live_view.decoder = None;
        if let Some(old) = self.live_view.picture.take() {
            cx.drop_image(old, None);
        }
        self.live_view.note = Note::Waiting;
    }

    pub fn copy_live_url(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.live_status() {
            cx.write_to_clipboard(ClipboardItem::new_string(s.url.clone()));
            self.toast("copied the address", cx);
        }
    }
}

// the page

pub fn render(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let Some(s) = app.picked().cloned() else {
        return nothing(cx).into_any_element();
    };
    app.sync_preview(window, cx);
    let narrow = narrow(window);
    let connected = s.state == State::Connected;
    let remote = app.live(&s.fingerprint).and_then(|l| l.remote.clone()).filter(|_| connected);
    // older headsets don't say how many are watching, and can't stream either
    let can = remote.as_ref().is_some_and(|r| r.live_viewers.is_some());
    let status = app.live_status().filter(|_| app.live_view.frame.is_some());
    let on = app.live_on();
    if on {
        app.arm_tick(cx);
    }
    let mine = app.live_view.frame.as_deref() == Some(s.fingerprint.as_str());
    let picker = (app.statuses.len() > 1).then(|| frame::frames(app, &s.fingerprint, Page::Live, cx));
    let watching_frame = remote.as_ref().and_then(|r| r.live_viewers);
    let players = status.as_ref().map(|st| st.viewers.saturating_sub(u32::from(app.live_view.decoder.is_some())));
    let url = status.as_ref().map(|st| st.url.clone());

    div()
        .id("live-page")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(28.))
        .pt(px(26.))
        .pb(px(32.))
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(title("live", 26.))
        .children(picker)
        .child(
            div()
                .flex()
                .when(narrow, |d| d.flex_col())
                .items_stretch()
                .gap(px(14.))
                .child(now(&s, connected, can, on && mine, app.live_view.starting, status.as_ref(), cx).flex_1().min_w(px(300.)))
                .child(watching(connected, can, watching_frame, players, narrow)),
        )
        .child(picture(app, status.as_ref(), on && mine, cx))
        .child(obs(url.as_deref(), cx))
        .into_any_element()
}

fn nothing(cx: &mut Context<FrameApp>) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .child(title("live", 26.))
        .child(primary("live-pair-first", "pair a frame").on_click(cx.listener(|app, _, _, cx| app.go(Page::Pair, cx))))
}

/// the headline card: what the stream's doing, and start or stop
fn now(s: &Status, connected: bool, can: bool, on: bool, starting: bool, status: Option<&LiveStatus>, cx: &mut Context<FrameApp>) -> Div {
    let (head, note, tint): (&str, String, u32) = if starting {
        ("starting", String::new(), theme::YELLOW)
    } else if !connected {
        ("off", format!("connect to {} first.", s.name), theme::PEACH)
    } else if !can {
        ("off", format!("live view needs a newer framecorder on {}.", s.name), theme::PEACH)
    } else {
        match status.map(|st| &st.phase) {
            Some(LivePhase::Connecting) if on => ("connecting", String::new(), theme::YELLOW),
            Some(LivePhase::Live) if on => {
                let rate = status.map_or(0, |st| st.bytes_per_second);
                ("live", format::rate(rate as f64), theme::MAUVE)
            }
            Some(LivePhase::Error) => ("couldn't connect", status.and_then(|st| st.error.clone()).unwrap_or_else(|| "try again.".into()), theme::RED),
            Some(LivePhase::Stopped) => ("stopped", String::new(), theme::SUBTEXT0),
            _ => ("off", String::new(), theme::SUBTEXT0),
        }
    };
    let glyph: AnyElement = if starting || matches!(status.map(|st| &st.phase), Some(LivePhase::Connecting)) && on {
        spinner("live-spin", 18., c(tint)).into_any_element()
    } else if on {
        dot(theme::MAUVE, true).into_any_element()
    } else {
        icon("live", 20., c(tint)).into_any_element()
    };
    let fp = s.fingerprint.clone();
    card()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(18.))
        .when(on, |d| d.border_color(a(theme::MAUVE, 0.35)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(div().size(px(44.)).flex_none().flex().items_center().justify_center().rounded(px(12.)).bg(a(tint, 0.12)).child(glyph))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(label("live view"))
                        .child(div().font_family(theme::HEADING).font_weight(FontWeight::BOLD).text_size(px(20.)).truncate().child(head.to_string())),
                ),
        )
        .when(!note.is_empty(), |d| d.child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(note)))
        .child(div().flex().flex_wrap().items_center().gap(px(10.)).child(if on || starting {
            danger("live-stop", "stop").h(px(44.)).px(px(18.)).on_click(cx.listener(|app, _, _, cx| app.stop_live(cx))).into_any_element()
        } else if connected && can {
            primary("live-start", "start live view")
                .h(px(44.))
                .child(icon("live", 16., c(theme::CRUST)))
                .on_click(cx.listener(move |app, _, _, cx| app.start_live(fp.clone(), cx)))
                .into_any_element()
        } else {
            primary("live-start", "start live view").h(px(44.)).opacity(0.35).cursor_default().into_any_element()
        }))
}

/// who's watching: on the frame, and the players on this computer
fn watching(connected: bool, can: bool, on_frame: Option<u32>, players: Option<u32>, narrow: bool) -> Div {
    let line = |n: u32, one: &str, many: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(data(n.to_string()).text_size(px(22.)).text_color(c(if n > 0 { theme::MAUVE } else { theme::TEXT })))
            .child(div().min_w_0().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if n == 1 { one.to_string() } else { many.to_string() }))
    };
    let quiet = |t: &str| div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child(t.to_string());
    let col = card().when(!narrow, |d| d.w(px(250.))).flex_none().flex().flex_col().gap(px(14.)).p(px(16.)).child(label("watching"));
    match (on_frame, players) {
        (None, _) if !connected => col.child(quiet("not connected")),
        (None, _) if !can => col.child(quiet("needs a newer framecorder on the headset")),
        (None, _) => col.child(quiet("not reported")),
        (Some(f), p) => col
            .child(line(f, "app watching the frame", "apps watching the frame"))
            .child(line(p.unwrap_or(0), "player on this computer", "players on this computer")),
    }
}

/// the small picture, or why there isn't one
fn picture(app: &FrameApp, status: Option<&LiveStatus>, on: bool, cx: &mut Context<FrameApp>) -> Div {
    let live = on && status.is_some_and(|st| st.phase == LivePhase::Live);
    let can_full = live && (app.ffmpeg || app.demo);
    let body: AnyElement = match (&app.live_view.picture, live) {
        (Some(frame), _) => img(frame.clone()).size_full().object_fit(ObjectFit::Contain).into_any_element(),
        (None, false) => placeholder(icon("live", 22., c(theme::OVERLAY2)).into_any_element(), ""),
        (None, true) if app.ffmpeg => match app.live_view.note {
            Note::Gone => placeholder(icon("x", 18., c(theme::PEACH)).into_any_element(), "couldn't decode the stream here. obs still gets it: see below."),
            _ => placeholder(spinner("live-first", 18., c(theme::MAUVE)).into_any_element(), "waiting for the first picture..."),
        },
        (None, true) => placeholder(icon("info", 20., c(theme::PEACH)).into_any_element(), "install ffmpeg to see it here. obs doesn't need it: see below."),
    };
    card()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("the picture"))
                .when(can_full, |d| {
                    d.child(
                        outline("live-fullscreen", "fullscreen")
                            .child(icon("fullscreen", 14., c(theme::TEXT)))
                            .on_click(cx.listener(|app, _, window, cx| app.enter_fullscreen(window, cx))),
                    )
                }),
        )
        .child(
            div()
                .w_full()
                .h(px(preview::HEIGHT as f32))
                .rounded(px(10.))
                .overflow_hidden()
                .bg(c(theme::CRUST))
                .flex()
                .items_center()
                .justify_center()
                .child(body),
        )
}

fn placeholder(glyph: AnyElement, text: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .px(px(24.))
        .child(glyph)
        .when(!text.is_empty(), |d| d.child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).text_center().child(text.to_string())))
        .into_any_element()
}

/// the address obs plays, and the three steps
fn obs(url: Option<&str>, cx: &mut Context<FrameApp>) -> Div {
    let shown = url.unwrap_or("http://127.0.0.1:38620/live.ts");
    let step = |n: usize, text: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().size(px(26.)).flex_none().flex().items_center().justify_center().rounded(px(8.)).bg(a(theme::MAUVE, 0.12)).text_color(c(theme::MAUVE)).child(data((n + 1).to_string()).text_size(px(13.))))
            .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(text.to_string()))
    };
    card()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(16.))
        .child(label("record it in obs"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(44.))
                .pl(px(14.))
                .pr(px(6.))
                .rounded(px(12.))
                .bg(a(theme::CRUST, 0.35))
                .border_1()
                .border_color(theme::card_line())
                .child(data(SharedString::from(shown.to_string())).flex_1().min_w_0().truncate().text_size(px(14.)).text_color(c(if url.is_some() { theme::TEXT } else { theme::OVERLAY2 })))
                .child(
                    outline("live-copy", "copy")
                        .h(px(32.))
                        .child(icon("copy", 14., c(theme::TEXT)))
                        .on_click(cx.listener(|app, _, _, cx| app.copy_live_url(cx))),
                ),
        )
        .child(div().flex().flex_col().gap(px(8.)).children(OBS_STEPS.iter().enumerate().map(|(i, t)| step(i, t))))
}

/// the picture on its own, filling the screen, for casting the window
pub fn fullscreen(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> AnyElement {
    app.sync_preview(window, cx);
    app.arm_tick(cx);
    let controls = app.live_view.moved.elapsed() < CONTROLS_FOR;
    let body: AnyElement = match (&app.live_view.picture, app.live_view.note) {
        (Some(frame), _) => img(frame.clone()).size_full().object_fit(ObjectFit::Contain).into_any_element(),
        (None, Note::Gone) => placeholder(icon("x", 18., c(theme::PEACH)).into_any_element(), "couldn't decode the stream here."),
        (None, _) => placeholder(spinner("live-full-first", 18., c(theme::MAUVE)).into_any_element(), "waiting for the picture..."),
    };
    div()
        .id("live-full")
        .size_full()
        .relative()
        .bg(gpui::black())
        .track_focus(&app.live_view.focus)
        .on_key_down(cx.listener(|app, ev: &KeyDownEvent, window, cx| {
            if matches!(ev.keystroke.key.as_str(), "escape" | "f") {
                app.exit_fullscreen(window, cx);
                cx.stop_propagation();
            }
        }))
        .on_mouse_move(cx.listener(|app, _, _, cx| {
            app.live_view.moved = Instant::now();
            cx.notify();
        }))
        .on_click(cx.listener(|app, ev: &ClickEvent, window, cx| {
            if ev.click_count() >= 2 {
                app.exit_fullscreen(window, cx);
            }
        }))
        .child(body)
        .when(controls, |d| {
            d.child(
                div().absolute().top(px(20.)).right(px(20.)).child(
                    outline("live-exit-fullscreen", "exit fullscreen")
                        .bg(a(theme::CRUST, 0.8))
                        .child(icon("exit-fullscreen", 14., c(theme::TEXT)))
                        .on_click(cx.listener(|app, _, window, cx| app.exit_fullscreen(window, cx))),
                ),
            )
        })
        .into_any_element()
}

/// the screen the window's on, in pixels, as big as the picture should get
fn full_size(window: &Window, cx: &gpui::App) -> preview::Size {
    let logical = window.display(cx).map(|d| d.bounds().size).unwrap_or_else(|| window.viewport_size());
    let scale = window.scale_factor();
    preview::full((f32::from(logical.width) * scale) as u32, (f32::from(logical.height) * scale) as u32)
}
