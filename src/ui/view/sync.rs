//! The sync section: what's paired, and pairing something new with a QR
//! code or a six digit code.

use super::widgets::{button, label};
use super::*;

const SHOWN: usize = 4;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    if !m.sync.available {
        c.text(f, Face::BodyMedium, 20.0, inner.x, inner.y + 26.0, "The sync service isn't running on this headset", RED, 1.0);
        c.text(f, Face::Body, 17.0, inner.x, inner.y + 58.0, "Run install.sh again to set it up.", SUBTEXT0, 1.0);
        return;
    }
    match m.sync.pairing {
        Some(pairing) => pair(c, f, m, inner, pairing, hits),
        None => devices(c, f, m, inner, hits),
    }
    notice(c, f, inner);
}

/// Clips only move while the headset is up; say so where it matters.
fn notice(c: &mut Canvas, f: &mut Fonts, inner: Rect) {
    let y = inner.y + inner.h - 34.0;
    c.fill_circle(inner.x + 6.0, y - 6.0, 5.0, YELLOW, 1.0);
    c.text(f, Face::Body, 17.0, inner.x + 22.0, y, "Clips only sync while your Frame is on, on the same Wi-Fi,", YELLOW, 0.9);
    c.text(f, Face::Body, 17.0, inner.x + 22.0, y + 26.0, "and the framecorder app is open on the other device.", YELLOW, 0.9);
}

fn devices(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, hits: &mut Vec<Hit>) {
    label(c, f, inner.x, inner.y + 16.0, "PAIRED DEVICES");
    let list = m.sync.devices;
    if list.is_empty() {
        c.text(f, Face::BodyMedium, 20.0, inner.x, inner.y + 62.0, "Nothing paired yet", TEXT, 1.0);
        let lines = ["Get the framecorder app on your phone or computer,", "then pair it here. Clips show up there on their own."];
        for (i, line) in lines.iter().enumerate() {
            c.text(f, Face::Body, 17.0, inner.x, inner.y + 96.0 + i as f32 * 26.0, line, SUBTEXT0, 1.0);
        }
    }
    for (i, d) in list.iter().take(SHOWN).enumerate() {
        let r = Rect::new(inner.x, inner.y + 36.0 + i as f32 * 68.0, inner.w, 58.0);
        c.fill_rrect(r, 14.0, SURFACE0, 0.40);
        c.stroke_rrect(r, 14.0, 1.5, SURFACE1, 0.5);
        c.fill_circle(r.x + 26.0, r.y + r.h / 2.0, 5.0, GREEN, 1.0);
        c.text(f, Face::BodyMedium, 19.0, r.x + 46.0, r.y + r.h / 2.0 + 7.0, &d.name, TEXT, 1.0);

        let remove = Rect::new(r.x + r.w - 124.0, r.y + 8.0, 112.0, 42.0);
        let hovered = m.hover == Some(Action::RemoveDevice(i));
        c.stroke_rrect(remove, 12.0, 1.5, if hovered { RED } else { SURFACE1 }, if hovered { 0.9 } else { 0.6 });
        let (rx, ry) = remove.center();
        c.text_centered(f, Face::Body, 16.0, rx, ry + 5.5, "Remove", if hovered { RED } else { SUBTEXT0 }, 1.0);
        hits.push(Hit { rect: remove, action: Action::RemoveDevice(i) });
    }
    if list.len() > SHOWN {
        let more = format!("and {} more", list.len() - SHOWN);
        c.text(f, Face::Body, 16.0, inner.x, inner.y + 36.0 + SHOWN as f32 * 68.0 + 18.0, &more, OVERLAY1, 1.0);
    }

    let r = Rect::new(inner.x + inner.w - 200.0, inner.y + inner.h - 126.0, 200.0, 54.0);
    button(c, f, m, hits, r, "Pair a device", Action::Pair, true);
}

fn pair(c: &mut Canvas, f: &mut Fonts, m: &Model, inner: Rect, pairing: Result<&Pairing, &str>, hits: &mut Vec<Hit>) {
    label(c, f, inner.x, inner.y + 16.0, "PAIR A DEVICE");
    let y = inner.y + 40.0;
    match pairing {
        Ok(pairing) => {
            // QR code on white with a quiet zone, so phones read it easily.
            let side = 232.0;
            let qr = Rect::new(inner.x, y, side, side);
            c.fill_rrect(qr, 16.0, [0xff, 0xff, 0xff], 1.0);
            let module = ((side - 24.0) / pairing.size as f32).floor();
            let inset = (side - module * pairing.size as f32) / 2.0;
            let (ox, oy) = ((qr.x + inset).round(), (qr.y + inset).round());
            for (i, _) in pairing.qr.iter().enumerate().filter(|(_, &dark)| dark) {
                let (col, row) = ((i % pairing.size) as f32, (i / pairing.size) as f32);
                c.fill_rect(Rect::new(ox + col * module, oy + row * module, module, module), CRUST, 1.0);
            }

            let tx = inner.x + side + 40.0;
            let code = format!("{} {}", &pairing.code[..3], &pairing.code[3..]);
            c.text(f, Face::Data, 64.0, tx, y + 62.0, &code, TEXT, 1.0);
            let left = pairing.expires_in().as_secs();
            let info = format!("Works once, for the next {}:{:02}", left / 60, left % 60);
            c.text(f, Face::Body, 17.0, tx, y + 98.0, &info, OVERLAY1, 1.0);

            let pick = format!("2.  Pick “{}”, or scan this with a phone", pairing.name);
            let steps = ["1.  Open the framecorder app on the other device", pick.as_str(), "3.  Type the code"];
            for (i, step) in steps.iter().enumerate() {
                c.text(f, Face::Body, 18.0, tx, y + 146.0 + i as f32 * 30.0, step, SUBTEXT0, 1.0);
            }
        }
        Err(why) => {
            c.text(f, Face::BodyMedium, 20.0, inner.x, y + 30.0, why, RED, 1.0);
        }
    }

    let r = Rect::new(inner.x + inner.w - 200.0, inner.y + inner.h - 126.0, 200.0, 54.0);
    button(c, f, m, hits, r, "Back to devices", Action::ClosePair, false);
}
