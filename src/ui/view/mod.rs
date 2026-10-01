//! Lays out and draws the dashboard tab. Catppuccin Mocha, mauve accent.
//!
//! Two screens: home (record, clip, and what's set up at a glance) and
//! settings (one section at a time, so nothing is a wall of controls). And
//! the setup, which comes before either the first time round.

mod home;
mod onboarding;
mod settings;
mod sync;
mod widgets;

use std::time::Duration;

use super::pairing::{Device, Pairing};
use super::paint::{Canvas, Rect, Rgb};
use super::settings::{Eye, FrameRate, Quality, Settings, Shape};
use super::text::{Face, Fonts};

pub use onboarding::Step;

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
const CONTENT_Y: f32 = 150.0;
const CONTENT_H: f32 = 492.0;

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
    /// To another step of the setup.
    Step(Step),
    /// Done with the setup, or skipping the rest of it.
    FinishSetup,
    /// Takes framecorder off the headset, on the second tap.
    Uninstall,
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
    pub pairing: Option<Result<&'a Pairing, &'a str>>,
}

pub struct Model<'a> {
    pub settings: &'a Settings,
    pub screen: Screen,
    pub sync: SyncView<'a>,
    /// A clip was just asked for, the button's taking a second off.
    pub clip_cooling: bool,
    /// Whether the recorder may read the panels, or records SteamVR's view.
    pub unlocked: bool,
    pub status: Status,
    pub hover: Option<Action>,
    /// Last finished recording or error, and whether it went well.
    pub note: Option<(&'a str, bool)>,
    /// Remove was tapped once, the next tap does it.
    pub uninstall_armed: bool,
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
    canvas.hline(PAD, CONTENT_Y - 22.0, WIDTH as f32 - 2.0 * PAD, TEXT, 0.05);
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
    }
    footer(canvas, fonts, m);
    hits
}

/// The title with its mauve full stop, and one button on the right.
fn header(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, title: &str, button: Option<(&str, Action, bool)>) {
    let w = c.text(f, Face::Heading, 44.0, PAD, 92.0, title, TEXT, 1.0);
    c.text(f, Face::Heading, 44.0, PAD + w, 92.0, ".", MAUVE, 1.0);
    let Some((label, action, primary)) = button else { return };
    let r = Rect::new(WIDTH as f32 - PAD - 168.0, 50.0, 168.0, 54.0);
    widgets::button(c, f, m, hits, r, label, action, primary);
}

/// The last recording's result, or why the settings won't budge.
fn footer(c: &mut Canvas, f: &mut Fonts, m: &Model) {
    let locked_here = m.locked() && matches!(m.screen, Screen::Settings(s) if s != Section::Sync);
    let (text, color, text_color) = match m.note {
        _ if locked_here => ("Recording. Stop it to change these.", YELLOW, YELLOW),
        Some((text, true)) => (text, GREEN, SUBTEXT0),
        Some((text, false)) => (text, RED, RED),
        None => return,
    };
    let y = HEIGHT as f32 - 42.0;
    c.hline(PAD, y - 34.0, WIDTH as f32 - 2.0 * PAD, TEXT, 0.05);
    c.fill_circle(PAD + 6.0, y - 6.0, 5.0, color, 1.0);
    c.text(f, Face::Body, 17.0, PAD + 22.0, y, text, text_color, 1.0);
}

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
