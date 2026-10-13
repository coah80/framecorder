//! The sync section: pairing something new with a QR code or a six digit
//! code on the left, and what's paired on the right.

use super::widgets::{button, card, label};
use super::*;

const SHOWN: usize = 4;
const DEVICES_W: f32 = 340.0;
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
    for (i, d) in list.iter().take(SHOWN).enumerate() {
        let row = Rect::new(x, r.y + 56.0 + i as f32 * 70.0, r.w - 40.0, 58.0);
        c.fill_rrect(row, 14.0, BASE, 0.6);
        c.fill_circle(row.x + 20.5, row.y + row.h / 2.0, 4.5, GREEN, 1.0);
        c.text(f, Face::BodyMedium, 18.0, row.x + 35.0, row.y + row.h / 2.0 + 6.5, &d.name, TEXT, 1.0);

        let remove = Rect::new(row.x + row.w - 8.0 - 96.0, row.y + 8.0, 96.0, 42.0);
        let hovered = m.hover == Some(Action::RemoveDevice(i));
        c.stroke_rrect(remove, 12.0, 1.5, if hovered { RED } else { SURFACE1 }, if hovered { 0.9 } else { 0.6 });
        let (rx, ry) = remove.center();
        c.text_centered(f, Face::Body, 15.0, rx, ry + 5.5, "Remove", if hovered { RED } else { SUBTEXT0 }, 1.0);
        hits.push(Hit { rect: remove, action: Action::RemoveDevice(i) });
    }
    if list.len() > SHOWN {
        let more = format!("and {} more", list.len() - SHOWN);
        c.text(f, Face::Body, 15.0, x + 4.0, r.y + 56.0 + SHOWN as f32 * 70.0 + 16.0, &more, OVERLAY1, 1.0);
    }
    let note = ["Paired devices get every new clip on", "their own. Removing one stops that."];
    for (i, line) in note.iter().enumerate() {
        c.text(f, Face::Body, 15.0, x + 4.0, r.y + r.h - 48.0 + i as f32 * 24.0, line, OVERLAY2, 1.0);
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
