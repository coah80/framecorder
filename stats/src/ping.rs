//! What an install may tell us once a day, and nothing else: which app and
//! version it is, what it runs on, whether it's new, how many recordings and
//! clips it made, and a handful of settings. Anything unexpected is refused
//! whole, so the counts can only ever hold these.

use serde_json::Value;

pub const MAX_BODY: usize = 1024;

const APPS: [&str; 3] = ["headset", "desktop", "android"];
const SYSTEMS: [&str; 5] = ["steamos", "windows", "macos", "linux", "android"];
const COUNTS: [&str; 2] = ["recordings", "clips"];
const SETTINGS: [(&str, &[&str]); 10] = [
    ("shape", &["wide", "square", "tall", "both"]),
    ("eye", &["left", "right", "both"]),
    ("quality", &["standard", "high", "max"]),
    ("fps", &["auto", "60", "30"]),
    ("codec", &["hevc", "h264"]),
    ("format", &["mp4", "mkv"]),
    ("clips", &["on", "off"]),
    ("mic", &["on", "off"]),
    ("split", &["on", "off"]),
    ("panels", &["on", "off"]),
];

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
            ("counts", Value::Object(counts)) => {
                for (name, n) in counts {
                    let n = n.as_u64().filter(|n| *n <= 10_000);
                    let (true, Some(n)) = (COUNTS.contains(&name.as_str()), n) else { return Err("not a ping") };
                    out.push((format!("count:{name}"), n));
                }
            }
            ("settings", Value::Object(settings)) => {
                for (name, v) in settings {
                    let allowed = SETTINGS.iter().find(|(k, _)| k == name).map(|(_, values)| *values);
                    let (Some(allowed), Some(v)) = (allowed, v.as_str()) else { return Err("not a ping") };
                    if !allowed.contains(&v) {
                        return Err("not a ping");
                    }
                    out.push((format!("setting:{name}={v}"), 1));
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
        let ping = br#"{"app":"headset","version":"0.2.0","os":"steamos","new":true,"counts":{"recordings":3,"clips":12},"settings":{"shape":"wide","clips":"on"}}"#;
        let mut got = counters(ping).unwrap();
        got.sort();
        let want = [
            ("active:headset", 1),
            ("count:clips", 12),
            ("count:recordings", 3),
            ("new:headset", 1),
            ("os:headset:steamos", 1),
            ("setting:clips=on", 1),
            ("setting:shape=wide", 1),
            ("version:headset:0.2.0", 1),
        ];
        assert_eq!(got, want.map(|(k, n)| (k.to_string(), n)));
        assert_eq!(counters(br#"{"app":"desktop","version":"0.2.0","os":"windows"}"#).unwrap().len(), 3);
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
            r#"["app"]"#,
            "",
        ] {
            assert!(counters(bad.as_bytes()).is_err(), "{bad}");
        }
    }
}
