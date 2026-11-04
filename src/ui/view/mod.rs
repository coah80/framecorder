//! Lays out and draws the dashboard tab. Catppuccin Mocha, mauve accent.
//!
//! Two screens: home (record, clip, and what's set up at a glance) and
//! settings (its sections along the top, one at a time, so nothing is a
//! wall of controls). And the setup, which comes before either the first
//! time round, and reporting an issue, a step off the home screen. Every
//! screen is a title with a chip saying what's being recorded, one button
//! on the right, the content, and a note at the foot.

mod home;
mod onboarding;
mod report;
mod settings;
mod sync;
mod widgets;

use std::time::Duration;

use super::pairing::clients::Update;
use super::pairing::{Device, Pairing, Presence};
use super::paint::{Canvas, Rect, Rgb};
use super::settings::{Eye, FrameRate, Quality, Settings, Shape};
use super::text::{Face, Fonts};

pub use onboarding::Step;
pub use report::{Phase, ReportView};
pub use sync::SHOWN as DEVICES_PER_PAGE;

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 760;

const CRUST: Rgb = [0x11, 0x11, 0x1b];
const BASE: Rgb = [0x1e, 0x1e, 0x2e];
const SURFACE0: Rgb = [0x31, 0x32, 0x44];
const SURFACE1: Rgb = [0x45, 0x47, 0x5a];
const TEXT: Rgb = [0xcd, 0xd6, 0xf4];
const SUBTEXT0: Rgb = [0xa6, 0xad, 0xc8];
const OVERLAY1: Rgb = [0x7f, 0x84, 0x9c];
const OVERLAY2: Rgb = [0x93, 0x99, 0xb2];
const MAUVE: Rgb = [0xcb, 0xa6, 0xf7];
const RED: Rgb = [0xf3, 0x8b, 0xa8];
const GREEN: Rgb = [0xa6, 0xe3, 0xa1];
const YELLOW: Rgb = [0xf9, 0xe2, 0xaf];

const PAD: f32 = 48.0;
/// The title's baseline, and the header buttons sit on the same line.
const HEADER_Y: f32 = 40.0;
const HEADER_H: f32 = 54.0;
const CONTENT_Y: f32 = 114.0;
const CONTENT_H: f32 = 574.0;
/// The hairline over the foot note.
const FOOTER_Y: f32 = 702.0;
const BUTTON_H: f32 = 54.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Video,
    Audio,
    Clips,
    Sync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    Settings(Section),
    Onboarding(Step),
    Report,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Record,
    ClipNow,
    /// Clips on or off.
    Clips,
    /// Opens settings on a section, or switches to it.
    Open(Section),
    /// Back to the home screen.
    Close,
    Shape(Shape),
    Eye(Eye),
    Quality(Quality),
    Fps(FrameRate),
    GameAudio,
    Mic,
    /// Replay buffer length.
    ClipLength(u32),
    Pair,
    ClosePair,
    RemoveDevice(usize),
    /// Another page of the paired devices list.
    DevicePage(usize),
    /// To another step of the setup.
    Step(Step),
    /// Done with the setup, or skipping the rest of it.
    FinishSetup,
    /// Stops all of framecorder until SteamVR starts again.
    TurnOff,
    /// Opens the report screen.
    Report,
    /// Brings up SteamVR's keyboard to say what went wrong.
    Type,
    /// Sends the report.
    Send,
}

pub enum Status {
    Idle,
    Recording { recorded: Duration },
    Stopping,
}

/// What's known about syncing: whether the service is there, what's paired,
/// and the code on show while pairing.
pub struct SyncView<'a> {
    pub available: bool,
    pub devices: &'a [Device],
    /// Which page of the list the sync section is on.
    pub page: usize,
    pub pairing: Option<Result<&'a Pairing, &'a str>>,
    /// An update of framecorder going on, started from an app or the timer.
    pub update: &'a Update,
}

pub struct Model<'a> {
    pub settings: &'a Settings,
    pub screen: Screen,
    pub sync: SyncView<'a>,
    /// A clip was just asked for, the button's taking a second off.
    pub clip_cooling: bool,
    /// How many apps are watching the live view.
    pub live_viewers: usize,
    pub report: ReportView<'a>,
    /// Whether the recorder may read the panels, or records SteamVR's view.
    pub unlocked: bool,
    pub status: Status,
    pub hover: Option<Action>,
    /// Last finished recording or error, and whether it went well.
    pub note: Option<(&'a str, bool)>,
    /// An update took the panels' permission away.
    pub relocked: bool,
    /// Bytes free where recordings go, when that could be measured.
    pub free: Option<u64>,
}

impl Model<'_> {
    /// Recording settings can't change under a recording.
    fn locked(&self) -> bool {
        !matches!(self.status, Status::Idle)
    }
}

pub struct Hit {
    pub rect: Rect,
    pub action: Action,
}

pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, m: &Model) -> Vec<Hit> {
    let mut hits = Vec::new();
    canvas.clear(BASE, 1.0);
    match m.screen {
        Screen::Home => {
            header(canvas, fonts, m, &mut hits, "framecorder", Some(("Settings", Action::Open(Section::Video), false)));
            home::draw(canvas, fonts, m, &mut hits);
        }
        Screen::Settings(section) => {
            header(canvas, fonts, m, &mut hits, "Settings", Some(("Done", Action::Close, true)));
            settings::draw(canvas, fonts, m, section, &mut hits);
        }
        Screen::Onboarding(step) => {
            // Nothing left to skip on the last step.
            let skip = (step != Step::Done).then_some(("Skip", Action::FinishSetup, false));
            header(canvas, fonts, m, &mut hits, "Set up", skip);
            onboarding::draw(canvas, fonts, m, step, &mut hits);
        }
        Screen::Report => {
            let button = match m.report.phase {
                Phase::Sent { .. } => ("Done", Action::Close, true),
                _ => ("Cancel", Action::Close, false),
            };
            header(canvas, fonts, m, &mut hits, "Report an issue", Some(button));
            report::draw(canvas, fonts, m, &mut hits);
        }
    }
    footer(canvas, fonts, m, &mut hits);
    hits
}

/// The title with its mauve full stop, the chip saying what's being
/// recorded, and one button on the right.
fn header(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, title: &str, button: Option<(&str, Action, bool)>) {
    let recording = m.locked();
    let baseline = HEADER_Y + 40.0;
    let w = c.text(f, Face::Heading, 40.0, PAD, baseline, title, TEXT, 1.0);
    let w = w + c.text(f, Face::Heading, 40.0, PAD + w, baseline, ".", if recording { RED } else { MAUVE }, 1.0);

    let rec = match m.status {
        Status::Recording { recorded } => format!("REC {}", clock(recorded)),
        Status::Stopping => "Saving".to_string(),
        Status::Idle => String::new(),
    };
    let (text, color) = match (recording, m.unlocked) {
        (true, _) => (rec.as_str(), RED),
        (false, true) => ("Panels unlocked", GREEN),
        (false, false) => ("SteamVR's view", YELLOW),
    };
    let cy = HEADER_Y + HEADER_H / 2.0;
    let mut x = PAD + w + 20.0;
    x += widgets::chip(c, f, x, cy, text, color) + 12.0;
    x += match m.sync.update {
        Update::Updating => widgets::chip(c, f, x, cy, "Updating…", YELLOW) + 12.0,
        Update::Failed(_) => widgets::chip(c, f, x, cy, "Update failed", RED) + 12.0,
        Update::Idle => 0.0,
    };
    // Someone's watching the live view from an app.
    if m.live_viewers > 0 {
        widgets::chip(c, f, x, cy, &format!("Live · {} watching", m.live_viewers), MAUVE);
    }

    let Some((label, action, primary)) = button else { return };
    let r = Rect::new(WIDTH as f32 - PAD - 168.0, HEADER_Y, 168.0, BUTTON_H);
    widgets::button(c, f, m, hits, r, label, action, primary);
}

/// The last recording's result, why the settings won't budge, or what's
/// worth knowing about the screen that's open. Settings also keep the way
/// to close framecorder down here, out of the way, and home the way to
/// report an issue.
fn footer(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    let section = match m.screen {
        Screen::Settings(s) => Some(s),
        _ => None,
    };
    let locked_here = m.locked() && section.is_some_and(|s| s != Section::Sync);
    let failed = match m.sync.update {
        Update::Failed(why) => Some(update_failed(why.as_deref())),
        _ => None,
    };
    let line: Option<(String, Rgb, Rgb)> = match m.note {
        _ if m.screen == Screen::Report => report_note(m),
        _ if locked_here => Some(("Recording. Stop it to change these.".into(), YELLOW, YELLOW)),
        _ if *m.sync.update == Update::Updating => Some((UPDATING.into(), YELLOW, YELLOW)),
        _ if m.relocked && !m.unlocked => Some((RELOCKED.into(), YELLOW, YELLOW)),
        Some((text, true)) => Some((text.into(), GREEN, SUBTEXT0)),
        Some((text, false)) => Some((text.into(), RED, RED)),
        None if failed.is_some() => failed.map(|f| (f, RED, RED)),
        None => match section {
            Some(Section::Sync) => Some((SYNC_NOTE.into(), YELLOW, YELLOW)),
            Some(_) => Some(("Changes save on their own and apply to the next recording.".into(), OVERLAY1, SUBTEXT0)),
            None => None,
        },
    };
    let link = match m.screen {
        Screen::Home => Some(("Report an issue", Action::Report, MAUVE)),
        Screen::Settings(_) => Some(("Close framecorder", Action::TurnOff, OVERLAY2)),
        _ => None,
    };
    if line.is_none() && link.is_none() {
        return;
    }
    let y = FOOTER_Y + 27.0;
    c.hline(PAD, FOOTER_Y, WIDTH as f32 - 2.0 * PAD, TEXT, 0.05);

    let mut room = WIDTH as f32 - 2.0 * PAD - 22.0;
    if let Some((label, action, color)) = link {
        let w = f.measure(Face::Body, 16.0, label) + 24.0;
        let r = Rect::new(WIDTH as f32 - PAD - w, FOOTER_Y + 8.0, w, 44.0);
        let hovered = m.hover == Some(action);
        let (color, alpha) = if hovered { (MAUVE, 1.0) } else { (color, if color == MAUVE { 0.85 } else { 1.0 }) };
        c.text(f, Face::Body, 16.0, r.x + 12.0, y, label, color, alpha);
        hits.push(Hit { rect: r, action });
        room -= w + 24.0;
    }
    if let Some((text, color, text_color)) = line {
        c.fill_circle(PAD + 6.0, y - 6.0, 5.0, color, 1.0);
        let text = f.fit(Face::Body, 16.0, &text, room);
        c.text(f, Face::Body, 16.0, PAD + 22.0, y, &text, text_color, 1.0);
    }
}

/// What to know on the report screen: that it's public, that it's on its
/// way, that it went, or why it didn't.
fn report_note(m: &Model) -> Option<(String, Rgb, Rgb)> {
    Some(match (m.report.phase, m.note) {
        (Phase::Failed(why), _) => (format!("Couldn't send it: {why}"), RED, RED),
        (Phase::Sending, _) => ("Sending…".into(), YELLOW, YELLOW),
        (Phase::Sent { .. }, _) => ("Sent. Thank you for taking the time.".into(), GREEN, SUBTEXT0),
        (Phase::Writing, Some((text, false))) => (text.into(), RED, RED),
        (Phase::Writing, _) => (PUBLIC_NOTE.into(), YELLOW, YELLOW),
    })
}

/// A report is a GitHub issue anyone can read.
const PUBLIC_NOTE: &str = "This goes on a public GitHub issue anyone can read, with your Steam name and framecorder's logs. Nothing from your videos.";

/// Clips only move while the headset is up; say so where it matters.
const SYNC_NOTE: &str = "Clips only sync while your Frame is on, on the same Wi-Fi, with the framecorder app open on the other device.";

/// An update replaced the recorder, which drops its permission.
const RELOCKED: &str = "Updated. Recording SteamVR's view until the installer unlocks the panels again.";

/// An app or the timer is updating framecorder; the tab restarts itself
/// once it's not recording and nobody's looking at it.
const UPDATING: &str = "Updating framecorder… it restarts on its own once you're out of this tab and not recording.";

/// The last update an app started didn't go through, in plain words.
fn update_failed(why: Option<&str>) -> String {
    match why.map(str::trim).filter(|w| !w.is_empty()) {
        Some(why) => format!("The update didn't go through: {}. Try again from the app, or run the installer.", why.trim_end_matches('.')),
        None => "The update didn't go through. Try again from the app, or run the installer.".to_string(),
    }
}

/// One line about a device beyond its name, fitted to `max` px: what it's
/// getting (the clip's name gives way before the percent does), that it's
/// on the old app, or that it's not around. Nothing when it's just connected.
pub fn device_line(f: &mut Fonts, d: &Device, max: f32) -> Option<(String, Rgb)> {
    if let Some(t) = &d.transfer {
        let (head, tail) = ("Getting ", format!(" · {}%", t.percent()));
        let room = max - f.measure(Face::Body, 14.0, head) - f.measure(Face::Body, 14.0, &tail);
        let clip = f.fit(Face::Body, 14.0, t.name.trim_end_matches(".mp4"), room);
        return Some((format!("{head}{clip}{tail}"), MAUVE));
    }
    if d.old_app {
        return Some(("Old app".to_string(), YELLOW));
    }
    (d.presence == Presence::Away).then(|| ("Not connected".to_string(), OVERLAY1))
}

/// The dot in front of a device's name: green while it's connected (or
/// nothing's known), grey when it's not around.
pub fn presence_color(d: &Device) -> Rgb {
    match d.presence {
        Presence::Away => OVERLAY1,
        Presence::Connected | Presence::Unknown => GREEN,
    }
}

/// How much room recordings have left, and roughly how long that is at the
/// quality that's set.
pub fn free_space(bytes: u64, mbps: u32) -> String {
    let gb = bytes as f64 / 1e9;
    let room = if gb >= 10.0 { format!("{gb:.0} GB free") } else { format!("{gb:.1} GB free") };
    let secs = bytes as f64 * 8.0 / (mbps.max(1) as f64 * 1e6);
    let time = if secs >= 3600.0 * 1.5 {
        format!("about {:.0} h", secs / 3600.0)
    } else if secs >= 3600.0 {
        "about an hour".to_string()
    } else if secs >= 60.0 {
        format!("about {:.0} min", secs / 60.0)
    } else {
        return format!("{room} · no room to record");
    };
    format!("{room} · {time} of recording")
}

/// Under this much free, the room line turns yellow.
pub const LOW_SPACE: u64 = 2_000_000_000;

pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn length(secs: u32) -> String {
    if secs >= 60 && secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

/// The note that floats up after a clip.
pub fn toast(fonts: &mut Fonts, text: &str, ok: bool) -> Canvas {
    let mut c = Canvas::new(TOAST_W, TOAST_H);
    c.clear(BASE, 0.0);
    let r = Rect::new(2.0, 2.0, TOAST_W as f32 - 4.0, TOAST_H as f32 - 4.0);
    c.fill_rrect(r, r.h / 2.0, BASE, 0.92);
    c.stroke_rrect(r, r.h / 2.0, 2.0, if ok { MAUVE } else { RED }, 0.8);
    c.fill_circle(r.x + 44.0, r.y + r.h / 2.0, 9.0, if ok { MAUVE } else { RED }, 1.0);
    c.text(fonts, Face::BodyMedium, 30.0, r.x + 74.0, r.y + r.h / 2.0 + 10.5, text, TEXT, 1.0);
    c
}

pub const TOAST_W: u32 = 640;
pub const TOAST_H: u32 = 96;

/// The tab's icon in the dashboard bar: a mauve record dot.
pub fn icon(size: u32) -> Canvas {
    let mut c = Canvas::new(size, size);
    let r = size as f32 / 2.0;
    c.ring(r, r, r - 2.0, size as f32 * 0.07, MAUVE, 1.0);
    c.fill_circle(r, r, r * 0.55, MAUVE, 1.0);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_says_how_long_recordings_have_left() {
        assert_eq!(free_space(128_400_000_000, 40), "128 GB free · about 7 h of recording");
        assert_eq!(free_space(1_400_000_000, 40), "1.4 GB free · about 5 min of recording");
        assert_eq!(free_space(20_000_000_000, 40), "20 GB free · about an hour of recording");
        assert_eq!(free_space(100_000_000, 40), "0.1 GB free · no room to record");
    }
}
