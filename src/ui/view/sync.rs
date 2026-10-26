//! The sync section: pairing something new with a QR code or a six digit
//! code on the left, and what's paired on the right.

use super::widgets::{button, card, label};
use super::{device_line, presence_color};
use super::*;

/// Rows a page of the list holds.
pub const SHOWN: usize = 4;
const DEVICES_W: f32 = 360.0;
const INSET: f32 = 32.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, pane: Rect, hits: &mut Vec<Hit>) {
    let left = Rect::new(pane.x, pane.y, pane.w - DEVICES_W - 22.0, pane.h);
    card(c, left);
    let inner = Rect::new(left.x + INSET, left.y + 28.0, left.w - 2.0 * INSET, left.h - 56.0);
    if !m.sync.available {
        c.text(f, Face::BodyMedium, 20.0, inner.x, inner.y + 26.0, "The sync service isn't running on this headset", RED, 1.0);
        c.text(f, Face::Body, 17.0, inner.x, inner.y + 58.0, "Run the installer again to set it up.", SUBTEXT0, 1.0);
        return;
    }
    match m.sync.pairing {
        Some(pairing) => pair(c, f, m, inner, pairing, hits),
        None => {
            label(c, f, inner.x, inner.y + 16.0, "PAIR A DEVICE");
            c.text(f, Face::BodyMedium, 20.0, inner.x, inner.y + 62.0, "Get clips on your phone or computer", TEXT, 1.0);
            let lines = ["Get the framecorder app on the other device, then pair it here.", "Clips show up there on their own from then on."];
            for (i, line) in lines.iter().enumerate() {
                c.text(f, Face::Body, 17.0, inner.x, inner.y + 96.0 + i as f32 * 26.0, line, SUBTEXT0, 1.0);
            }
            let r = Rect::new(inner.x, inner.y + inner.h - BUTTON_H, 200.0, BUTTON_H);
            button(c, f, m, hits, r, "Pair a device", Action::Pair, true);
        }
    }
    devices(c, f, m, Rect::new(pane.x + pane.w - DEVICES_W, pane.y, DEVICES_W, pane.h), hits);
}

fn devices(c: &mut Canvas, f: &mut Fonts, m: &Model, r: Rect, hits: &mut Vec<Hit>) {
    card(c, r);
    let x = r.x + 20.0;
    label(c, f, x + 4.0, r.y + 38.0, "PAIRED DEVICES");
    let list = m.sync.devices;
    if list.is_empty() {
        c.text(f, Face::BodyMedium, 18.0, x + 4.0, r.y + 84.0, "Nothing paired yet", TEXT, 1.0);
    }
    let pages = list.len().div_ceil(SHOWN).max(1);
    let page = m.sync.page.min(pages - 1);
    let first = page * SHOWN;
    for (i, d) in list.iter().enumerate().skip(first).take(SHOWN) {
        let row = Rect::new(x, r.y + 56.0 + (i - first) as f32 * 70.0, r.w - 40.0, 58.0);
        c.fill_rrect(row, 14.0, BASE, 0.6);
        c.fill_circle(row.x + 20.5, row.y + row.h / 2.0, 4.5, presence_color(d), 1.0);

        let remove = Rect::new(row.x + row.w - 8.0 - 96.0, row.y + 8.0, 96.0, 42.0);
        let name_w = remove.x - 10.0 - (row.x + 35.0);
        let line = device_line(f, d, name_w);
        // The name sits in the middle on its own, or over the line about it.
        let name_y = if line.is_some() { row.y + 25.0 } else { row.y + row.h / 2.0 + 6.5 };
        let name = f.fit(Face::BodyMedium, 18.0, &d.name, name_w);
        c.text(f, Face::BodyMedium, 18.0, row.x + 35.0, name_y, &name, TEXT, 1.0);
        if let Some((line, color)) = line {
            c.text(f, Face::Body, 14.0, row.x + 35.0, row.y + 46.0, &line, color, 1.0);
        }
        if let Some(t) = &d.transfer {
            let track = Rect::new(row.x + 35.0, row.y + row.h - 6.0, name_w, 2.0);
            c.fill_rect(track, MAUVE, 0.2);
            c.fill_rect(Rect::new(track.x, track.y, track.w * t.percent() as f32 / 100.0, track.h), MAUVE, 0.9);
        }

        let hovered = m.hover == Some(Action::RemoveDevice(i));
        c.stroke_rrect(remove, 12.0, 1.5, if hovered { RED } else { SURFACE1 }, if hovered { 0.9 } else { 0.6 });
        let (rx, ry) = remove.center();
        c.text_centered(f, Face::Body, 15.0, rx, ry + 5.5, "Remove", if hovered { RED } else { SUBTEXT0 }, 1.0);
        hits.push(Hit { rect: remove, action: Action::RemoveDevice(i) });
    }
    if pages > 1 {
        pager(c, f, m, Rect::new(x, r.y + 56.0 + SHOWN as f32 * 70.0, r.w - 40.0, 44.0), page, pages, hits);
    }

    let note: Vec<String> = match list.iter().find(|d| d.old_app) {
        Some(d) => {
            let tail = f.measure(Face::Body, 15.0, " has the old app.");
            let name = f.fit(Face::Body, 15.0, &d.name, r.w - 48.0 - tail);
            vec![format!("{name} has the old app."), "It can't update itself: get the new one".into(), "at framecorder.coah80.com".into()]
        }
        None => vec!["Paired devices get every new clip on".into(), "their own. Removing one stops that.".into()],
    };
    let color = if list.iter().any(|d| d.old_app) { YELLOW } else { OVERLAY2 };
    let top = r.y + r.h - 24.0 - note.len() as f32 * 24.0;
    for (i, line) in note.iter().enumerate() {
        c.text(f, Face::Body, 15.0, x + 4.0, top + i as f32 * 24.0, line, color, 1.0);
    }
}

/// Which page of the list is showing, with a way to the ones either side.
#[allow(clippy::too_many_arguments)]
fn pager(c: &mut Canvas, f: &mut Fonts, m: &Model, r: Rect, page: usize, pages: usize, hits: &mut Vec<Hit>) {
    let text = format!("Page {} of {pages}", page + 1);
    c.text(f, Face::Body, 15.0, r.x + 4.0, r.y + r.h / 2.0 + 5.5, &text, OVERLAY1, 1.0);
    let arrows = [("‹", page.checked_sub(1)), ("›", (page + 1 < pages).then_some(page + 1))];
    for (i, (glyph, to)) in arrows.into_iter().enumerate() {
        let b = Rect::new(r.x + r.w - 2.0 * 50.0 + i as f32 * 50.0, r.y, 44.0, r.h);
        let Some(to) = to else {
            c.text_centered(f, Face::Body, 26.0, b.x + b.w / 2.0, r.y + r.h / 2.0 + 9.0, glyph, OVERLAY1, 0.35);
            continue;
        };
        let action = Action::DevicePage(to);
        let hovered = m.hover == Some(action);
        c.fill_rrect(b, 12.0, if hovered { MAUVE } else { SURFACE0 }, if hovered { 0.12 } else { 0.4 });
        c.stroke_rrect(b, 12.0, 1.5, if hovered { MAUVE } else { SURFACE1 }, 0.6);
        c.text_centered(f, Face::Body, 26.0, b.x + b.w / 2.0, r.y + r.h / 2.0 + 9.0, glyph, if hovered { MAUVE } else { TEXT }, 1.0);
        hits.push(Hit { rect: b, action });
    }
}

/// The code on show, how to use it, and the way back. Drawn into `inner`
/// without any card around it, so the setup can show it too.
pub fn pair(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, pairing: Result<&Pairing, &str>, hits: &mut Vec<Hit>) {
    let y = inner.y;
    match pairing {
        Ok(pairing) => {
            // QR code on white with a quiet zone, so phones read it easily.
            let side = 232.0;
            let qr = Rect::new(inner.x, y, side, side);
            c.fill_rrect(qr, 16.0, [0xff, 0xff, 0xff], 1.0);
            let module = ((side - 28.0) / pairing.size as f32).floor();
            let inset = (side - module * pairing.size as f32) / 2.0;
            let (ox, oy) = ((qr.x + inset).round(), (qr.y + inset).round());
            for (i, _) in pairing.qr.iter().enumerate().filter(|(_, &dark)| dark) {
                let (col, row) = ((i % pairing.size) as f32, (i / pairing.size) as f32);
                c.fill_rect(Rect::new(ox + col * module, oy + row * module, module, module), CRUST, 1.0);
            }

            let tx = inner.x + side + 32.0;
            label(c, f, tx, y + 16.0, "PAIR A DEVICE");
            let code = format!("{} {}", &pairing.code[..3], &pairing.code[3..]);
            c.text(f, Face::Data, 64.0, tx, y + 82.0, &code, TEXT, 1.0);
            let left = pairing.expires_in().as_secs();
            let info = format!("Works once, for the next {}:{:02}", left / 60, left % 60);
            c.text(f, Face::Body, 16.0, tx, y + 116.0, &info, OVERLAY2, 1.0);

            let pick = format!("Pick “{}”, or scan this with a phone", pairing.name);
            let steps = ["Open the framecorder app on the other device", pick.as_str(), "Type the code"];
            for (i, step) in steps.iter().enumerate() {
                let line = y + 162.0 + i as f32 * 30.0;
                c.text(f, Face::Data, 18.0, tx, line, &(i + 1).to_string(), MAUVE, 1.0);
                c.text(f, Face::Body, 18.0, tx + 24.0, line, step, SUBTEXT0, 1.0);
            }
        }
        Err(why) => {
            c.text(f, Face::BodyMedium, 20.0, inner.x, y + 30.0, why, RED, 1.0);
        }
    }

    let r = Rect::new(inner.x, inner.y + inner.h - BUTTON_H, 200.0, BUTTON_H);
    button(c, f, m, hits, r, "Back to devices", Action::ClosePair, false);
}
