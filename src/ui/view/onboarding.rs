//! The first-run setup: one choice a step, with the defaults already picked,
//! so pressing next all the way through ends up somewhere good. Shown once,
//! before the record button ever is.

use super::super::settings::CLIP_LENGTHS;
use super::widgets::{button, card, label, segmented, toggle_row};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Shape,
    Quality,
    Audio,
    Clips,
    Sync,
    Done,
}

const STEPS: [Step; 7] = [Step::Welcome, Step::Shape, Step::Quality, Step::Audio, Step::Clips, Step::Sync, Step::Done];

impl Step {
    /// Pairing only gets asked about where there's a sync service to pair with.
    fn shown(self, sync: bool) -> bool {
        self != Step::Sync || sync
    }

    /// Whether there's something to pick, unlike the hello and goodbye.
    fn asks(self) -> bool {
        !matches!(self, Step::Welcome | Step::Done)
    }

    pub fn next(self, sync: bool) -> Option<Step> {
        STEPS.iter().copied().skip_while(|&s| s != self).skip(1).find(|s| s.shown(sync))
    }

    pub fn back(self, sync: bool) -> Option<Step> {
        STEPS.iter().rev().copied().skip_while(|&s| s != self).skip(1).find(|s| s.shown(sync))
    }

    /// Which question this is and out of how many, for the steps that ask one.
    pub fn progress(self, sync: bool) -> (usize, usize) {
        let asked = || STEPS.iter().filter(|s| s.asks() && s.shown(sync));
        let at = match self {
            Step::Welcome => 0,
            Step::Done => asked().count(),
            step => asked().take_while(|&&s| s != step).count() + 1,
        };
        (at, asked().count())
    }
}

const CARD_H: f32 = 412.0;
const INSET: f32 = 40.0;
const NAV_Y: f32 = CONTENT_Y + CARD_H + 20.0;
const NAV_W: f32 = 200.0;
const NAV_H: f32 = 60.0;
/// Where a step's controls start, under its heading.
const CONTROLS_Y: f32 = 118.0;
const PICK_H: f32 = 68.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, step: Step, hits: &mut Vec<Hit>) {
    let pane = Rect::new(PAD, CONTENT_Y, WIDTH as f32 - 2.0 * PAD, CARD_H);
    card(c, pane);
    let inner = Rect::new(pane.x + INSET, pane.y + 28.0, pane.w - 2.0 * INSET, pane.h - 50.0);
    match step {
        Step::Welcome => hello(c, f, pane, "Let's set up your recordings", "A few quick choices. You can change them all later in settings."),
        Step::Shape => shape(c, f, m, inner, hits),
        Step::Quality => quality(c, f, m, inner, hits),
        Step::Audio => audio(c, f, m, inner, hits),
        Step::Clips => clips(c, f, m, inner, hits),
        Step::Sync => pair(c, f, m, inner, hits),
        Step::Done => hello(c, f, pane, "You're all set", "Hit record, then switch tabs or close the menu and it starts."),
    }
    nav(c, f, m, step, hits);
}

/// Back on the left, where you are in the middle, next on the right.
fn nav(c: &mut Canvas, f: &mut Fonts, m: &Model, step: Step, hits: &mut Vec<Hit>) {
    let sync = m.sync.available;
    if let Some(back) = step.back(sync) {
        button(c, f, m, hits, Rect::new(PAD, NAV_Y, NAV_W, NAV_H), "Back", Action::Step(back), false);
    }
    let r = Rect::new(WIDTH as f32 - PAD - NAV_W, NAV_Y, NAV_W, NAV_H);
    match (step, step.next(sync)) {
        (Step::Welcome, Some(next)) => button(c, f, m, hits, r, "Get started", Action::Step(next), true),
        (_, Some(next)) => button(c, f, m, hits, r, "Next", Action::Step(next), true),
        (_, None) => button(c, f, m, hits, r, "Finish", Action::FinishSetup, true),
    }

    let (at, of) = step.progress(sync);
    let (cx, cy) = (WIDTH as f32 / 2.0, NAV_Y + NAV_H / 2.0);
    let gap = 28.0;
    let x0 = cx - gap * (of as f32 - 1.0) / 2.0;
    // The dots move up to make room for the count under them.
    let y = if step.asks() { cy - 12.0 } else { cy };
    for i in 1..=of {
        let x = x0 + (i - 1) as f32 * gap;
        if i == at && step.asks() {
            c.fill_circle(x, y, 8.0, MAUVE, 1.0);
        } else {
            c.fill_circle(x, y, 6.0, if i <= at { MAUVE } else { SURFACE1 }, if i <= at { 0.5 } else { 1.0 });
        }
    }
    if step.asks() {
        c.text_centered(f, Face::Data, 18.0, cx, cy + 24.0, &format!("{at} of {of}"), SUBTEXT0, 1.0);
    }
}

/// The first and last steps: the record dot, a heading, and one line.
fn hello(c: &mut Canvas, f: &mut Fonts, pane: Rect, heading: &str, line: &str) {
    let (cx, cy) = (pane.x + pane.w / 2.0, pane.y + 124.0);
    c.ring(cx, cy, 52.0, 4.0, MAUVE, 0.7);
    c.fill_circle(cx, cy, 38.0, MAUVE, 0.92);
    c.text_centered(f, Face::Heading, 40.0, cx, cy + 124.0, heading, TEXT, 1.0);
    c.text_centered(f, Face::Body, 21.0, cx, cy + 172.0, line, SUBTEXT0, 1.0);
}

/// What's being asked, and what the current pick means.
fn question(c: &mut Canvas, f: &mut Fonts, inner: Rect, heading: &str, meaning: &str) {
    c.text(f, Face::Heading, 32.0, inner.x, inner.y + 40.0, heading, TEXT, 1.0);
    c.text(f, Face::Body, 19.0, inner.x, inner.y + 78.0, meaning, SUBTEXT0, 1.0);
}

fn shape(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let meaning = match s.shape {
        Shape::Wide => "Widescreen · 1920 × 1080 · good for most things",
        Shape::Square => "Square · 1440 × 1440",
        Shape::Tall => "Vertical · 1080 × 1920 · good for phones",
        Shape::BothEyes => "Both panels, just as the headset shows them",
    };
    question(c, f, inner, "What shape should your videos be?", meaning);
    let shapes = [
        ("16:9", Action::Shape(Shape::Wide), s.shape == Shape::Wide),
        ("1:1", Action::Shape(Shape::Square), s.shape == Shape::Square),
        ("9:16", Action::Shape(Shape::Tall), s.shape == Shape::Tall),
        ("Both eyes", Action::Shape(Shape::BothEyes), s.shape == Shape::BothEyes),
    ];
    let y = inner.y + CONTROLS_Y;
    segmented(c, f, m, hits, Rect::new(inner.x, y, inner.w, PICK_H), &shapes, false);

    // With both eyes in the video there's no eye to pick.
    if s.shape == Shape::BothEyes {
        return;
    }
    label(c, f, inner.x, y + PICK_H + 46.0, "THE EYE THAT GETS RECORDED");
    let eyes = [
        ("Left eye", Action::Eye(Eye::Left), s.eye == Eye::Left),
        ("Right eye", Action::Eye(Eye::Right), s.eye == Eye::Right),
    ];
    segmented(c, f, m, hits, Rect::new(inner.x, y + PICK_H + 62.0, inner.w / 2.0, PICK_H), &eyes, false);
}

fn quality(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let meaning = match s.quality {
        Quality::Standard => "Smaller files that still look good",
        Quality::High => "Sharp, without the files getting huge",
        Quality::Max => "As sharp as it gets, with the biggest files",
    };
    question(c, f, inner, "How sharp should they be?", meaning);
    let qualities = [
        ("Standard", Action::Quality(Quality::Standard), s.quality == Quality::Standard),
        ("High", Action::Quality(Quality::High), s.quality == Quality::High),
        ("Max", Action::Quality(Quality::Max), s.quality == Quality::Max),
    ];
    segmented(c, f, m, hits, Rect::new(inner.x, inner.y + CONTROLS_Y, inner.w, PICK_H), &qualities, false);
}

fn audio(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    question(c, f, inner, "What should they sound like?", "Turn off anything you don't want in the video.");
    let at = |i: f32| Rect::new(inner.x, inner.y + CONTROLS_Y + i * 108.0, inner.w, 92.0);
    toggle_row(c, f, m, hits, at(0.0), "Game audio", "Everything you hear from the game", Action::GameAudio, s.game_audio, false);
    let mic = if s.game_audio { "Your voice, mixed in with the game" } else { "Your voice" };
    toggle_row(c, f, m, hits, at(1.0), "Microphone", mic, Action::Mic, s.mic, false);
}

fn clips(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let meaning = if s.clips {
        format!("Hold the left thumbstick to clip the last {}", length(s.clip_secs))
    } else {
        "Nothing is kept, so there's nothing to clip".to_string()
    };
    question(c, f, inner, "Clip what just happened", &meaning);
    let y = inner.y + CONTROLS_Y;
    let kept = "Keeps the last bit ready to save, the whole time you play";
    toggle_row(c, f, m, hits, Rect::new(inner.x, y, inner.w, 92.0), "Clips", kept, Action::Clips, s.clips, false);

    label(c, f, inner.x, y + 92.0 + 40.0, "HOW MUCH A CLIP KEEPS");
    let lengths: Vec<(String, Action, bool)> = CLIP_LENGTHS.iter().map(|&n| (length(n), Action::ClipLength(n), s.clip_secs == n)).collect();
    let options: Vec<(&str, Action, bool)> = lengths.iter().map(|(t, a, on)| (t.as_str(), *a, *on)).collect();
    segmented(c, f, m, hits, Rect::new(inner.x, y + 92.0 + 56.0, inner.w, PICK_H), &options, !s.clips);
}

/// Pairing is the one step that's fine to leave for later.
fn pair(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    // The code, or why there's no pairing, look the same as in settings.
    if !m.sync.available || m.sync.pairing.is_some() {
        return sync::draw(c, f, m, inner, hits);
    }
    question(c, f, inner, "Get clips on your phone or computer", "Pair the framecorder app now, or later in settings.");
    let y = inner.y + CONTROLS_Y;
    button(c, f, m, hits, Rect::new(inner.x, y, 240.0, PICK_H), "Pair a device", Action::Pair, false);

    const SHOWN: usize = 3;
    let list = m.sync.devices;
    if list.is_empty() {
        return;
    }
    let x = inner.x + 240.0 + 48.0;
    label(c, f, x, y + 16.0, "PAIRED");
    for (i, d) in list.iter().take(SHOWN).enumerate() {
        let line = y + 52.0 + i as f32 * 34.0;
        c.fill_circle(x + 6.0, line - 7.0, 5.0, GREEN, 1.0);
        c.text(f, Face::BodyMedium, 19.0, x + 24.0, line, &d.name, TEXT, 1.0);
    }
    if list.len() > SHOWN {
        let more = format!("and {} more", list.len() - SHOWN);
        c.text(f, Face::Body, 16.0, x + 24.0, y + 52.0 + SHOWN as f32 * 34.0, &more, OVERLAY1, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(sync: bool) -> Vec<Step> {
        std::iter::successors(Some(Step::Welcome), |s| s.next(sync)).collect()
    }

    #[test]
    fn pairing_is_only_asked_about_when_sync_is_there() {
        assert_eq!(walk(true), STEPS);
        assert!(!walk(false).contains(&Step::Sync));
        assert_eq!(Step::Done.back(false), Some(Step::Clips));
        assert_eq!(Step::Clips.progress(false), (4, 4));
        assert_eq!(Step::Clips.progress(true), (4, 5));
    }

    #[test]
    fn back_undoes_next() {
        for sync in [true, false] {
            for step in walk(sync) {
                if let Some(next) = step.next(sync) {
                    assert_eq!(next.back(sync), Some(step));
                }
            }
        }
        // Sync going away while its step is up still leaves a way out.
        assert_eq!(Step::Sync.next(false), Some(Step::Done));
        assert_eq!(Step::Sync.back(false), Some(Step::Clips));
    }
}
