//! The handful of controls everything is built from. All sized for pointing
//! at from across the room: nothing under 48 px tall.

use super::*;

pub fn card(c: &mut Canvas, r: Rect) {
    c.fill_rrect(r, 24.0, SURFACE0, 0.40);
    c.stroke_rrect(r, 24.0, 1.5, SURFACE1, 0.50);
}

/// A button: mauve filled when it's the main thing to do, outlined otherwise.
#[allow(clippy::too_many_arguments)]
pub fn button(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, r: Rect, label: &str, action: Action, primary: bool) {
    let hovered = m.hover == Some(action);
    let (cx, cy) = r.center();
    if primary {
        c.fill_rrect(r, 14.0, MAUVE, if hovered { 1.0 } else { 0.9 });
        c.text_centered(f, Face::Heading, 18.0, cx, cy + 6.5, label, CRUST, 1.0);
    } else {
        c.fill_rrect(r, 14.0, if hovered { MAUVE } else { SURFACE0 }, if hovered { 0.08 } else { 0.40 });
        c.stroke_rrect(r, 14.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, 0.6);
        c.text_centered(f, Face::BodyMedium, 18.0, cx, cy + 6.5, label, if hovered { MAUVE } else { TEXT }, 1.0);
    }
    hits.push(Hit { rect: r, action });
}

/// A row of options where one is picked.
pub fn segmented(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, track: Rect, options: &[(&str, Action, bool)], locked: bool) {
    let alpha = if locked { 0.4 } else { 1.0 };
    c.fill_rrect(track, 14.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(track, 14.0, 1.5, SURFACE1, 0.50 * alpha);

    let w = (track.w - 8.0) / options.len() as f32;
    for (i, &(text, action, selected)) in options.iter().enumerate() {
        let seg = Rect::new(track.x + 4.0 + i as f32 * w, track.y + 4.0, w, track.h - 8.0);
        let hovered = !locked && m.hover == Some(action);
        if selected {
            c.fill_rrect(seg, 11.0, MAUVE, alpha);
        } else if hovered {
            c.fill_rrect(seg, 11.0, MAUVE, 0.08);
        }
        let color = if selected { CRUST } else if hovered { MAUVE } else { SUBTEXT0 };
        let (cx, cy) = seg.center();
        c.text_centered(f, Face::BodyMedium, 18.0, cx, cy + 6.5, text, color, alpha);
        if !locked {
            hits.push(Hit { rect: seg, action });
        }
    }
}

/// A setting that's one of a few options: its name, what the current pick
/// means, and the options underneath.
#[allow(clippy::too_many_arguments)]
pub fn choice(
    c: &mut Canvas,
    f: &mut Fonts,
    m: &Model,
    hits: &mut Vec<Hit>,
    r: Rect,
    name: &str,
    meaning: &str,
    options: &[(&str, Action, bool)],
    locked: bool,
) {
    let alpha = if locked { 0.4 } else { 1.0 };
    c.text(f, Face::BodyMedium, 19.0, r.x, r.y + 22.0, name, TEXT, alpha);
    let w = f.measure(Face::Body, 16.0, meaning);
    c.text(f, Face::Body, 16.0, r.x + r.w - w, r.y + 22.0, meaning, OVERLAY1, alpha);
    segmented(c, f, m, hits, Rect::new(r.x, r.y + 36.0, r.w, 52.0), options, locked);
}

/// 56x30 switch: the 44x24 one scaled up a bit for VR.
fn switch(c: &mut Canvas, r: Rect, on: bool, alpha: f32) {
    let (sw, sh) = (56.0, 30.0);
    let track = Rect::new(r.x + r.w - sw - 18.0, r.y + (r.h - sh) / 2.0, sw, sh);
    c.fill_rrect(track, sh / 2.0, if on { MAUVE } else { SURFACE1 }, alpha);
    let knob_x = if on { track.x + track.w - sh / 2.0 } else { track.x + sh / 2.0 };
    c.fill_circle(knob_x, track.y + sh / 2.0, 11.0, if on { CRUST } else { TEXT }, alpha);
}

/// A compact on/off control with just its name.
#[allow(clippy::too_many_arguments)]
pub fn toggle(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, r: Rect, name: &str, action: Action, on: bool, locked: bool) {
    let alpha = if locked { 0.4 } else { 1.0 };
    let hovered = !locked && m.hover == Some(action);
    c.fill_rrect(r, 14.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(r, 14.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, if hovered { 0.6 } else { 0.5 } * alpha);
    c.text(f, Face::BodyMedium, 18.0, r.x + 18.0, r.y + r.h / 2.0 + 6.5, name, if on { TEXT } else { SUBTEXT0 }, alpha);
    switch(c, r, on, alpha);
    if !locked {
        hits.push(Hit { rect: r, action });
    }
}

/// An on/off setting with a line saying what it does.
#[allow(clippy::too_many_arguments)]
pub fn toggle_row(
    c: &mut Canvas,
    f: &mut Fonts,
    m: &Model,
    hits: &mut Vec<Hit>,
    r: Rect,
    name: &str,
    meaning: &str,
    action: Action,
    on: bool,
    locked: bool,
) {
    let alpha = if locked { 0.4 } else { 1.0 };
    let hovered = !locked && m.hover == Some(action);
    c.fill_rrect(r, 16.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(r, 16.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, if hovered { 0.6 } else { 0.5 } * alpha);
    c.text(f, Face::BodyMedium, 20.0, r.x + 24.0, r.y + r.h / 2.0 - 4.0, name, TEXT, alpha);
    c.text(f, Face::Body, 16.0, r.x + 24.0, r.y + r.h / 2.0 + 22.0, meaning, OVERLAY1, alpha);
    switch(c, r, on, alpha);
    if !locked {
        hits.push(Hit { rect: r, action });
    }
}

/// A small uppercase heading over a group of things.
pub fn label(c: &mut Canvas, f: &mut Fonts, x: f32, y: f32, text: &str) {
    c.text(f, Face::Label, 14.0, x, y, text, OVERLAY2, 1.0);
}
