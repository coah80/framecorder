//! The dashboard's two pages, drawn here whole: no scripts, nothing loaded
//! from anywhere else.

use std::collections::BTreeMap;

use crate::store::Day;

const STYLE: &str = "
:root{color-scheme:dark}*{box-sizing:border-box}
body{margin:0;background:#11111b;color:#cdd6f4;font:15px/1.5 Poppins,system-ui,sans-serif}
main{max-width:1080px;margin:0 auto;padding:32px 20px 64px}
h1{font:800 30px Montserrat,system-ui,sans-serif;margin:0}h1 b{color:#cba6f7}
h2{font:700 15px Montserrat,system-ui,sans-serif;margin:0 0 12px;color:#a6adc8}
header{display:flex;justify-content:space-between;align-items:center;margin-bottom:24px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin-bottom:12px}
.two{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:12px;margin-bottom:12px}
.card{background:#1e1e2e;border:1px solid #313244;border-radius:16px;padding:16px 18px}
.n{font:600 30px 'Space Grotesk',ui-monospace,monospace;color:#cdd6f4}.l{color:#a6adc8;font-size:13px}.s{color:#7f849c;font-size:12px}
table{width:100%;border-collapse:collapse}td{padding:4px 0;border-top:1px solid #313244}td:last-child{text-align:right;font-family:'Space Grotesk',ui-monospace,monospace}
tr:first-child td{border-top:0}
svg{display:block;width:100%;height:auto}.key{display:flex;gap:14px;font-size:12px;color:#a6adc8;margin-top:8px}.key i{display:inline-block;width:10px;height:10px;border-radius:3px;margin-right:5px}
button,input{font:inherit}button{background:#cba6f7;color:#11111b;border:0;border-radius:12px;padding:10px 18px;font-weight:600;cursor:pointer}
button.quiet{background:none;color:#a6adc8;border:1px solid #313244}
form.login{max-width:340px;margin:18vh auto 0;display:grid;gap:12px}
input{background:#1e1e2e;border:1px solid #313244;border-radius:12px;padding:12px 14px;color:#cdd6f4}
.err{color:#f38ba8;font-size:13px;min-height:20px}
";

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page(body: &str) -> String {
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"robots\" content=\"noindex\"><title>framecorder stats</title><style>{STYLE}</style></head><body><main>{body}</main></body></html>")
}

pub fn login(message: &str) -> String {
    page(&format!(
        "<form class=\"login\" method=\"post\" action=\"/login\"><h1>framecorder<b>.</b></h1><input type=\"password\" name=\"password\" placeholder=\"Password\" autocomplete=\"current-password\" autofocus required><div class=\"err\">{}</div><button>Open</button></form>",
        esc(message)
    ))
}

const SURFACES: [(&str, &str, &str); 3] = [("headset", "Headset", "#cba6f7"), ("desktop", "Desktop", "#89b4fa"), ("android", "Android", "#a6e3a1")];

/// Bars for each day, one colour per series stacked, with the value on hover.
fn bars(days: &[(String, &Day)], series: &[(&str, &str, &str)]) -> String {
    let (w, h, pad) = (1000.0, 180.0, 22.0);
    let totals: Vec<u64> = days.iter().map(|(_, d)| series.iter().map(|(key, _, _)| d.get(*key).copied().unwrap_or(0)).sum()).collect();
    let top = totals.iter().copied().max().unwrap_or(0).max(1) as f64;
    let step = w / days.len().max(1) as f64;
    let mut svg = format!("<svg viewBox=\"0 0 {w} {}\" role=\"img\">", h + pad);
    for (i, (name, day)) in days.iter().enumerate() {
        let mut y = h;
        for (key, label, colour) in series {
            let n = day.get(*key).copied().unwrap_or(0);
            let bar = n as f64 / top * (h - 8.0);
            y -= bar;
            if n > 0 {
                svg += &format!("<rect x=\"{:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{bar:.1}\" rx=\"3\" fill=\"{colour}\"><title>{} · {label}: {n}</title></rect>", i as f64 * step + 2.0, step - 4.0, esc(name));
            }
        }
        if i % 5 == 0 || i + 1 == days.len() {
            svg += &format!("<text x=\"{:.1}\" y=\"{}\" fill=\"#7f849c\" font-size=\"11\" text-anchor=\"middle\">{}</text>", (i as f64 + 0.5) * step, h + 16.0, esc(&name[5..]));
        }
    }
    svg += &format!("<text x=\"0\" y=\"11\" fill=\"#7f849c\" font-size=\"11\">{top:.0}</text></svg><div class=\"key\">");
    for (_, label, colour) in series {
        svg += &format!("<span><i style=\"background:{colour}\"></i>{label}</span>");
    }
    svg + "</div>"
}

/// Counters starting with `prefix` added up over `days`, biggest first.
fn ranked(days: &[(String, &Day)], prefix: &str) -> Vec<(String, u64)> {
    let mut sums: BTreeMap<&str, u64> = BTreeMap::new();
    for (_, day) in days {
        for (key, n) in day.iter().filter(|(k, _)| k.starts_with(prefix)) {
            *sums.entry(&key[prefix.len()..]).or_default() += n;
        }
    }
    let mut rows: Vec<(String, u64)> = sums.into_iter().map(|(k, n)| (k.to_owned(), n)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows
}

fn table(title: &str, rows: &[(String, u64)]) -> String {
    let mut html = format!("<div class=\"card\"><h2>{}</h2><table>", esc(title));
    if rows.is_empty() {
        html += "<tr><td class=\"s\">Nothing yet</td><td></td></tr>";
    }
    for (name, n) in rows.iter().take(12) {
        html += &format!("<tr><td>{}</td><td>{n}</td></tr>", esc(name));
    }
    html + "</table></div>"
}

/// `window` is the days to show, oldest first, ending today.
pub fn dashboard(all: &BTreeMap<String, Day>, window: &[String]) -> String {
    let empty = Day::new();
    let days: Vec<(String, &Day)> = window.iter().map(|d| (d.clone(), all.get(d).unwrap_or(&empty))).collect();
    let week = &days[days.len().saturating_sub(7)..];
    let sum = |span: &[(String, &Day)], key: &str| span.iter().map(|(_, d)| d.get(key).copied().unwrap_or(0)).sum::<u64>();
    let today = days.last().map(|(_, d)| *d).unwrap_or(&empty);
    let yesterday = days.len().checked_sub(2).map(|i| days[i].1).unwrap_or(&empty);

    let mut tiles = String::new();
    for (key, label, _) in SURFACES {
        let active = format!("active:{key}");
        tiles += &format!(
            "<div class=\"card\"><div class=\"n\">{}</div><div class=\"l\">{label} active today</div><div class=\"s\">{} yesterday · {} new this week</div></div>",
            today.get(&active).copied().unwrap_or(0),
            yesterday.get(&active).copied().unwrap_or(0),
            sum(week, &format!("new:{key}"))
        );
    }
    for (key, label) in [("count:recordings", "Recordings this week"), ("count:clips", "Clips this week")] {
        tiles += &format!("<div class=\"card\"><div class=\"n\">{}</div><div class=\"l\">{label}</div></div>", sum(week, key));
    }
    let clicks = |span: &[(String, &Day)]| ranked(span, "discord:from=").iter().map(|(_, n)| n).sum::<u64>();
    tiles += &format!("<div class=\"card\"><div class=\"n\">{}</div><div class=\"l\">Discord clicks this week</div><div class=\"s\">{} in 30 days</div></div>", clicks(week), clicks(&days));

    let actives: Vec<(String, &str, &str)> = SURFACES.iter().map(|(key, label, colour)| (format!("active:{key}"), *label, *colour)).collect();
    let actives: Vec<(&str, &str, &str)> = actives.iter().map(|(k, l, c)| (k.as_str(), *l, *c)).collect();
    // one bar a day for every way into the discord
    let visits: Vec<(String, Day)> = days.iter().map(|(name, d)| (name.clone(), Day::from([("visits".to_owned(), d.iter().filter(|(k, _)| k.starts_with("discord:from=")).map(|(_, n)| n).sum())]))).collect();
    let visits: Vec<(String, &Day)> = visits.iter().map(|(name, d)| (name.clone(), d)).collect();

    let mut body = format!("<header><h1>framecorder<b>.</b> stats</h1><form method=\"post\" action=\"/logout\"><button class=\"quiet\">Lock</button></form></header><div class=\"grid\">{tiles}</div>");
    body += &format!("<div class=\"card\" style=\"margin-bottom:12px\"><h2>Installs that ran each day, last 30 days</h2>{}</div>", bars(&days, &actives));
    body += &format!(
        "<div class=\"two\"><div class=\"card\"><h2>Recordings and clips made</h2>{}</div><div class=\"card\"><h2>Discord link visits</h2>{}</div></div>",
        bars(&days, &[("count:recordings", "Recordings", "#cba6f7"), ("count:clips", "Clips", "#f9e2af")]),
        bars(&visits, &[("visits", "Visits", "#89b4fa")])
    );
    body += "<div class=\"two\">";
    for (key, label, _) in SURFACES {
        body += &table(&format!("{label} versions, this week"), &ranked(week, &format!("version:{key}:")));
    }
    body += &table("Desktop systems, this week", &ranked(week, "os:desktop:"));
    body += &table("Headset settings, this week", &ranked(week, "setting:"));
    body += &table("Discord visits from, 30 days", &ranked(&days, "discord:from="));
    body += &table("Discord visits by country, 30 days", &ranked(&days, "discord:country="));
    body += "</div><p class=\"s\">Every number is a daily total. No install, address or device is stored.</p>";
    page(&body)
}
