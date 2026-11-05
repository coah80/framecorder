//! The report screen: what went wrong, typed on SteamVR's keyboard, next to
//! what goes with it, and once it's sent, the issue it became.

use super::super::report::Summary;
use super::widgets::{card, label};
use super::*;

/// Where the report is, for drawing.
#[derive(Clone, Copy)]
pub enum Phase<'a> {
    Writing,
    Sending,
    Sent { number: u64, url: &'a str },
    Failed(&'a str),
}

pub struct ReportView<'a> {
    pub text: &'a str,
    /// SteamVR's keyboard is up.
    pub typing: bool,
    /// What goes along, once it's been gathered.
    pub summary: Option<&'a Summary>,
    pub phase: Phase<'a>,
}

impl ReportView<'_> {
    /// Something's written, the details are in, and nothing's on its way.
    pub fn can_send(&self) -> bool {
        !self.text.trim().is_empty() && self.summary.is_some() && matches!(self.phase, Phase::Writing | Phase::Failed(_))
    }

    fn editable(&self) -> bool {
        matches!(self.phase, Phase::Writing | Phase::Failed(_))
    }
}

const RIGHT_W: f32 = 420.0;
const GAP: f32 = 22.0;
const LEFT_W: f32 = WIDTH as f32 - 2.0 * PAD - RIGHT_W - GAP;
const RIGHT_X: f32 = PAD + LEFT_W + GAP;
const INSET: f32 = 24.0;
const BOX_H: f32 = 372.0;
const LINE_H: f32 = 30.0;
const TEXT_SIZE: f32 = 20.0;
const DETAIL_ROW_H: f32 = 60.0;

pub fn draw(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    if let Phase::Sent { number, url } = m.report.phase {
        return sent(c, f, number, url);
    }
    description(c, f, m, hits);
    details(c, f, m);
}

/// The box with what the user typed (tap it to type), and the way to send.
fn description(c: &mut Canvas, f: &mut Fonts, m: &Model, hits: &mut Vec<Hit>) {
    let r = Rect::new(PAD, CONTENT_Y, LEFT_W, CONTENT_H);
    card(c, r);
    let x = r.x + INSET;
    label(c, f, x, r.y + 36.0, "WHAT WENT WRONG");

    let report = &m.report;
    let b = Rect::new(x, r.y + 56.0, r.w - 2.0 * INSET, BOX_H);
    let hovered = report.editable() && m.over(b);
    c.fill_rrect(b, 16.0, BASE, 0.6);
    let (stroke, alpha, width) = match (report.typing, hovered) {
        (true, _) => (MAUVE, 0.9, 2.0),
        (false, true) => (MAUVE, 0.6, 1.5),
        (false, false) => (SURFACE1, 0.6, 1.5),
    };
    c.stroke_rrect(b, 16.0, width, stroke, alpha);

    let max_lines = ((b.h - 40.0) / LINE_H) as usize;
    let room = b.w - 40.0;
    let baseline = |i: usize| b.y + 20.0 + TEXT_SIZE + i as f32 * LINE_H;
    if report.text.trim().is_empty() {
        let hint = ["Tap here to type what went wrong,", "and what you expected instead."];
        for (i, line) in hint.iter().enumerate() {
            c.text(f, Face::Body, TEXT_SIZE, b.x + 20.0, baseline(i), line, OVERLAY1, 1.0);
        }
    } else {
        let mut lines = wrap(f, Face::Body, TEXT_SIZE, report.text, room);
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            if let Some(last) = lines.last_mut() {
                last.push_str(" …");
            }
        }
        for (i, line) in lines.iter().enumerate() {
            let line = f.fit(Face::Body, TEXT_SIZE, line, room);
            c.text(f, Face::Body, TEXT_SIZE, b.x + 20.0, baseline(i), &line, TEXT, 1.0);
        }
    }
    if report.editable() {
        hits.push(Hit { rect: b, action: Action::Type });
    }

    let under = b.y + b.h + 30.0;
    if report.typing {
        c.text(f, Face::Body, 16.0, x, under, "Typing on SteamVR's keyboard · press Done when you're finished", MAUVE, 1.0);
    } else if !report.text.is_empty() && report.editable() {
        c.text(f, Face::Body, 16.0, x, under, "Tap the box to change it", OVERLAY1, 1.0);
    }

    // Send, and when it can't be pressed yet, why.
    let send = Rect::new(r.x + r.w - INSET - 200.0, r.y + r.h - INSET - BUTTON_H, 200.0, BUTTON_H);
    let (text, why) = match report.phase {
        Phase::Sending => ("Sending…", None),
        Phase::Failed(_) => ("Try again", None),
        _ if report.summary.is_none() => ("Send", Some("Gathering the details…")),
        _ if report.text.trim().is_empty() => ("Send", Some("Say what went wrong first")),
        _ => ("Send", None),
    };
    if report.can_send() {
        widgets::button(c, f, m, hits, send, text, Action::Send, true);
    } else {
        c.fill_rrect(send, 14.0, MAUVE, 0.3);
        let (cx, cy) = send.center();
        c.text_centered(f, Face::Heading, 18.0, cx, cy + 6.5, text, CRUST, 0.6);
    }
    if let Some(why) = why {
        let w = f.measure(Face::Body, 16.0, why);
        c.text(f, Face::Body, 16.0, send.x - 20.0 - w, send.y + send.h / 2.0 + 5.5, why, OVERLAY1, 1.0);
    }
}

/// What goes with the words, a line a thing, and where the whole of it is.
fn details(c: &mut Canvas, f: &mut Fonts, m: &Model) {
    let r = Rect::new(RIGHT_X, CONTENT_Y, RIGHT_W, CONTENT_H);
    card(c, r);
    let x = r.x + INSET;
    let w = r.w - 2.0 * INSET;
    label(c, f, x, r.y + 36.0, "WHAT GETS SENT");

    let Some(s) = m.report.summary else {
        c.text(f, Face::BodyMedium, 18.0, x, r.y + 80.0, "Gathering the details…", TEXT, 1.0);
        c.text(f, Face::Body, 16.0, x, r.y + 108.0, "Versions, logs and recent errors. A few seconds.", SUBTEXT0, 1.0);
        return;
    };
    let errors = match s.errors {
        0 => "None from the last 2 days".to_string(),
        1 => "1 line from the last 2 days".to_string(),
        n => format!("{n} lines from the last 2 days"),
    };
    let logs = if s.logs.is_empty() { "None could be read".to_string() } else { s.logs.join(", ") };
    let rows = [
        ("FRAMECORDER", s.framecorder.as_str()),
        ("STEAMOS", s.os.as_str()),
        ("STEAMVR", s.steamvr.as_str()),
        ("RECENT ERRORS", errors.as_str()),
        ("LOGS", logs.as_str()),
    ];
    let mut y = r.y + 76.0;
    for (name, value) in rows {
        c.text(f, Face::Label, 13.0, x, y, name, OVERLAY2, 1.0);
        let lines = wrap(f, Face::BodyMedium, 17.0, value, w);
        let shown = if name == "LOGS" { 3 } else { 1 };
        for (i, line) in lines.iter().take(shown).enumerate() {
            let line = if i + 1 == shown && lines.len() > shown { f.fit(Face::BodyMedium, 17.0, &format!("{line} …"), w) } else { f.fit(Face::BodyMedium, 17.0, line, w) };
            c.text(f, Face::BodyMedium, 17.0, x, y + 24.0 + i as f32 * 24.0, &line, TEXT, 1.0);
        }
        y += DETAIL_ROW_H + (shown.min(lines.len()) as f32 - 1.0) * 24.0;
    }

    let note = ["The whole thing, as it goes, is in", "~/.local/state/framecorder/report.json"];
    for (i, line) in note.iter().enumerate() {
        c.text(f, Face::Body, 14.0, x, r.y + r.h - 48.0 + i as f32 * 22.0, line, OVERLAY1, 1.0);
    }
}

/// The issue it became, and where to follow it.
fn sent(c: &mut Canvas, f: &mut Fonts, number: u64, url: &str) {
    let r = Rect::new(PAD, CONTENT_Y, WIDTH as f32 - 2.0 * PAD, CONTENT_H);
    card(c, r);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + 120.0);
    c.ring(cx, cy, 52.0, 4.0, MAUVE, 0.7);
    c.fill_circle(cx, cy, 38.0, MAUVE, 0.92);
    c.text_centered(f, Face::Heading, 40.0, cx, cy + 124.0, "Sent, thank you", TEXT, 1.0);
    c.text_centered(f, Face::Data, 44.0, cx, cy + 190.0, &format!("Issue #{number}"), MAUVE, 1.0);
    let link = f.fit(Face::BodyMedium, 20.0, url.trim_start_matches("https://"), r.w - 2.0 * INSET);
    c.text_centered(f, Face::BodyMedium, 20.0, cx, cy + 232.0, &link, TEXT, 1.0);
    let lines = ["Open it on your phone or computer to follow along.", "If anything's unclear, the question gets asked there."];
    for (i, line) in lines.iter().enumerate() {
        c.text_centered(f, Face::Body, 18.0, cx, cy + 282.0 + i as f32 * 28.0, line, SUBTEXT0, 1.0);
    }
}

/// Breaks text into lines no wider than `max`, at spaces and where the
/// text already breaks. A word too long for a line gets one to itself.
pub fn wrap(f: &mut Fonts, face: Face, size: f32, text: &str, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let longer = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || f.measure(face, size, &longer) <= max {
                line = longer;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_at_spaces_and_keeps_the_users_own_lines() {
        let mut f = Fonts::load().unwrap();
        let text = "the clip came out tilted to the left\n\nand the sound was gone";
        let lines = wrap(&mut f, Face::Body, 20.0, text, 200.0);
        assert!(lines.len() >= 4, "{lines:?}");
        assert!(lines.contains(&String::new()), "a blank line stays a blank line");
        assert!(lines.iter().all(|l| f.measure(Face::Body, 20.0, l) <= 200.0 || !l.contains(' ')), "{lines:?}");
        assert_eq!(lines.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>(), "no words lost");
    }
}
