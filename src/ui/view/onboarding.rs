//! The first-run setup: one choice a step, with the defaults already picked,
//! so pressing next all the way through ends up somewhere good. Shown once,
//! before the record button ever is.

use super::super::settings::CLIP_LENGTHS;
use super::widgets::{button, card, label, segmented, toggle_row};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Shape,
    Quality,
    Audio,
    Clips,
    Sync,
}

const STEPS: [Step; 5] = [Step::Shape, Step::Quality, Step::Audio, Step::Clips, Step::Sync];

impl Step {
    /// Pairing only gets asked about where there's a sync service to pair with.
    fn shown(self, sync: bool) -> bool {
        self != Step::Sync || sync
    }

    pub fn next(self, sync: bool) -> Option<Step> {
        STEPS.iter().copied().skip_while(|&s| s != self).skip(1).find(|s| s.shown(sync))
    }

    pub fn back(self, sync: bool) -> Option<Step> {
        STEPS.iter().rev().copied().skip_while(|&s| s != self).skip(1).find(|s| s.shown(sync))
    }

    /// Which step this is and out of how many.
    pub fn progress(self, sync: bool) -> (usize, usize) {
        let shown = || STEPS.iter().filter(|s| s.shown(sync));
        (shown().take_while(|&&s| s != self).count() + 1, shown().count())
    }
}

const CARD_H: f32 = 490.0;
const INSET: f32 = 40.0;
const NAV_Y: f32 = CONTENT_Y + CARD_H + 20.0;
const NAV_W: f32 = 200.0;
const NAV_H: f32 = 60.0;
/// Where a step's controls start, under its heading.
const CONTROLS_Y: f32 = 118.0;
const PICK_H: f32 = 68.0;
const TILE_H: f32 = 168.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, step: Step, hits: &mut Vec<Hit>) {
    let pane = Rect::new(PAD, CONTENT_Y, WIDTH as f32 - 2.0 * PAD, CARD_H);
    card(c, pane);
    let inner = Rect::new(pane.x + INSET, pane.y + 28.0, pane.w - 2.0 * INSET, pane.h - 50.0);
    match step {
        Step::Shape => shape(c, f, m, inner, hits),
        Step::Quality => quality(c, f, m, inner, hits),
        Step::Audio => audio(c, f, m, inner, hits),
        Step::Clips => clips(c, f, m, inner, hits),
        Step::Sync => pair(c, f, m, inner, hits),
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
    match step.next(sync) {
        Some(next) => button(c, f, m, hits, r, "Next", Action::Step(next), true),
        None => button(c, f, m, hits, r, "Finish", Action::FinishSetup, true),
    }

    let (at, of) = step.progress(sync);
    let (cx, cy) = (WIDTH as f32 / 2.0, NAV_Y + NAV_H / 2.0);
    let gap = 28.0;
    let x0 = cx - gap * (of as f32 - 1.0) / 2.0;
    let y = cy - 12.0;
    for i in 1..=of {
        let x = x0 + (i - 1) as f32 * gap;
        if i == at {
            c.fill_circle(x, y, 8.0, MAUVE, 1.0);
        } else {
            c.fill_circle(x, y, 6.0, if i < at { MAUVE } else { SURFACE1 }, if i < at { 0.5 } else { 1.0 });
        }
    }
    c.text_centered(f, Face::Data, 18.0, cx, cy + 24.0, &format!("{at} of {of}"), SUBTEXT0, 1.0);
}

/// The step's name, and a value for the current pick when there is one.
fn question(c: &mut Canvas, f: &mut Fonts, inner: Rect, heading: &str, meaning: &str) {
    c.text(f, Face::Heading, 32.0, inner.x, inner.y + 40.0, heading, TEXT, 1.0);
    c.text(f, Face::Body, 19.0, inner.x, inner.y + 78.0, meaning, SUBTEXT0, 1.0);
}

fn shape(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let meaning = match s.shape {
        Shape::Wide => "1920 × 1080",
        Shape::Square => "1440 × 1440",
        Shape::Tall => "1080 × 1920",
        Shape::BothEyes => "",
    };
    question(c, f, inner, "Shape", meaning);
    let shapes = [
        (Shape::Wide, "16:9", "Widescreen"),
        (Shape::Square, "1:1", "Square"),
        (Shape::Tall, "9:16", "Vertical"),
        (Shape::BothEyes, "Both eyes", ""),
    ];
    let y = inner.y + CONTROLS_Y;
    let w = (inner.w - 3.0 * 16.0) / 4.0;
    for (i, (shape, name, sub)) in shapes.into_iter().enumerate() {
        tile(c, f, m, hits, Rect::new(inner.x + i as f32 * (w + 16.0), y, w, TILE_H), shape, name, sub);
    }

    // With both eyes in the video there's no eye to pick.
    if s.shape == Shape::BothEyes {
        return;
    }
    label(c, f, inner.x, y + TILE_H + 40.0, "EYE");
    let eyes = [
        ("Left eye", Action::Eye(Eye::Left), s.eye == Eye::Left),
        ("Right eye", Action::Eye(Eye::Right), s.eye == Eye::Right),
        ("Both eyes blended", Action::Eye(Eye::Both), s.eye == Eye::Both),
    ];
    segmented(c, f, m, hits, Rect::new(inner.x, y + TILE_H + 56.0, 640.0, 52.0), &eyes, false);
}

/// One shape to pick: a little outline of it, its ratio and a word on it.
#[allow(clippy::too_many_arguments)]
fn tile(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, r: Rect, shape: Shape, name: &str, sub: &str) {
    let action = Action::Shape(shape);
    let selected = m.settings.shape == shape;
    let hovered = m.over(r);
    if selected {
        c.fill_rrect(r, 18.0, MAUVE, 0.12);
        c.stroke_rrect(r, 18.0, 2.0, MAUVE, 1.0);
    } else {
        c.fill_rrect(r, 18.0, if hovered { MAUVE } else { SURFACE0 }, if hovered { 0.08 } else { 0.40 });
        c.stroke_rrect(r, 18.0, 2.0, if hovered { MAUVE } else { SURFACE1 }, 0.6);
    }
    let (cx, _) = r.center();
    let color: Rgb = if selected { MAUVE } else { OVERLAY2 };
    let outline = |c: &mut Canvas, x: f32, w: f32| {
        let o = Rect::new(x, r.y + 26.0, w, 54.0);
        if selected {
            c.fill_rrect(o, 6.0, MAUVE, 0.2);
        }
        c.stroke_rrect(o, 6.0, 2.5, color, 1.0);
    };
    match shape {
        Shape::Wide => outline(c, cx - 48.0, 96.0),
        Shape::Square => outline(c, cx - 27.0, 54.0),
        Shape::Tall => outline(c, cx - 16.0, 32.0),
        Shape::BothEyes => {
            outline(c, cx - 47.0, 44.0);
            outline(c, cx + 3.0, 44.0);
        }
    }
    c.text_centered(f, Face::Data, 22.0, cx, r.y + 116.0, name, if selected { MAUVE } else { TEXT }, 1.0);
    c.text_centered(f, Face::Body, 14.0, cx, r.y + 140.0, sub, SUBTEXT0, 1.0);
    if !selected {
        hits.push(Hit { rect: r, action });
    }
}

fn quality(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    // Megabits a second, times 60 s, over 8 bits a byte.
    let meaning = format!("{} Mbit/s · about {} MB a minute", s.quality.mbps(), s.quality.mbps() * 60 / 8);
    question(c, f, inner, "Quality", &meaning);
    let qualities = [
        ("Standard", Action::Quality(Quality::Standard), s.quality == Quality::Standard),
        ("High", Action::Quality(Quality::High), s.quality == Quality::High),
        ("Max", Action::Quality(Quality::Max), s.quality == Quality::Max),
    ];
    segmented(c, f, m, hits, Rect::new(inner.x, inner.y + CONTROLS_Y, inner.w, PICK_H), &qualities, false);
}

fn audio(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    question(c, f, inner, "Audio", "");
    let at = |i: f32| Rect::new(inner.x, inner.y + CONTROLS_Y + i * 108.0, inner.w, 92.0);
    toggle_row(c, f, m, hits, at(0.0), "Game audio", Action::GameAudio, s.game_audio, false);
    toggle_row(c, f, m, hits, at(1.0), "Microphone", Action::Mic, s.mic, false);
}

fn clips(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let meaning = if s.clips { format!("Hold the left thumbstick to clip the last {}", length(s.clip_secs)) } else { String::new() };
    question(c, f, inner, "Clips", &meaning);
    let y = inner.y + CONTROLS_Y;
    toggle_row(c, f, m, hits, Rect::new(inner.x, y, inner.w, 92.0), "Clips", Action::Clips, s.clips, false);

    label(c, f, inner.x, y + 92.0 + 40.0, "LENGTH");
    let lengths: Vec<(String, Action, bool)> = CLIP_LENGTHS.iter().map(|&n| (length(n), Action::ClipLength(n), s.clip_secs == n)).collect();
    let options: Vec<(&str, Action, bool)> = lengths.iter().map(|(t, a, on)| (t.as_str(), *a, *on)).collect();
    segmented(c, f, m, hits, Rect::new(inner.x, y + 92.0 + 56.0, inner.w, PICK_H), &options, !s.clips);
}

/// Pairing is the one step that's fine to leave for later.
fn pair(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    if !m.sync.available {
        c.text(f, Face::BodyMedium, 20.0, inner.x, inner.y + 26.0, SYNC_MISSING, RED, 1.0);
        c.text(f, Face::Body, 17.0, inner.x, inner.y + 58.0, &SYNC_FIX.join(" "), SUBTEXT0, 1.0);
        return;
    }
    // The code looks the same as in settings.
    if let Some(pairing) = m.sync.pairing {
        return sync::pair(c, f, m, inner, pairing, hits);
    }
    question(c, f, inner, "Sync", "");
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
        let name = f.fit(Face::BodyMedium, 19.0, &d.name, inner.x + inner.w - x - 24.0);
        c.text(f, Face::BodyMedium, 19.0, x + 24.0, line, &name, TEXT, 1.0);
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
        std::iter::successors(Some(Step::Shape), |s| s.next(sync)).collect()
    }

    #[test]
    fn pairing_is_only_asked_about_when_sync_is_there() {
        assert_eq!(walk(true), STEPS);
        assert!(!walk(false).contains(&Step::Sync));
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
        assert_eq!(Step::Sync.next(false), None);
        assert_eq!(Step::Sync.back(false), Some(Step::Clips));
    }
}
