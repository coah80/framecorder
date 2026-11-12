//! The settings screen: the sections as pills along the top, and one
//! section's controls underneath. Every control says what the current
//! choice means.

use super::super::settings::CLIP_LENGTHS;
use super::widgets::{card, choice, dropdown, dropdown_field, dropdown_list, label, toggle_row};
use super::*;

const PILLS_H: f32 = 50.0;
const PANE_Y: f32 = CONTENT_Y + PILLS_H + 18.0;
const PANE_H: f32 = CONTENT_Y + CONTENT_H - PANE_Y;
const INSET: f32 = 32.0;
const ROW_H: f32 = 114.0;

const PANE: Rect = Rect { x: PAD, y: PANE_Y, w: WIDTH as f32 - 2.0 * PAD, h: PANE_H };
/// Where a section's controls go inside its card.
const INNER: Rect = Rect { x: PANE.x + INSET, y: PANE.y + 6.0, w: PANE.w - 2.0 * INSET, h: PANE.h - 12.0 };

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, section: Section, hits: &mut Vec<Hit>) {
    pills(c, f, m, section, hits);
    if section == Section::Sync {
        return sync::draw(c, f, m, PANE, hits);
    }
    card(c, PANE);
    let inner = INNER;
    match section {
        Section::Video => video(c, f, m, inner, hits),
        Section::Audio => audio(c, f, m, inner, hits),
        Section::Clips => clips(c, f, m, inner, hits),
        Section::General => general(c, f, m, inner, hits),
        Section::Sync => unreachable!(),
    }
}

fn pills(c: &mut Canvas, f: &mut Fonts, m: &Model, current: Section, hits: &mut Vec<Hit>) {
    let sections = [(Section::Video, "Video"), (Section::Audio, "Audio"), (Section::Clips, "Clips"), (Section::Sync, "Sync"), (Section::General, "General")];
    let mut x = PAD;
    for (section, name) in sections {
        let w = f.measure(Face::BodyMedium, 18.0, name) + 52.0;
        let r = Rect::new(x, CONTENT_Y, w, PILLS_H);
        let action = Action::Open(section);
        let selected = section == current;
        let hovered = m.over(r);
        if selected {
            c.fill_rrect(r, 14.0, MAUVE, 0.12);
            c.stroke_rrect(r, 14.0, 1.5, MAUVE, 0.5);
        } else if hovered {
            c.fill_rrect(r, 14.0, MAUVE, 0.08);
        }
        let color = if selected || hovered { MAUVE } else { SUBTEXT0 };
        let (cx, cy) = r.center();
        c.text_centered(f, Face::BodyMedium, 18.0, cx, cy + 6.5, name, color, 1.0);
        if !selected {
            hits.push(Hit { rect: r, action });
        }
        x += w + 8.0;
    }
}

/// The rows of a section, a hairline between each.
fn rows<const N: usize>(c: &mut Canvas, inner: Rect) -> [Rect; N] {
    let rows = std::array::from_fn(|i| Rect::new(inner.x, inner.y + i as f32 * ROW_H, inner.w, ROW_H));
    for r in &rows[..N - 1] {
        c.hline(r.x, r.y + r.h - 1.0, r.w, TEXT, 0.05);
    }
    rows
}

fn video(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();

    let [shape_row, eye_row, quality_row, fps_row] = rows(c, inner);
    // Without the panels it's SteamVR's view, whatever was picked.
    let (shape, eye) = if m.unlocked { (s.shape, s.eye) } else { (Shape::Wide, Eye::Left) };
    let shapes = [
        ("16:9", Action::Shape(Shape::Wide), shape == Shape::Wide),
        ("1:1", Action::Shape(Shape::Square), shape == Shape::Square),
        ("9:16", Action::Shape(Shape::Tall), shape == Shape::Tall),
        ("Both eyes", Action::Shape(Shape::BothEyes), shape == Shape::BothEyes),
    ];
    let meaning = match shape {
        Shape::Wide => "1920 × 1080",
        Shape::Square => "1440 × 1440",
        Shape::Tall => "1080 × 1920",
        Shape::BothEyes => "",
    };
    choice(c, f, m, hits, shape_row, "Shape", meaning, &shapes, locked || !m.unlocked);

    let eyes = [
        ("Left eye", Action::Eye(Eye::Left), eye == Eye::Left),
        ("Right eye", Action::Eye(Eye::Right), eye == Eye::Right),
        ("Both eyes blended", Action::Eye(Eye::Both), eye == Eye::Both),
    ];
    // SteamVR's view is always the left eye, and both-eyes shape already has both.
    let fixed = !m.unlocked || shape == Shape::BothEyes;
    choice(c, f, m, hits, eye_row, "Eye", "", &eyes, locked || fixed);

    let qualities = [
        ("Standard", Action::Quality(Quality::Standard), s.quality == Quality::Standard),
        ("High", Action::Quality(Quality::High), s.quality == Quality::High),
        ("Max", Action::Quality(Quality::Max), s.quality == Quality::Max),
    ];
    // Megabits a second, times 60 s, over 8 bits a byte.
    let meaning = format!("{} Mbit/s · about {} MB a minute", s.quality.mbps(), s.quality.mbps() * 60 / 8);
    choice(c, f, m, hits, quality_row, "Quality", &meaning, &qualities, locked);

    let rates = [
        ("Match display", Action::Fps(FrameRate::Auto), s.fps == FrameRate::Auto),
        ("60 fps", Action::Fps(FrameRate::Sixty), s.fps == FrameRate::Sixty),
        ("30 fps", Action::Fps(FrameRate::Thirty), s.fps == FrameRate::Thirty),
    ];
    choice(c, f, m, hits, fps_row, "Frame rate", "", &rates, locked);
}

fn audio(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();
    let at = |i: f32| Rect::new(inner.x, inner.y + 22.0 + i * 108.0, inner.w, 92.0);
    toggle_row(c, f, m, hits, at(0.0), "Game audio", Action::GameAudio, s.game_audio, locked);
    toggle_row(c, f, m, hits, at(1.0), "Microphone", Action::Mic, s.mic, locked);
    let mut row = 2.0;
    if let Some((options, picked)) = mic_options(m) {
        let picker = mic_row(inner);
        let value = picked.map_or("Not connected", |i| options[i].as_str());
        let value = fit(f, value, picker.w - 90.0);
        dropdown(c, f, m, hits, picker, "Mic", &value, Action::MicMenu, m.mic_menu, locked || !s.mic);
        row += 1.0;
    }
    // splitting needs something to split
    let both = s.game_audio && s.mic;
    // a picker's 6 px taller than a switch's row, keep the gap even under it
    let split = at(row);
    let split = if row > 2.0 { Rect::new(split.x, split.y + 6.0, split.w, split.h) } else { split };
    toggle_row(c, f, m, hits, split, "Split tracks", Action::SplitAudio, both && s.split_audio, locked || !both);
}

fn mic_row(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y + 22.0 + 2.0 * 108.0, inner.w, 98.0)
}

/// The list of mics, open over the audio settings.
pub fn mic_menu(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    let Some((options, picked)) = mic_options(m).filter(|_| m.mic_menu && m.settings.mic && !m.locked()) else { return };
    let field = dropdown_field(mic_row(INNER));
    let labels: Vec<String> = options.iter().map(|label| fit(f, label, field.w - 90.0)).collect();
    let rows: Vec<(&str, Action, bool)> = labels.iter().enumerate().map(|(i, label)| (label.as_str(), Action::MicSource(i), picked == Some(i))).collect();
    dropdown_list(c, f, m, hits, field, &rows, Action::MicMenu);
}

/// The mics to pick from and which one's in use, once there's a choice:
/// another mic plugged in, or the picked one gone (then none is, and the
/// default's recorded meanwhile).
fn mic_options(m: &Model) -> Option<(Vec<String>, Option<usize>)> {
    let picked = &m.settings.mic_source;
    let gone = !picked.is_empty() && !m.mics.iter().any(|mic| &mic.name == picked);
    if m.mics.len() < 2 && !gone {
        return None;
    }
    let on = m.mics.iter().position(|mic| if picked.is_empty() { mic.default } else { &mic.name == picked });
    Some((m.mics.iter().map(|mic| mic.label.clone()).collect(), on))
}

/// `text` cut down with an ellipsis to fit `width` at the segment's size.
fn fit(f: &mut Fonts, text: &str, width: f32) -> String {
    if f.measure(Face::BodyMedium, 18.0, text) <= width {
        return text.to_string();
    }
    let mut cut: String = text.to_string();
    while !cut.is_empty() && f.measure(Face::BodyMedium, 18.0, &format!("{cut}…")) > width {
        cut.pop();
    }
    format!("{}…", cut.trim_end())
}

fn general(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    toggle_row(c, f, m, hits, Rect::new(inner.x, inner.y + 22.0, inner.w, 92.0), "Start with SteamVR", Action::Autostart, m.autostart, false);
    toggle_row(c, f, m, hits, Rect::new(inner.x, inner.y + 22.0 + 108.0, inner.w, 92.0), "Automatic updates", Action::AutoUpdate, m.auto_update, false);
}

fn clips(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();
    toggle_row(c, f, m, hits, Rect::new(inner.x, inner.y + 22.0, inner.w, 92.0), "Clips", Action::Clips, s.clips, locked);

    let lengths: Vec<(String, Action, bool)> = CLIP_LENGTHS.iter().map(|&n| (length(n), Action::ClipLength(n), s.clip_secs == n)).collect();
    let options: Vec<(&str, Action, bool)> = lengths.iter().map(|(t, a, on)| (t.as_str(), *a, *on)).collect();
    let r = Rect::new(inner.x, inner.y + 22.0 + 108.0, inner.w, ROW_H);
    choice(c, f, m, hits, r, "Length", "", &options, locked || !s.clips);

    let y = r.y + ROW_H + 32.0;
    label(c, f, inner.x, y, "HOW TO CLIP");
    let notes = [
        "Hold the left thumbstick down, or press Clip now on the main screen.",
        "Change the button in SteamVR's controller settings, under framecorder.",
    ];
    for (i, note) in notes.iter().enumerate() {
        c.text(f, Face::Body, 17.0, inner.x, y + 34.0 + i as f32 * 28.0, note, SUBTEXT0, 1.0);
    }
}
