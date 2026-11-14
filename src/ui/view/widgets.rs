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
    let hovered = m.over(r);
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
        let hovered = !locked && m.over(seg);
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

/// A little status pill: a dot and a few words in one color. Returns its width.
pub fn chip(c: &mut Canvas, f: &mut Fonts, x: f32, cy: f32, text: &str, color: Rgb) -> f32 {
    let w = f.measure(Face::BodyMedium, 15.0, text) + 47.0;
    let r = Rect::new(x, cy - 17.0, w, 34.0);
    c.fill_rrect(r, 17.0, color, 0.08);
    c.stroke_rrect(r, 17.0, 1.0, color, 0.25);
    c.fill_circle(r.x + 18.5, cy, 4.5, color, 1.0);
    c.text(f, Face::BodyMedium, 15.0, r.x + 33.0, cy + 5.5, text, color, 1.0);
    w
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
    c.text(f, Face::BodyMedium, 19.0, r.x, r.y + 30.0, name, TEXT, alpha);
    let w = f.measure(Face::Body, 16.0, meaning);
    c.text(f, Face::Body, 16.0, r.x + r.w - w, r.y + 30.0, meaning, OVERLAY2, alpha);
    segmented(c, f, m, hits, Rect::new(r.x, r.y + 46.0, r.w, 52.0), options, locked);
}

/// A setting picked from a list: its name, and a field holding the current
/// pick that opens the list (`dropdown_list`). With nothing else to pick
/// (`action` is None) the field just shows the value.
#[allow(clippy::too_many_arguments)]
pub fn dropdown(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, r: Rect, name: &str, value: &str, action: Option<Action>, open: bool, locked: bool) {
    let alpha = if locked { 0.4 } else { 1.0 };
    c.text(f, Face::BodyMedium, 19.0, r.x, r.y + 30.0, name, TEXT, alpha);
    let field = dropdown_field(r);
    let cy = field.y + field.h / 2.0;
    c.fill_rrect(field, 14.0, SURFACE0, 0.40 * alpha);
    let Some(action) = action else {
        c.text(f, Face::BodyMedium, 18.0, field.x + 20.0, cy + 6.5, value, TEXT, alpha);
        return;
    };
    let lit = !locked && (open || m.over(field));
    c.stroke_rrect(field, 14.0, 1.5, if lit { MAUVE } else { SURFACE1 }, if lit { 0.6 } else { 0.5 } * alpha);
    c.text(f, Face::BodyMedium, 18.0, field.x + 20.0, cy + 6.5, value, TEXT, alpha);
    // a chevron, pointing up while the list is open
    let (x, s) = (field.x + field.w - 34.0, if open { -1.0 } else { 1.0 });
    let color = if lit { MAUVE } else { SUBTEXT0 };
    c.line(x - 7.0, cy - 3.5 * s, x, cy + 3.5 * s, 2.5, color, alpha);
    c.line(x, cy + 3.5 * s, x + 7.0, cy - 3.5 * s, 2.5, color, alpha);
    if !locked {
        hits.push(Hit { rect: field, action });
    }
}

/// Where a dropdown in row `r` draws its field.
pub fn dropdown_field(r: Rect) -> Rect {
    Rect::new(r.x, r.y + 46.0, r.w, 52.0)
}

/// The open list of a dropdown, drawn over everything else: under its field,
/// or above it when there's no room below. Its rows take a tap before
/// anything on screen, and a tap anywhere else is `close`.
pub fn dropdown_list(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>, field: Rect, options: &[(&str, Action, bool)], close: Action) {
    const ROW: f32 = 56.0;
    let h = options.len() as f32 * ROW + 8.0;
    let below = field.y + field.h + 8.0;
    let y = if below + h <= HEIGHT as f32 - 12.0 { below } else { field.y - 8.0 - h };
    let panel = Rect::new(field.x, y, field.w, h);
    c.fill_rrect(Rect::new(panel.x + 2.0, panel.y + 10.0, panel.w - 4.0, panel.h), 16.0, CRUST, 0.55);
    c.fill_rrect(panel, 16.0, BASE, 1.0);
    c.fill_rrect(panel, 16.0, SURFACE0, 0.70);
    c.stroke_rrect(panel, 16.0, 1.5, SURFACE1, 0.8);
    let mut front = Vec::with_capacity(options.len() + 1);
    for (i, &(text, action, selected)) in options.iter().enumerate() {
        let row = Rect::new(panel.x + 4.0, panel.y + 4.0 + i as f32 * ROW, panel.w - 8.0, ROW);
        let hovered = m.over(row);
        if selected || hovered {
            c.fill_rrect(row, 12.0, MAUVE, if selected { 0.14 } else { 0.08 });
        }
        let cy = row.y + row.h / 2.0;
        c.text(f, Face::BodyMedium, 18.0, row.x + 16.0, cy + 6.5, text, if selected || hovered { MAUVE } else { TEXT }, 1.0);
        if selected {
            let x = row.x + row.w - 38.0;
            c.line(x, cy, x + 6.0, cy + 6.0, 2.5, MAUVE, 1.0);
            c.line(x + 6.0, cy + 6.0, x + 18.0, cy - 6.0, 2.5, MAUVE, 1.0);
        }
        front.push(Hit { rect: row, action });
    }
    front.push(Hit { rect: Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32), action: close });
    hits.splice(0..0, front);
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
    let hovered = !locked && m.over(r);
    c.fill_rrect(r, 14.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(r, 14.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, if hovered { 0.6 } else { 0.5 } * alpha);
    c.text(f, Face::BodyMedium, 18.0, r.x + 18.0, r.y + r.h / 2.0 + 6.5, name, if on { TEXT } else { SUBTEXT0 }, alpha);
    switch(c, r, on, alpha);
    if !locked {
        hits.push(Hit { rect: r, action });
    }
}

/// A full width on/off setting.
#[allow(clippy::too_many_arguments)]
pub fn toggle_row(
    c: &mut Canvas,
    f: &mut Fonts,
    m: &Model,
    hits: &mut Vec<Hit>,
    r: Rect,
    name: &str,
    action: Action,
    on: bool,
    locked: bool,
) {
    toggle_row_noted(c, f, m, hits, r, name, "", action, on, locked);
}

/// A toggle row with a line under its name, for a switch whose name can't
/// say what it does.
#[allow(clippy::too_many_arguments)]
pub fn toggle_row_noted(
    c: &mut Canvas,
    f: &mut Fonts,
    m: &Model,
    hits: &mut Vec<Hit>,
    r: Rect,
    name: &str,
    note: &str,
    action: Action,
    on: bool,
    locked: bool,
) {
    let alpha = if locked { 0.4 } else { 1.0 };
    let hovered = !locked && m.over(r);
    c.fill_rrect(r, 16.0, SURFACE0, 0.40 * alpha);
    c.stroke_rrect(r, 16.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, if hovered { 0.6 } else { 0.5 } * alpha);
    let middle = r.y + r.h / 2.0;
    if note.is_empty() {
        c.text(f, Face::BodyMedium, 20.0, r.x + 24.0, middle + 7.0, name, TEXT, alpha);
    } else {
        c.text(f, Face::BodyMedium, 20.0, r.x + 24.0, middle - 6.0, name, TEXT, alpha);
        c.text(f, Face::Body, 16.0, r.x + 24.0, middle + 21.0, note, OVERLAY2, alpha);
    }
    switch(c, r, on, alpha);
    if !locked {
        hits.push(Hit { rect: r, action });
    }
}

/// A small uppercase heading over a group of things.
pub fn label(c: &mut Canvas, f: &mut Fonts, x: f32, y: f32, text: &str) {
    c.text(f, Face::Label, 14.0, x, y, text, OVERLAY2, 1.0);
}
