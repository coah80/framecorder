//! the frame page: the headset, driven from here. record, stop and save
//! clips, see how it's doing, and set up its next recording.

use std::rc::Rc;
use std::time::Instant;

use framecorder_app_lib::core::api::{About, Recording, Remote};
use framecorder_app_lib::core::engine::{State, Status};
use gpui::{div, prelude::*, px, relative, AnyElement, Context, Div, FontWeight, SharedString, Stateful, Window};

use crate::app::{narrow, status_color, status_text, FrameApp, Page};
use crate::format;
use crate::remote;
use crate::sync::Clip;
use crate::theme::{self, a, c};
use crate::widgets::{card, data, dot, icon, label, outline, primary, spinner, switch, title};

const SHAPES: [(&str, &str); 4] = [("wide", "16:9"), ("square", "1:1"), ("tall", "9:16"), ("both", "both eyes")];
const EYES: [(&str, &str); 2] = [("left", "left eye"), ("right", "right eye")];
/// 0 is clips off
const LENGTHS: [(u32, &str); 5] = [(0, "off"), (15, "15 s"), (30, "30 s"), (60, "1 min"), (120, "2 min")];
const QUALITIES: [(&str, &str); 3] = [("standard", "standard"), ("high", "high"), ("max", "max")];
const RATES: [(&str, &str); 3] = [("auto", "auto"), ("60", "60 fps"), ("30", "30 fps")];

/// what the headset uses when it hasn't said yet
fn defaults() -> Recording {
    Recording {
        shape: "wide".into(),
        quality: "high".into(),
        fps: "auto".into(),
        game_audio: true,
        mic: true,
        clips: true,
        clip: 30,
        eye: None,
        panels: None,
    }
}

fn mbps(quality: &str) -> u32 {
    match quality {
        "standard" => 20,
        "max" => 80,
        _ => 40,
    }
}

pub fn render(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let Some(s) = app.picked().cloned() else {
        return nothing(cx).into_any_element();
    };
    let narrow = narrow(window);
    let connected = s.state == State::Connected;
    let live = app.live(&s.fingerprint);
    let remote = live.and_then(|l| l.remote.clone()).filter(|_| connected);
    let about = live.and_then(|l| l.about.clone()).filter(|_| connected);
    let settings = live.and_then(|l| l.recording.clone());
    let asked = live.is_some_and(|l| l.asked);
    let sending = live.and_then(|l| l.sending);
    let elapsed = live.map_or(0, |l| l.elapsed_ms(Instant::now()));
    if remote.as_ref().is_some_and(|r| r.recording && r.running) {
        app.arm_tick(cx);
    }
    // a download from this frame, as a share of the file
    let syncing = app
        .progress
        .as_ref()
        .filter(|p| p.fingerprint == s.fingerprint)
        .map(|p| if p.total > 0 { p.done as f32 / p.total as f32 } else { 1.0 });
    let picker = (app.statuses.len() > 1).then(|| frames(app, &s.fingerprint, cx));

    div()
        .id("frame-page")
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
        .child(header(&s))
        .children(picker)
        .child(
            // side by side, the headset under the remote once the window's too narrow for both
            div()
                .flex()
                .when(narrow, |d| d.flex_col())
                .items_stretch()
                .gap(px(14.))
                .child(now(&s, remote.as_ref(), elapsed, sending, settings.as_ref(), syncing, cx).flex_1().min_w(px(300.)))
                .child(headset(connected, about.as_ref(), narrow)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(label("next recording"))
                .child(next_recording(&s, settings.as_ref(), connected, cx))
                .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child(match (&settings, connected, asked) {
                    (_, false, _) => format!("connect to {} to change these.", s.name),
                    (Some(_), _, _) => "these take effect from the frame's next recording.".to_string(),
                    (None, _, true) => format!("these can be changed from here once {} has the latest framecorder.", s.name),
                    (None, _, false) => "asking the frame what it's set to...".to_string(),
                })),
        )
        .child(
            // side by side, under each other once the window's too narrow for both
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(px(14.))
                .child(here(&app.clips, &s.fingerprint).flex_1().min_w(px(260.)))
                .child(this_frame(&s.fingerprint, cx)),
        )
        .into_any_element()
}

/// what's come over from this frame, clips and recordings side by side
fn here(clips: &[Clip], fingerprint: &str) -> Div {
    let mine: Vec<&Clip> = clips.iter().filter(|c| c.host == fingerprint).collect();
    let total: u64 = mine.iter().map(|c| c.size).sum();
    let clip_bytes: u64 = mine.iter().filter(|c| c.is_clip).map(|c| c.size).sum();
    let share = if total > 0 { (clip_bytes as f32 / total as f32).clamp(0., 1.) } else { 0. };
    let legend = |color: u32, text: String| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(c(theme::SUBTEXT0))
            .child(dot(color, false).size(px(8.)))
            .child(data(text))
    };
    card()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(16.))
        .child(
            div().flex().items_center().justify_between().gap(px(12.)).child(label("on this computer").flex_none()).child(
                data(match mine.len() {
                    0 => "nothing yet".to_string(),
                    1 => format!("1 video · {}", format::size(total)),
                    n => format!("{n} videos · {}", format::size(total)),
                })
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(c(theme::SUBTEXT0)),
            ),
        )
        .when(total > 0, |d| {
            d.child(
                div()
                    .flex()
                    .h(px(8.))
                    .gap(px(3.))
                    .when(share > 0., |d| d.child(div().h_full().w(relative(share)).rounded_full().bg(c(theme::MAUVE))))
                    .when(share < 1., |d| {
                        d.child(div().h_full().w(relative(1. - share)).rounded_full().bg(a(theme::TEXT, 0.35)))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(16.))
                    .when(clip_bytes > 0, |d| d.child(legend(theme::MAUVE, format!("clips {}", format::size(clip_bytes)))))
                    .when(total > clip_bytes, |d| {
                        d.child(legend(theme::SUBTEXT0, format!("recordings {}", format::size(total - clip_bytes))))
                    }),
            )
        })
        .when(total == 0, |d| {
            d.child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child("save a clip on it and it lands here."))
        })
}

/// the rows about this frame: the connection check, a retry, and unpairing
fn this_frame(fingerprint: &str, cx: &mut Context<FrameApp>) -> Div {
    let fp = fingerprint.to_string();
    let row = |id: &str, icon_name: &str, head: &str, sub: &str, red: bool| {
        let ink = if red { theme::RED } else { theme::TEXT };
        div()
            .id(SharedString::from(format!("this-{id}")))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(9.))
            .cursor_pointer()
            .hover(|s| s.bg(a(theme::TEXT, 0.04)))
            .child(icon(icon_name, 16., c(if red { theme::RED } else { theme::SUBTEXT0 })))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(c(ink)).child(head.to_string()))
                    .child(div().text_size(px(11.)).text_color(c(theme::OVERLAY2)).truncate().child(sub.to_string())),
            )
    };
    let (check, unpair) = (fp.clone(), fp);
    card()
        .flex_1()
        .min_w(px(250.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(div().px(px(14.)).pt(px(12.)).pb(px(4.)).child(label("this frame")))
        .child(
            row("check", "wifi", "check the connection", "see what's getting in the way", false)
                .on_click(cx.listener(move |app, _, _, cx| app.open_check(check.clone(), cx))),
        )
        .child(
            row("retry", "retry", "try again now", "instead of waiting for the next look", false)
                .on_click(cx.listener(move |app, _, _, cx| app.retry(cx))),
        )
        .child(row("unpair", "unlink", "unpair", "what's already here stays", true).on_click(cx.listener(
            move |app, _, window, cx| {
                app.go(Page::Settings, cx);
                app.ask_unpair(unpair.clone(), window, cx);
            },
        )))
}

/// the page with nothing paired, which the pair screen normally covers
fn nothing(cx: &mut Context<FrameApp>) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .child(title("frame", 26.))
        .child(div().text_color(c(theme::SUBTEXT0)).child("pair a frame and you can drive it from here."))
        .child(primary("pair-first", "pair a frame").on_click(cx.listener(|app, _, _, cx| app.go(Page::Pair, cx))))
}

fn header(s: &Status) -> Div {
    let ip = s.addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(&s.addr).to_string();
    div().flex().flex_col().gap(px(6.)).child(title(&s.name, 26.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(13.))
            .text_color(c(theme::SUBTEXT0))
            .child(dot(status_color(s), false))
            .child(format!("steam frame · {} · {ip}", status_text(s))),
    )
}

/// which frame, when there's more than one
fn frames(app: &FrameApp, picked: &str, cx: &mut Context<FrameApp>) -> Div {
    div().flex().flex_wrap().gap(px(8.)).children(app.statuses.iter().map(|s| {
        let on = s.fingerprint == picked;
        let fp = s.fingerprint.clone();
        div()
            .id(SharedString::from(format!("pick-{fp}")))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(12.))
            .rounded_full()
            .border_1()
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .border_color(if on { c(theme::MAUVE) } else { theme::card_line() })
            .bg(if on { a(theme::MAUVE, 0.08) } else { theme::card() })
            .text_color(c(if on { theme::TEXT } else { theme::SUBTEXT0 }))
            .when(!on, |d| d.hover(|s| s.border_color(c(theme::SURFACE2)).text_color(c(theme::TEXT))))
            .on_click(cx.listener(move |app, _, _, cx| app.pick_frame(fp.clone(), cx)))
            .child(dot(status_color(s), false))
            .child(s.name.clone())
    }))
}

// now

#[allow(clippy::too_many_arguments)]
fn now(
    s: &Status,
    remote: Option<&Remote>,
    elapsed: u64,
    sending: Option<&str>,
    settings: Option<&Recording>,
    syncing: Option<f32>,
    cx: &mut Context<FrameApp>,
) -> Div {
    let (head, note) = remote::words(s, remote);
    let recording = remote.is_some_and(|r| r.recording);
    let red = matches!(s.state, State::Unreachable | State::Full);
    let tint = match s.state {
        State::Connected if recording => theme::RED,
        State::Connected => theme::MAUVE,
        State::Connecting => theme::YELLOW,
        _ if red => theme::RED,
        _ => theme::PEACH,
    };
    let glyph: AnyElement = match s.state {
        State::Connected if recording => div().size(px(12.)).rounded_full().bg(c(theme::RED)).into_any_element(),
        State::Connected => icon("headset", 20., c(tint)).into_any_element(),
        State::Connecting => spinner("now-looking", 18., c(tint)).into_any_element(),
        State::Unreachable => icon("wifi-off", 20., c(tint)).into_any_element(),
        State::Full => icon("disk", 20., c(tint)).into_any_element(),
        State::Unpaired => icon("headset", 20., c(tint)).into_any_element(),
        State::WrongFingerprint => icon("lock", 20., c(tint)).into_any_element(),
    };
    let fp = s.fingerprint.clone();
    card()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(18.))
        .when(recording, |d| d.border_color(a(theme::RED, 0.35)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(
                    div()
                        .size(px(44.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(12.))
                        .bg(a(tint, 0.12))
                        .child(glyph),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(label("now"))
                        .child(
                            div()
                                .font_family(theme::HEADING)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(20.))
                                .truncate()
                                .child(head),
                        ),
                )
                .when(recording, |d| d.child(clock(elapsed, remote.is_some_and(|r| r.running))))
                .when_some(syncing.filter(|_| !recording), |d, p| d.child(syncing_pill(p))),
        )
        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(note))
        // three buttons don't fit a narrow window on one line
        .child(div().flex().flex_wrap().items_center().gap(px(10.)).children(match s.state {
            State::Connected => {
                let usable = remote.is_some_and(|r| r.available && r.ready);
                let can_clip = usable && remote.is_some_and(|r| r.clip_ready && !r.recording);
                let clips_off = usable && remote.is_some_and(|r| r.clips.is_none());
                vec![
                    record_button(&fp, recording, usable && sending.is_none(), sending.is_some_and(|w| w != "clip"), cx)
                        .into_any_element(),
                    clip_button(&fp, clips_off, can_clip && sending.is_none(), sending == Some("clip"), cx)
                        .into_any_element(),
                    mic_button(&fp, settings, cx).into_any_element(),
                ]
            }
            State::Connecting => vec![],
            State::Unreachable | State::Full => vec![
                primary("now-check", "check the connection")
                    .h(px(44.))
                    .on_click(cx.listener(move |app, _, _, cx| app.open_check(fp.clone(), cx)))
                    .into_any_element(),
                outline("now-retry", "try again").h(px(44.)).on_click(cx.listener(|app, _, _, cx| app.retry(cx))).into_any_element(),
            ],
            State::Unpaired | State::WrongFingerprint => vec![primary("now-pair", "pair again")
                .h(px(44.))
                .on_click(cx.listener(move |app, _, _, cx| app.open_pair_for(fp.clone(), cx)))
                .into_any_element()],
        }))
}

/// how far a download from this frame is, in a mauve pill
fn syncing_pill(p: f32) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(32.))
        .px(px(12.))
        .rounded_full()
        .bg(a(theme::MAUVE, 0.12))
        .child(icon("download", 14., c(theme::MAUVE)))
        .child(data(format!("{}%", (p * 100.) as u32)).text_size(px(15.)).text_color(c(theme::MAUVE)))
}

/// how long it's been recording, in a red pill
fn clock(elapsed_ms: u64, running: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(32.))
        .px(px(12.))
        .rounded_full()
        .bg(a(theme::RED, 0.14))
        .child(dot(if running { theme::RED } else { theme::YELLOW }, false).size(px(8.)))
        .child(data(format::length(elapsed_ms as f64 / 1000.)).text_size(px(15.)).text_color(c(theme::RED)))
}

/// red and filled to start, hollow with a square in it to stop
fn record_button(fp: &str, recording: bool, enabled: bool, busy: bool, cx: &mut Context<FrameApp>) -> Stateful<Div> {
    let fp = fp.to_string();
    let what = if recording { "stop" } else { "record" };
    let ink = if recording { theme::RED } else { theme::CRUST };
    let glyph: AnyElement = if busy {
        spinner("record-busy", 14., c(ink)).into_any_element()
    } else if recording {
        div().size(px(12.)).rounded(px(3.)).bg(c(ink)).into_any_element()
    } else {
        div().size(px(12.)).rounded_full().bg(c(ink)).into_any_element()
    };
    let b = div()
        .id("record")
        .flex()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .h(px(44.))
        .px(px(20.))
        .rounded(px(12.))
        .font_family(theme::HEADING)
        .font_weight(FontWeight::BOLD)
        .text_size(px(14.))
        .text_color(c(ink))
        .child(glyph)
        .child(what);
    let b = if recording {
        b.bg(a(theme::RED, 0.14)).border_1().border_color(a(theme::RED, 0.5))
    } else {
        b.bg(c(theme::RED))
    };
    if !enabled {
        return b.opacity(0.35).cursor_default();
    }
    b.cursor_pointer()
        .hover(move |s| if recording { s.bg(a(theme::RED, 0.24)) } else { s.bg(c(theme::RED_LIGHT)) })
        .on_click(cx.listener(move |app, _, _, cx| app.command(&fp, what, false, cx)))
}

fn clip_button(fp: &str, clips_off: bool, enabled: bool, busy: bool, cx: &mut Context<FrameApp>) -> Stateful<Div> {
    let fp = fp.to_string();
    let glyph: AnyElement = if busy {
        spinner("clip-busy", 14., c(theme::TEXT)).into_any_element()
    } else {
        icon("clip", 16., c(theme::TEXT)).into_any_element()
    };
    let b = outline("clip", if clips_off { "clips off" } else { "save clip" }).h(px(44.)).px(px(18.)).child(glyph);
    if !enabled {
        return b.opacity(0.35).cursor_default();
    }
    b.on_click(cx.listener(move |app, _, _, cx| app.command(&fp, "clip", false, cx)))
}

/// the mic for the next recording, right on the remote: lit while it's on.
/// greyed until the frame has said what it's set to
fn mic_button(fp: &str, settings: Option<&Recording>, cx: &mut Context<FrameApp>) -> Stateful<Div> {
    let fp = fp.to_string();
    let on = settings.is_some_and(|r| r.mic);
    let b = outline("mic", if on { "mic on" } else { "mic off" })
        .h(px(44.))
        .px(px(18.))
        .child(icon(if on { "mic" } else { "mic-off" }, 16., c(if on { theme::MAUVE } else { theme::TEXT })));
    let Some(r) = settings.cloned() else {
        return b.opacity(0.35).cursor_default();
    };
    b.when(on, |d| d.bg(a(theme::MAUVE, 0.1)).border_color(a(theme::MAUVE, 0.4)).text_color(c(theme::MAUVE)))
        .on_click(cx.listener(move |app, _, _, cx| app.change_settings(&fp, Recording { mic: !r.mic, ..r.clone() }, cx)))
}

// the headset

fn headset(connected: bool, about: Option<&About>, narrow: bool) -> Div {
    let col = card()
        .when(!narrow, |d| d.w(px(250.)))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(16.))
        .child(label("the headset"));
    let quiet = |t: &str| div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child(t.to_string());
    match about {
        _ if !connected => col.child(quiet("battery and storage show up once it's connected.")),
        None => col.child(quiet("battery and storage show up once your frame has the next framecorder update.")),
        Some(About { battery: None, storage: None }) => col.child(quiet("the frame hasn't said how it's doing.")),
        Some(ab) => col
            .when_some(ab.battery.as_ref(), |d, b| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(battery(b.percent, b.charging))
                        .child(data(format!("{}%", b.percent)).text_size(px(15.)))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(c(if b.charging { theme::GREEN } else { theme::SUBTEXT0 }))
                                .child(if b.charging { "charging" } else { "on battery" }),
                        ),
                )
            })
            .when_some(ab.storage.as_ref(), |d, st| {
                let total = st.total.max(1) as f32;
                let videos = (st.videos as f32 / total).clamp(0., 1.);
                let other = ((st.total.saturating_sub(st.free).saturating_sub(st.videos)) as f32 / total).clamp(0., 1. - videos);
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .h(px(8.))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(a(theme::TEXT, 0.1))
                                .child(div().h_full().w(relative(videos)).bg(c(theme::MAUVE)))
                                .child(div().h_full().w(relative(other)).bg(a(theme::TEXT, 0.35))),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(c(theme::SUBTEXT0))
                                .child(dot(theme::MAUVE, false).size(px(8.)))
                                .child(format!("framecorder's videos take {}", format::size(st.videos))),
                        )
                        .child(
                            data(format!("{} free of {}", format::size(st.free), format::size(st.total)))
                                .text_size(px(12.))
                                .text_color(c(theme::SUBTEXT0)),
                        ),
                )
            }),
    }
}

/// a battery, filled to its level: green on the charger, red when it's nearly out
fn battery(percent: u8, charging: bool) -> Div {
    let level = f32::from(percent.min(100)) / 100.;
    let fill = if charging {
        theme::GREEN
    } else if percent <= 15 {
        theme::RED
    } else {
        theme::TEXT
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .w(px(30.))
                .h(px(15.))
                .p(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(a(theme::TEXT, 0.5))
                .child(div().h_full().w(relative(level)).rounded(px(2.)).bg(c(fill))),
        )
        .child(div().w(px(2.)).h(px(6.)).rounded(px(1.)).bg(a(theme::TEXT, 0.5)))
}

// next recording

type Pick = Rc<dyn Fn(usize) -> Recording>;

fn next_recording(s: &Status, settings: Option<&Recording>, connected: bool, cx: &mut Context<FrameApp>) -> Div {
    let enabled = connected && settings.is_some();
    let r = settings.cloned().unwrap_or_else(defaults);
    let fp = s.fingerprint.clone();
    // without the panels unlocked it records steamvr's view, 16:9 from the left eye, whatever was picked
    let locked = r.panels == Some(false);
    let shape = if locked { 0 } else { SHAPES.iter().position(|(v, _)| *v == r.shape).unwrap_or(0) };
    let both = SHAPES[shape].0 == "both";
    let eye = r.eye.as_deref().map(|e| if locked { 0 } else { EYES.iter().position(|(v, _)| *v == e).unwrap_or(0) });
    let length = if r.clips { LENGTHS.iter().position(|(v, _)| *v == r.clip).unwrap_or(2) } else { 0 };
    let quality = QUALITIES.iter().position(|(v, _)| *v == r.quality).unwrap_or(1);
    let rate = RATES.iter().position(|(v, _)| *v == r.fps).unwrap_or(0);

    let with = |f: fn(&Recording, usize) -> Recording| -> Pick {
        let r = r.clone();
        Rc::new(move |i| f(&r, i))
    };
    let shapes: Vec<Choice> =
        SHAPES.iter().enumerate().map(|(i, &(v, l))| Choice { label: l, glyph: Some(v), off: locked && i > 0 }).collect();
    let eyes: Vec<Choice> = EYES.iter().map(|&(_, l)| Choice { label: l, glyph: None, off: locked || both }).collect();
    let lengths: Vec<Choice> = LENGTHS.iter().map(|&(_, l)| Choice { label: l, glyph: None, off: false }).collect();

    card()
        .flex()
        .flex_col()
        .when(!enabled, |d| d.opacity(0.55))
        .child(setting(true, "shape", None).child(choices(
            &fp,
            "shape",
            &shapes,
            shape,
            enabled,
            with(|r, i| Recording { shape: SHAPES[i].0.into(), ..r.clone() }),
            cx,
        )))
        // older headsets don't say which eye, and then there's nothing to pick
        .when_some(eye, |d, eye| {
            d.child(setting(false, "eye", both.then(|| "both are in the video".into())).child(choices(
                &fp,
                "eye",
                &eyes,
                eye,
                enabled,
                with(|r, i| Recording { eye: Some(EYES[i].0.into()), ..r.clone() }),
                cx,
            )))
        })
        .when(locked, |d| d.child(locked_note(eye.is_some())))
        .child(setting(false, "clips", Some("the last moments, saved on demand".into())).child(choices(
            &fp,
            "clips",
            &lengths,
            length,
            enabled,
            with(|r, i| Recording { clips: i > 0, clip: if i > 0 { LENGTHS[i].0 } else { r.clip }, ..r.clone() }),
            cx,
        )))
        .child(setting(false, "quality", Some(format!("{} Mbit/s", mbps(&r.quality)))).child(choices(
            &fp,
            "quality",
            &plain(&QUALITIES),
            quality,
            enabled,
            with(|r, i| Recording { quality: QUALITIES[i].0.into(), ..r.clone() }),
            cx,
        )))
        .child(setting(false, "frame rate", None).child(choices(
            &fp,
            "rate",
            &plain(&RATES),
            rate,
            enabled,
            with(|r, i| Recording { fps: RATES[i].0.into(), ..r.clone() }),
            cx,
        )))
        .child(
            // the two switches side by side, one under the other in a narrow window
            setting(false, "sound", None).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(16.))
                    .gap_y(px(10.))
                    .child(toggle(
                        &fp,
                        "game-audio",
                        "game audio",
                        "what you hear in the headset",
                        r.game_audio,
                        enabled,
                        with(|r, _| Recording { game_audio: !r.game_audio, ..r.clone() }),
                        cx,
                    ))
                    .child(toggle(
                        &fp,
                        "mic",
                        "mic",
                        "your voice, mixed in",
                        r.mic,
                        enabled,
                        with(|r, _| Recording { mic: !r.mic, ..r.clone() }),
                        cx,
                    )),
            ),
        )
}

/// why the shapes and the eye are greyed out: the panels aren't unlocked on the headset
fn locked_note(has_eye: bool) -> Div {
    let what = if has_eye { "square, vertical, both eyes and the eye choice" } else { "square, vertical and both eyes" };
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .px(px(16.))
        .py(px(10.))
        .border_t_1()
        .border_color(theme::line())
        .text_size(px(12.))
        .text_color(c(theme::SUBTEXT0))
        .child(icon("lock", 14., c(theme::PEACH)).mt(px(2.)))
        .child(div().flex_1().min_w_0().child(format!("{what} need the panels unlocked on the headset; until then it records SteamVR's view.")))
}

fn setting(first: bool, name: &str, hint: Option<String>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(10.))
        .when(!first, |d| d.border_t_1().border_color(theme::line()))
        .child(
            div()
                .w(px(128.))
                .flex_none()
                .flex()
                .flex_col()
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(name.to_string()))
                .when_some(hint, |d, h| d.child(div().text_size(px(11.)).text_color(c(theme::OVERLAY2)).child(h))),
        )
}

struct Choice {
    label: &'static str,
    /// a shape to draw the outline of, over the label
    glyph: Option<&'static str>,
    /// greyed out: the headset can't do it right now
    off: bool,
}

fn plain(options: &[(&str, &'static str)]) -> Vec<Choice> {
    options.iter().map(|&(_, l)| Choice { label: l, glyph: None, off: false }).collect()
}

/// one row of options with one picked, like the library's filters
#[allow(clippy::too_many_arguments)]
fn choices(fp: &str, name: &str, options: &[Choice], picked: usize, enabled: bool, pick: Pick, cx: &mut Context<FrameApp>) -> Div {
    let tall = options.iter().any(|o| o.glyph.is_some());
    div()
        .flex_1()
        .flex()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(10.))
        .bg(a(theme::CRUST, 0.35))
        .border_1()
        .border_color(theme::card_line())
        .children(options.iter().enumerate().map(|(i, o)| {
            let on = i == picked;
            let can = enabled && !on && !o.off;
            let ink = if on { theme::CRUST } else { theme::SUBTEXT0 };
            let (fp, pick) = (fp.to_string(), pick.clone());
            div()
                .id(SharedString::from(format!("{name}-{i}")))
                .flex_1()
                .h(px(if tall { 46. } else { 32. }))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(5.))
                .rounded(px(8.))
                .text_size(px(if tall { 11. } else { 13. }))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ink))
                .when(on, |d| d.bg(c(theme::MAUVE)))
                .when(o.off, |d| d.opacity(0.4))
                .when(can, |d| d.cursor_pointer().hover(|s| s.text_color(c(theme::MAUVE)).bg(a(theme::MAUVE, 0.08))))
                .when(can, |d| d.on_click(cx.listener(move |app, _, _, cx| app.change_settings(&fp, pick(i), cx))))
                .when_some(o.glyph, |d, g| d.child(shape_glyph(g, ink)))
                .child(o.label)
        }))
}

/// the outline of the picture each shape makes
fn shape_glyph(shape: &str, ink: u32) -> Div {
    let frame = |w: f32, h: f32| div().w(px(w)).h(px(h)).rounded(px(3.)).border_2().border_color(c(ink));
    let row = div().flex().items_center().h(px(18.)).gap(px(2.));
    match shape {
        "square" => row.child(frame(16., 16.)),
        "tall" => row.child(frame(11., 18.)),
        "both" => row.child(frame(11., 14.)).child(frame(11., 14.)),
        _ => row.child(frame(24., 14.)),
    }
}

/// a switch with its name and what it's for, half a row wide
#[allow(clippy::too_many_arguments)]
fn toggle(fp: &str, id: &str, name: &str, sub: &str, on: bool, enabled: bool, pick: Pick, cx: &mut Context<FrameApp>) -> Div {
    let fp = fp.to_string();
    div()
        .flex_1()
        .min_w(px(190.))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(px(13.)).child(name.to_string()))
                .child(div().text_size(px(11.)).text_color(c(theme::OVERLAY2)).truncate().child(sub.to_string())),
        )
        .child(
            switch(SharedString::from(format!("switch-{id}")), on)
                .when(enabled, |d| d.on_click(cx.listener(move |app, _, _, cx| app.change_settings(&fp, pick(0), cx)))),
        )
}
