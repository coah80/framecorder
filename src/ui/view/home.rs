//! The home screen: the record button and clips on the left, and what's set
//! up right now as four tiles on the right. Each tile opens its settings.

use super::widgets::{card, label, toggle};
use super::*;

const LEFT_W: f32 = 500.0;
const RIGHT_X: f32 = PAD + LEFT_W + 32.0;
const RIGHT_W: f32 = WIDTH as f32 - RIGHT_X - PAD;
const GAP: f32 = 20.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    record_card(c, f, m, hits);

    let s = m.settings;
    let (w, h) = ((RIGHT_W - GAP) / 2.0, (CONTENT_H - GAP) / 2.0);
    let at = |col: f32, row: f32| Rect::new(RIGHT_X + col * (w + GAP), CONTENT_Y + row * (h + GAP), w, h);

    let shape = match s.shape {
        Shape::Wide => "16:9",
        Shape::Square => "1:1",
        Shape::Tall => "9:16",
        Shape::BothEyes => "Both eyes",
    };
    let view = match (s.shape, s.eye) {
        _ if !m.unlocked => "16:9 · SteamVR's view".to_string(),
        (Shape::BothEyes, _) => shape.to_string(),
        (_, Eye::Left) => format!("{shape} · Left eye"),
        (_, Eye::Right) => format!("{shape} · Right eye"),
    };
    tile(c, f, m, hits, at(0.0, 0.0), Section::Video, &view);

    let audio = match (s.game_audio, s.mic) {
        (true, true) => "Game + mic",
        (true, false) => "Game only",
        (false, true) => "Mic only",
        (false, false) => "No audio",
    };
    tile(c, f, m, hits, at(1.0, 0.0), Section::Audio, audio);

    let clips = s.clipping().map_or("Off".to_string(), |secs| format!("Last {}", length(secs)));
    tile(c, f, m, hits, at(0.0, 1.0), Section::Clips, &clips);

    let sync = match (m.sync.available, m.sync.devices.len()) {
        (false, _) => "Not set up".to_string(),
        (true, 0) => "Not paired".to_string(),
        (true, 1) => "1 device".to_string(),
        (true, n) => format!("{n} devices"),
    };
    tile(c, f, m, hits, at(1.0, 1.0), Section::Sync, &sync);
}

/// One thing that's set up: its name and the setting in a few words.
fn tile(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, r: Rect, section: Section, value: &str) {
    let action = Action::Open(section);
    let hovered = m.hover == Some(action);
    c.fill_rrect(r, 24.0, if hovered { MAUVE } else { SURFACE0 }, if hovered { 0.08 } else { 0.40 });
    c.stroke_rrect(r, 24.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, if hovered { 0.6 } else { 0.50 });

    let name = match section {
        Section::Video => "VIDEO",
        Section::Audio => "AUDIO",
        Section::Clips => "CLIPS",
        Section::Sync => "SYNC",
    };
    let x = r.x + 26.0;
    label(c, f, x, r.y + 44.0, name);
    c.text(f, Face::Heading, 30.0, x, r.y + r.h / 2.0 + 10.0, value, TEXT, 1.0);
    c.text(f, Face::BodyMedium, 16.0, x, r.y + r.h - 26.0, "Change", if hovered { MAUVE } else { OVERLAY1 }, 1.0);
    hits.push(Hit { rect: r, action });
}

fn record_card(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    let r = Rect::new(PAD, CONTENT_Y, LEFT_W, CONTENT_H);
    card(c, r);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + 160.0);
    let hovered = m.hover == Some(Action::Record);
    let recording = m.locked();

    let button = Rect::new(cx - 96.0, cy - 96.0, 192.0, 192.0);
    let accent = if recording { RED } else { MAUVE };
    if hovered {
        c.fill_circle(cx, cy, 104.0, accent, 0.08);
    }
    c.ring(cx, cy, 92.0, 4.0, accent, if hovered { 1.0 } else { 0.7 });
    if recording {
        let s = 64.0;
        c.fill_rrect(Rect::new(cx - s / 2.0, cy - s / 2.0, s, s), 12.0, RED, 1.0);
    } else {
        c.fill_circle(cx, cy, 72.0, MAUVE, if hovered { 1.0 } else { 0.92 });
    }
    if !matches!(m.status, Status::Stopping) {
        hits.push(Hit { rect: button, action: Action::Record });
    }

    let text = match m.status {
        Status::Idle => "Start recording",
        Status::Stopping => "Saving…",
        Status::Recording { .. } => "Stop recording",
    };
    c.text_centered(f, Face::Heading, 24.0, cx, cy + 142.0, text, TEXT, 1.0);

    let (time, time_alpha) = match m.status {
        Status::Recording { recorded, .. } => (clock(recorded), 1.0),
        _ => ("00:00:00".to_string(), 0.35),
    };
    c.text_centered(f, Face::Data, 52.0, cx, cy + 212.0, &time, TEXT, time_alpha);

    // Clips: the switch, a button to clip right now, and what's going on.
    let s = m.settings;
    let row_y = cy + 238.0;
    let w = (r.w - 56.0 - 16.0) / 2.0;
    toggle(c, f, m, hits, Rect::new(r.x + 28.0, row_y, w, 50.0), "Clips", Action::Clips, s.clips, recording);

    let pill = Rect::new(r.x + 28.0 + w + 16.0, row_y, w, 50.0);
    let can_clip = s.clips && !recording && !m.clip_cooling;
    let hovered = can_clip && m.hover == Some(Action::ClipNow);
    let alpha = if can_clip { 1.0 } else { 0.35 };
    c.fill_rrect(pill, 14.0, MAUVE, if hovered { 0.22 } else { 0.12 } * alpha);
    c.stroke_rrect(pill, 14.0, 1.5, MAUVE, if hovered { 0.9 } else { 0.5 } * alpha);
    let (px, py) = pill.center();
    c.text_centered(f, Face::BodyMedium, 18.0, px, py + 6.5, "Clip now", MAUVE, alpha);
    if can_clip {
        hits.push(Hit { rect: pill, action: Action::ClipNow });
    }

    let (status, color) = match (s.clips, recording) {
        (false, _) => ("Clips are off".to_string(), OVERLAY1),
        (true, true) => ("Clips are off while recording".to_string(), OVERLAY1),
        (true, false) => (format!("Hold the left thumbstick to clip the last {}", length(s.clip_secs)), SUBTEXT0),
    };
    c.text_centered(f, Face::Body, 16.0, cx, cy + 318.0, &status, color, 1.0);
}
