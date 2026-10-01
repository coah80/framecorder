//! The settings screen: sections down the left, one section's controls on
//! the right. Every control says what the current choice means.

use super::super::settings::CLIP_LENGTHS;
use super::widgets::{card, choice, label, toggle_row};
use super::*;

const SIDE_W: f32 = 232.0;
const PANE_X: f32 = PAD + SIDE_W + 28.0;
const PANE_W: f32 = WIDTH as f32 - PANE_X - PAD;
const INSET: f32 = 32.0;
const ROW_H: f32 = 112.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, section: Section, hits: &mut Vec<Hit>) {
    sidebar(c, f, m, section, hits);
    let pane = Rect::new(PANE_X, CONTENT_Y, PANE_W, CONTENT_H);
    card(c, pane);
    let inner = Rect::new(pane.x + INSET, pane.y + 28.0, pane.w - 2.0 * INSET, pane.h - 56.0);
    match section {
        Section::Video => video(c, f, m, inner, hits),
        Section::Audio => audio(c, f, m, inner, hits),
        Section::Clips => clips(c, f, m, inner, hits),
        Section::Sync => sync::draw(c, f, m, inner, hits),
    }
}

fn sidebar(c: &mut Canvas, f: &mut Fonts, m: &Model, current: Section, hits: &mut Vec<Hit>) {
    let sections = [(Section::Video, "Video"), (Section::Audio, "Audio"), (Section::Clips, "Clips"), (Section::Sync, "Sync")];
    for (i, (section, name)) in sections.into_iter().enumerate() {
        let r = Rect::new(PAD, CONTENT_Y + i as f32 * 76.0, SIDE_W, 64.0);
        let action = Action::Open(section);
        let selected = section == current;
        let hovered = m.hover == Some(action);
        if selected {
            c.fill_rrect(r, 16.0, MAUVE, 0.12);
            c.stroke_rrect(r, 16.0, 1.5, MAUVE, 0.5);
        } else if hovered {
            c.fill_rrect(r, 16.0, MAUVE, 0.08);
        }
        let color = if selected || hovered { MAUVE } else { SUBTEXT0 };
        c.text(f, Face::BodyMedium, 20.0, r.x + 24.0, r.y + r.h / 2.0 + 7.0, name, color, 1.0);
        if !selected {
            hits.push(Hit { rect: r, action });
        }
    }

    // Out of the way at the bottom, and it takes a second tap.
    let r = Rect::new(PAD, CONTENT_Y + CONTENT_H - 64.0, SIDE_W, 64.0);
    let hovered = m.hover == Some(Action::Uninstall);
    if m.uninstall_armed {
        c.fill_rrect(r, 16.0, RED, 0.12);
        c.stroke_rrect(r, 16.0, 1.5, RED, 0.5);
    } else if hovered {
        c.fill_rrect(r, 16.0, RED, 0.08);
    }
    let (text, color) = match (m.uninstall_armed, hovered) {
        (true, _) => ("Tap again to remove", RED),
        (false, true) => ("Remove framecorder", RED),
        (false, false) => ("Remove framecorder", OVERLAY2),
    };
    c.text(f, Face::Body, 18.0, r.x + 24.0, r.y + r.h / 2.0 + 7.0, text, color, 1.0);
    hits.push(Hit { rect: r, action: Action::Uninstall });
}

fn row(inner: Rect, i: usize) -> Rect {
    Rect::new(inner.x, inner.y + i as f32 * ROW_H, inner.w, ROW_H)
}

fn video(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();

    // Without the panels it's SteamVR's view, whatever was picked.
    let (shape, eye) = if m.unlocked { (s.shape, s.eye) } else { (Shape::Wide, Eye::Left) };
    let shapes = [
        ("16:9", Action::Shape(Shape::Wide), shape == Shape::Wide),
        ("1:1", Action::Shape(Shape::Square), shape == Shape::Square),
        ("9:16", Action::Shape(Shape::Tall), shape == Shape::Tall),
        ("Both eyes", Action::Shape(Shape::BothEyes), shape == Shape::BothEyes),
    ];
    let meaning = match s.shape {
        _ if !m.unlocked => "SteamVR's view, 1920 × 1080 · the others need the recorder unlocked",
        Shape::Wide => "Widescreen · 1920 × 1080",
        Shape::Square => "Square · 1440 × 1440",
        Shape::Tall => "Vertical · 1080 × 1920",
        Shape::BothEyes => "Both panels, just as the headset shows them",
    };
    choice(c, f, m, hits, row(inner, 0), "Shape", meaning, &shapes, locked || !m.unlocked);

    let eyes = [
        ("Left eye", Action::Eye(Eye::Left), eye == Eye::Left),
        ("Right eye", Action::Eye(Eye::Right), eye == Eye::Right),
    ];
    let (meaning, fixed) = match shape {
        _ if !m.unlocked => ("SteamVR's view is always the left eye", true),
        Shape::BothEyes => ("Both eyes are in the video", true),
        _ => ("The eye whose view gets recorded", false),
    };
    choice(c, f, m, hits, row(inner, 1), "Eye", meaning, &eyes, locked || fixed);

    let qualities = [
        ("Standard", Action::Quality(Quality::Standard), s.quality == Quality::Standard),
        ("High", Action::Quality(Quality::High), s.quality == Quality::High),
        ("Max", Action::Quality(Quality::Max), s.quality == Quality::Max),
    ];
    // Megabits a second, times 60 s, over 8 bits a byte.
    let meaning = format!("{} Mbit/s · about {} MB a minute", s.quality.mbps(), s.quality.mbps() * 60 / 8);
    choice(c, f, m, hits, row(inner, 2), "Quality", &meaning, &qualities, locked);

    let rates = [
        ("Match display", Action::Fps(FrameRate::Auto), s.fps == FrameRate::Auto),
        ("60 fps", Action::Fps(FrameRate::Sixty), s.fps == FrameRate::Sixty),
        ("30 fps", Action::Fps(FrameRate::Thirty), s.fps == FrameRate::Thirty),
    ];
    let meaning = match s.fps {
        FrameRate::Auto => "Every frame the headset shows · smoothest",
        FrameRate::Sixty => "Can stutter a little when the headset runs at 72",
        FrameRate::Thirty => "Smaller files, less smooth",
    };
    choice(c, f, m, hits, row(inner, 3), "Frame rate", meaning, &rates, locked);
}

fn audio(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();
    let at = |i: f32| Rect::new(inner.x, inner.y + i * 108.0, inner.w, 92.0);
    toggle_row(c, f, m, hits, at(0.0), "Game audio", "Everything you hear from the game", Action::GameAudio, s.game_audio, locked);
    let mic = if s.game_audio { "Your voice, mixed in with the game" } else { "Your voice" };
    toggle_row(c, f, m, hits, at(1.0), "Microphone", mic, Action::Mic, s.mic, locked);

    let y = inner.y + 2.0 * 108.0 + 36.0;
    label(c, f, inner.x, y, "GOOD TO KNOW");
    let notes = ["The headset mutes its microphone whenever it's off your head.", "Audio is saved as AAC, 192 kbit/s."];
    for (i, note) in notes.iter().enumerate() {
        c.text(f, Face::Body, 17.0, inner.x, y + 34.0 + i as f32 * 28.0, note, SUBTEXT0, 1.0);
    }
}

fn clips(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    let s = m.settings;
    let locked = m.locked();
    let meaning = if s.clips {
        format!("Keeps the last {} ready to save, the whole time you play", length(s.clip_secs))
    } else {
        "Nothing is kept, so there's nothing to clip".to_string()
    };
    toggle_row(c, f, m, hits, Rect::new(inner.x, inner.y, inner.w, 92.0), "Clips", &meaning, Action::Clips, s.clips, locked);

    let lengths: Vec<(String, Action, bool)> = CLIP_LENGTHS.iter().map(|&n| (length(n), Action::ClipLength(n), s.clip_secs == n)).collect();
    let options: Vec<(&str, Action, bool)> = lengths.iter().map(|(t, a, on)| (t.as_str(), *a, *on)).collect();
    let meaning = format!("A clip is the last {} you played", length(s.clip_secs));
    let r = Rect::new(inner.x, inner.y + 120.0, inner.w, ROW_H);
    choice(c, f, m, hits, r, "Length", &meaning, &options, locked || !s.clips);

    let y = inner.y + 120.0 + ROW_H + 32.0;
    label(c, f, inner.x, y, "HOW TO CLIP");
    let notes = [
        "Hold the left thumbstick down, or press Clip now on the main screen.",
        "Change the button in SteamVR's controller settings, under framecorder.",
        "Recording turns clips off until you stop, the recording has it all.",
    ];
    for (i, note) in notes.iter().enumerate() {
        c.text(f, Face::Body, 17.0, inner.x, y + 34.0 + i as f32 * 28.0, note, SUBTEXT0, 1.0);
    }
}
