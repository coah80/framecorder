//! What an install may tell us once a day, and nothing else: which app and
//! version it is, what it runs on, whether it's new and roughly how long
//! it's been around, how many of a few things happened since last time, and
//! how its settings stand. Every name and value is on a list here, and
//! anything else is refused whole, so the counts can only ever hold these.

use serde_json::Value;

pub const MAX_BODY: usize = 1024;

const APPS: [&str; 3] = ["headset", "desktop", "android"];
const SYSTEMS: [&str; 5] = ["steamos", "windows", "macos", "linux", "android"];
/// Things that happened since the last ping, and the most one ping may claim of each.
const COUNTS: [(&str, u64); 8] = [
    ("recordings", 10_000),
    ("clips", 10_000),
    // minutes of recordings saved
    ("minutes", 100_000),
    // times someone started watching live
    ("live", 10_000),
    // recordings started from a phone or computer
    ("remote", 10_000),
    // issue reports sent
    ("reports", 1_000),
    // files an app brought over, and their megabytes
    ("synced", 100_000),
    ("synced_mb", 10_000_000),
];
/// How long an install has been around, in days since its first ping.
const AGES: [&str; 5] = ["0", "1-6", "7-29", "30-89", "90+"];
const ON_OFF: &[&str] = &["on", "off"];
const PAIRED: &[&str] = &["0", "1", "2", "3+"];
/// The settings each app may report, and the values each can have.
fn settings(app: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match app {
        "headset" => &[
            ("shape", &["wide", "square", "tall", "both"]),
            ("eye", &["left", "right", "both"]),
            ("dominant", &["left", "right"]),
            ("quality", &["standard", "high", "max"]),
            ("fps", &["auto", "60", "30"]),
            ("codec", &["hevc", "h264"]),
            ("format", &["mp4", "mkv"]),
            ("clips", ON_OFF),
            ("clip", &["15", "30", "60", "120"]),
            ("mic", ON_OFF),
            ("split", ON_OFF),
            ("panels", ON_OFF),
            ("folder", &["videos", "drive"]),
            ("autostart", ON_OFF),
            ("updates", ON_OFF),
            ("paired", PAIRED),
        ],
        "desktop" => &[("background", ON_OFF), ("notify", ON_OFF), ("view", &["grid", "list"]), ("arch", &["x86_64", "aarch64"]), ("paired", PAIRED)],
        _ => &[("background", ON_OFF), ("notes", ON_OFF), ("wallpaper", ON_OFF), ("sdk", &["33-", "34", "35", "36", "37+"]), ("paired", PAIRED)],
    }
}

fn version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The counters a ping adds to, or why it was refused.
pub fn counters(body: &[u8]) -> Result<Vec<(String, u64)>, &'static str> {
    let Ok(Value::Object(ping)) = serde_json::from_slice::<Value>(body) else { return Err("not a ping") };
    let text = |key: &str| ping.get(key).and_then(Value::as_str).unwrap_or_default();
    let (app, v, os) = (text("app"), text("version"), text("os"));
    if !APPS.contains(&app) || !version(v) || !SYSTEMS.contains(&os) {
        return Err("not a ping");
    }
    let mut out = vec![(format!("active:{app}"), 1), (format!("version:{app}:{v}"), 1), (format!("os:{app}:{os}"), 1)];
    match ping.get("new") {
        Some(Value::Bool(true)) => out.push((format!("new:{app}"), 1)),
        Some(Value::Bool(false)) | None => {}
        Some(_) => return Err("not a ping"),
    }
    for (key, value) in ping.iter() {
        match (key.as_str(), value) {
            ("app" | "version" | "os" | "new", _) => {}
            ("age", Value::String(age)) if AGES.contains(&age.as_str()) => out.push((format!("age:{app}:{age}"), 1)),
            ("counts", Value::Object(counts)) => {
                for (name, n) in counts {
                    let most = COUNTS.iter().find(|(k, _)| k == name).map(|(_, most)| *most);
                    let (Some(most), Some(n)) = (most, n.as_u64()) else { return Err("not a ping") };
                    if n > most {
                        return Err("not a ping");
                    }
                    out.push((format!("count:{name}"), n));
                }
            }
            ("settings", Value::Object(given)) => {
                for (name, v) in given {
                    let allowed = settings(app).iter().find(|(k, _)| k == name).map(|(_, values)| *values);
                    let (Some(allowed), Some(v)) = (allowed, v.as_str()) else { return Err("not a ping") };
                    if !allowed.contains(&v) {
                        return Err("not a ping");
                    }
                    out.push((format!("setting:{app}:{name}={v}"), 1));
                }
            }
            _ => return Err("not a ping"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_only_ever_adds_to_the_counters_it_names() {
        let ping = br#"{"app":"headset","version":"0.2.0","os":"steamos","new":true,"age":"0","counts":{"recordings":3,"clips":12,"minutes":41},"settings":{"shape":"wide","clips":"on","paired":"3+"}}"#;
        let mut got = counters(ping).unwrap();
        got.sort();
        let want = [
            ("active:headset", 1),
            ("age:headset:0", 1),
            ("count:clips", 12),
            ("count:minutes", 41),
            ("count:recordings", 3),
            ("new:headset", 1),
            ("os:headset:steamos", 1),
            ("setting:headset:clips=on", 1),
            ("setting:headset:paired=3+", 1),
            ("setting:headset:shape=wide", 1),
            ("version:headset:0.2.0", 1),
        ];
        assert_eq!(got, want.map(|(k, n)| (k.to_string(), n)));
        assert_eq!(counters(br#"{"app":"desktop","version":"0.2.0","os":"windows"}"#).unwrap().len(), 3);
        // each app has its own settings
        let desktop = counters(br#"{"app":"desktop","version":"0.2.0","os":"linux","age":"90+","settings":{"view":"list","arch":"aarch64"}}"#).unwrap();
        assert!(desktop.contains(&("setting:desktop:view=list".to_string(), 1)) && desktop.contains(&("age:desktop:90+".to_string(), 1)));
        assert!(counters(br#"{"app":"android","version":"0.2.0","os":"android","settings":{"sdk":"36","wallpaper":"on"}}"#).is_ok());
    }

    #[test]
    fn anything_else_is_refused_whole() {
        for bad in [
            r#"{"app":"desktop","version":"0.2.0","os":"windows","id":"abc"}"#,
            r#"{"app":"desktop","version":"0.2.0-<script>","os":"windows"}"#,
            r#"{"app":"toaster","version":"0.2.0","os":"windows"}"#,
            r#"{"app":"headset","version":"0.2.0","os":"steamos","counts":{"recordings":999999}}"#,
            r#"{"app":"headset","version":"0.2.0","os":"steamos","counts":{"videos":1}}"#,
            r#"{"app":"headset","version":"0.2.0","os":"steamos","settings":{"folder":"/home/me"}}"#,
            r#"{"app":"headset","version":"0.2.0","os":"steamos","settings":{"shape":"hexagon"}}"#,
            // another app's setting, a made-up age, more minutes than there are
            r#"{"app":"desktop","version":"0.2.0","os":"linux","settings":{"shape":"wide"}}"#,
            r#"{"app":"desktop","version":"0.2.0","os":"linux","age":"forever"}"#,
            r#"{"app":"headset","version":"0.2.0","os":"steamos","counts":{"minutes":100001}}"#,
            r#"["app"]"#,
            "",
        ] {
            assert!(counters(bad.as_bytes()).is_err(), "{bad}");
        }
    }
}
