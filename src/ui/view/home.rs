//! The home screen: the record button and clips on the left, and what's set
//! up right now as a list on the right, each row opening its settings, with
//! what's paired under it.

use super::widgets::{card, label, toggle};
use super::{device_line, free_space, presence_color, LOW_SPACE};
use super::*;

const RIGHT_W: f32 = 420.0;
const GAP: f32 = 22.0;
const LEFT_W: f32 = WIDTH as f32 - 2.0 * PAD - RIGHT_W - GAP;
const RIGHT_X: f32 = PAD + LEFT_W + GAP;
const ROW_H: f32 = 66.0;
/// Rows the list fits; with more than that, the last row says "and N more".
const SHOWN: usize = 4;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    record_card(c, f, m, hits);
    let rows = Rect::new(RIGHT_X, CONTENT_Y, RIGHT_W, 4.0 * ROW_H + 16.0);
    settings_rows(c, f, m, rows, hits);
    let y = rows.y + rows.h + 20.0;
    sync_card(c, f, m, Rect::new(RIGHT_X, y, RIGHT_W, CONTENT_Y + CONTENT_H - y), hits);
}

/// What the next recording will be, a row a setting. Each opens its section.
fn settings_rows(c: &mut Canvas, f: &mut Fonts, m: &Model, r: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();
    let alpha = if locked { 0.45 } else { 1.0 };
    c.fill_rrect(r, 24.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(r, 24.0, 1.5, SURFACE1, 0.50 * alpha);

    let shape = match s.shape {
        Shape::Wide => "16:9",
        Shape::Square => "1:1",
        Shape::Tall => "9:16",
        Shape::BothEyes => "Both eyes",
    };
    let video = match (s.shape, s.eye) {
        _ if !m.unlocked => "16:9 · SteamVR's view".to_string(),
        (Shape::BothEyes, _) => shape.to_string(),
        (_, Eye::Left) => format!("{shape} · Left eye"),
        (_, Eye::Right) => format!("{shape} · Right eye"),
    };
    let quality = match s.quality {
        Quality::Standard => "Standard",
        Quality::High => "High",
        Quality::Max => "Max",
    };
    let fps = match s.fps {
        FrameRate::Auto => "Match display",
        FrameRate::Sixty => "60 fps",
        FrameRate::Thirty => "30 fps",
    };
    let quality = format!("{quality} · {} Mbit/s · {fps}", s.quality.mbps());
    let audio = match (s.game_audio, s.mic) {
        (true, true) => "Game + mic",
        (true, false) => "Game only",
        (false, true) => "Mic only",
        (false, false) => "No audio",
    };
    let clips = s.clipping().map_or("Off".to_string(), |secs| format!("Last {}", length(secs)));
    let rows = [
        ("VIDEO", video.as_str(), Section::Video),
        ("QUALITY", quality.as_str(), Section::Video),
        ("AUDIO", audio, Section::Audio),
        ("CLIPS", clips.as_str(), Section::Clips),
    ];
    for (i, (name, value, section)) in rows.into_iter().enumerate() {
        let row = Rect::new(r.x + 10.0, r.y + 8.0 + i as f32 * ROW_H, r.w - 20.0, ROW_H);
        let action = Action::Open(section);
        let hovered = !locked && m.hover == Some(action);
        if hovered {
            c.fill_rrect(row, 14.0, MAUVE, 0.08);
        }
        if i + 1 < rows.len() {
            c.hline(row.x + 14.0, row.y + row.h - 1.0, row.w - 28.0, TEXT, 0.05);
        }
        label(c, f, row.x + 14.0, row.y + 25.0, name);
        c.text(f, Face::BodyMedium, 19.0, row.x + 14.0, row.y + 50.0, value, TEXT, alpha);
        let chevron = if hovered { MAUVE } else { OVERLAY1 };
        c.text(f, Face::Body, 30.0, row.x + row.w - 30.0, row.y + 43.0, "›", chevron, alpha);
        if !locked {
            hits.push(Hit { rect: row, action });
        }
    }
}

/// What's paired, and the way to pair more. The section has the rest.
fn sync_card(c: &mut Canvas, f: &mut Fonts, m: &Model, r: Rect, hits: &mut Vec<Hit>) {
    card(c, r);
    let x = r.x + 24.0;
    let list = m.sync.devices;
    let heading = match (m.sync.available, list.len()) {
        (false, _) => "SYNC · NOT SET UP".to_string(),
        (true, 0) => "SYNC · NOT PAIRED".to_string(),
        (true, 1) => "SYNC · 1 DEVICE".to_string(),
        (true, n) => format!("SYNC · {n} DEVICES"),
    };
    label(c, f, x, r.y + 36.0, &heading);

    let action = Action::Open(Section::Sync);
    let text = if m.sync.available { "Pair a device" } else { "Details" };
    let w = f.measure(Face::BodyMedium, 15.0, text);
    let link = Rect::new(r.x + r.w - 24.0 - w - 20.0, r.y + 14.0, w + 20.0, 44.0);
    let hovered = m.hover == Some(action);
    c.text(f, Face::BodyMedium, 15.0, link.x + 10.0, r.y + 36.0, text, MAUVE, if hovered { 1.0 } else { 0.8 });
    hits.push(Hit { rect: link, action });

    if list.is_empty() {
        let line = if m.sync.available { "Get the app on your phone or computer." } else { "Run the installer again to set it up." };
        c.text(f, Face::BodyMedium, 18.0, x, r.y + 80.0, "Nothing paired yet", TEXT, 1.0);
        c.text(f, Face::Body, 16.0, x, r.y + 108.0, line, SUBTEXT0, 1.0);
        return;
    }
    let shown = if list.len() > SHOWN { SHOWN - 1 } else { SHOWN };
    for (i, d) in list.iter().take(shown).enumerate() {
        let row = Rect::new(x, r.y + 56.0 + i as f32 * 52.0, r.w - 48.0, 44.0);
        let baseline = row.y + row.h / 2.0 + 6.0;
        c.fill_rrect(row, 12.0, BASE, 0.6);
        c.fill_circle(row.x + 18.5, row.y + row.h / 2.0, 4.5, presence_color(d), 1.0);
        // What the device is up to sits on the right, the name gets the rest.
        let mut name_w = row.w - 33.0 - 14.0;
        if let Some((line, color)) = device_line(f, d, row.w * 0.55) {
            let w = f.measure(Face::Body, 14.0, &line);
            c.text(f, Face::Body, 14.0, row.x + row.w - 14.0 - w, baseline, &line, color, 1.0);
            name_w -= w + 12.0;
        }
        let name = f.fit(Face::BodyMedium, 17.0, &d.name, name_w);
        c.text(f, Face::BodyMedium, 17.0, row.x + 33.0, baseline, &name, TEXT, 1.0);
        if let Some(t) = &d.transfer {
            let track = Rect::new(row.x + 33.0, row.y + row.h - 5.0, row.w - 47.0, 2.0);
            c.fill_rect(track, MAUVE, 0.2);
            c.fill_rect(Rect::new(track.x, track.y, track.w * t.percent() as f32 / 100.0, track.h), MAUVE, 0.9);
        }
    }
    if list.len() > shown {
        // The section has them all, a page at a time.
        let more = format!("and {} more", list.len() - shown);
        let row = Rect::new(x, r.y + 56.0 + shown as f32 * 52.0, r.w - 48.0, 44.0);
        let hovered = m.hover == Some(action);
        c.text(f, Face::Body, 15.0, row.x + 12.0, row.y + row.h / 2.0 + 5.5, &more, if hovered { MAUVE } else { OVERLAY1 }, 1.0);
        hits.push(Hit { rect: row, action });
    }
}

fn record_card(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    let r = Rect::new(PAD, CONTENT_Y, LEFT_W, CONTENT_H);
    let hovered = m.hover == Some(Action::Record);
    let recording = m.locked();
    let accent = if recording { RED } else { MAUVE };
    c.fill_rrect(r, 24.0, SURFACE0, 0.40);
    c.stroke_rrect(r, 24.0, 1.5, if recording { RED } else { SURFACE1 }, if recording { 0.35 } else { 0.50 });

    let (cx, cy) = (r.x + r.w / 2.0, r.y + 160.0);
    let button = Rect::new(cx - 132.0, cy - 132.0, 264.0, 264.0);
    c.fill_circle(cx, cy, 130.0, accent, if hovered { 0.14 } else { 0.08 });
    c.ring(cx, cy, 132.0, 5.0, accent, if hovered { 1.0 } else { 0.7 });
    if recording {
        let s = 92.0;
        c.fill_rrect(Rect::new(cx - s / 2.0, cy - s / 2.0, s, s), 18.0, RED, 1.0);
    } else {
        c.fill_circle(cx, cy, 100.0, MAUVE, if hovered { 1.0 } else { 0.92 });
    }
    if !matches!(m.status, Status::Stopping) {
        hits.push(Hit { rect: button, action: Action::Record });
    }

    let s = m.settings;
    let text = match m.status {
        Status::Idle => "Start recording",
        Status::Stopping => "Saving…",
        Status::Recording { .. } => "Stop recording",
    };
    c.text_centered(f, Face::Heading, 28.0, cx, cy + 176.0, text, TEXT, 1.0);

    let (time, time_alpha) = match m.status {
        Status::Recording { recorded, .. } => (clock(recorded), 1.0),
        _ => ("00:00:00".to_string(), 0.35),
    };
    c.text_centered(f, Face::Data, 64.0, cx, cy + 250.0, &time, TEXT, time_alpha);
    if let Some(free) = m.free {
        let room = free_space(free, s.quality.mbps());
        let color = if free < LOW_SPACE { YELLOW } else { OVERLAY2 };
        c.text_centered(f, Face::Body, 15.0, cx, cy + 284.0, &room, color, 1.0);
    }

    // Clips: the switch, a button to clip right now, and what's going on.
    let inset = 36.0;
    let row_y = r.y + r.h - 116.0;
    let w = (r.w - 2.0 * inset - 14.0) / 2.0;
    toggle(c, f, m, hits, Rect::new(r.x + inset, row_y, w, 52.0), "Clips", Action::Clips, s.clips, recording);

    let pill = Rect::new(r.x + inset + w + 14.0, row_y, w, 52.0);
    let can_clip = s.clips && !recording && !m.clip_cooling;
    let hovered = can_clip && m.hover == Some(Action::ClipNow);
    let alpha = if can_clip { 1.0 } else { 0.4 };
    c.fill_rrect(pill, 14.0, MAUVE, if hovered { 0.22 } else { 0.12 } * alpha);
    c.stroke_rrect(pill, 14.0, 1.5, MAUVE, if hovered { 0.9 } else { 0.5 } * alpha);
    let (px, py) = pill.center();
    c.text_centered(f, Face::BodyMedium, 18.0, px, py + 6.5, "Clip now", MAUVE, alpha);
    if can_clip {
        hits.push(Hit { rect: pill, action: Action::ClipNow });
    }

    let (status, color) = match (s.clips, recording) {
        (false, _) => ("Clips are off".to_string(), OVERLAY2),
        (true, true) => ("Clips are off while recording, the recording has it all".to_string(), OVERLAY2),
        (true, false) => (format!("Hold the left thumbstick to clip the last {}", length(s.clip_secs)), SUBTEXT0),
    };
    c.text_centered(f, Face::Body, 16.0, cx, r.y + r.h - 30.0, &status, color, 1.0);
}
