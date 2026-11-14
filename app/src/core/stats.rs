//! the one thing an app says about itself, and only with anonymous stats on:
//! once a day, which app and version it is, what it runs on, roughly how long
//! it's been installed, how a few of its settings stand, and how many files
//! it brought over since the last one and their size. no id, no names,
//! nothing about a frame or what's in a video. the service on the other end
//! keeps daily totals only (`stats/` in this repo), so one install can't be
//! told from another

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::store::Entry;

const URL: &str = "https://stats.coah80.com/v1/ping";
const FILE: &str = "stats.json";

fn os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "android" => "android",
        _ => "linux",
    }
}

/// the utc date `secs` after 1970 falls on, as 2026-11-14
fn day(secs: u64) -> String {
    // civil-from-days, howard hinnant's
    let z = (secs / 86400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// how long an install has been around, as the stats service buckets it
fn age(days: u64) -> &'static str {
    match days {
        0 => "0",
        1..=6 => "1-6",
        7..=29 => "7-29",
        30..=89 => "30-89",
        _ => "90+",
    }
}

/// how many frames are paired, as the stats service buckets it
pub fn paired(n: usize) -> String {
    if n >= 3 { "3+".into() } else { n.to_string() }
}

/// how many of `clips` came over after `since`, and their megabytes, capped
/// at what the stats service takes
fn synced(clips: &[Entry], since: i64) -> (u64, u64) {
    let new = clips.iter().filter(|e| e.synced_at > since);
    let (files, bytes) = new.fold((0u64, 0u64), |(n, b), e| (n + 1, b + e.size));
    (files.min(100_000), (bytes / 1_000_000).min(10_000_000))
}

/// what a ping says. `state` is what's on disk from the last one, if any,
/// `first` the day the install was first seen, `now` the time, in seconds since 1970
fn body(app: &str, version: &str, settings: &[(&str, String)], clips: &[Entry], state: &Value, first: u64, now: u64) -> Value {
    let settings: serde_json::Map<String, Value> = settings.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
    // a first ping looks back a day, so an old library doesn't count as brought over today
    let since = state["at"].as_i64().unwrap_or(now as i64 - 86400);
    let (files, mb) = synced(clips, since);
    json!({
        "app": app,
        "version": version,
        "os": os(),
        "new": state["ever"] != true,
        "age": age((now / 86400).saturating_sub(first)),
        "counts": {"synced": files, "synced_mb": mb},
        "settings": settings,
    })
}

/// sends today's ping unless one already went today. `dir` is the app's
/// state folder, `app` "desktop" or "android", `settings` the app's own, in
/// the service's words, `clips` everything it has synced. a ping that
/// doesn't make it changes nothing, the next launch tries again
pub async fn ping(dir: &Path, app: &str, version: &str, settings: &[(&str, String)], clips: &[Entry]) {
    let url = std::env::var("FRAMECORDER_STATS_URL").unwrap_or_else(|_| URL.into());
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let today = day(secs);
    let path = dir.join(FILE);
    let state: Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if state["last"] == today.as_str() {
        return;
    }
    let first = state["first"].as_u64().unwrap_or(secs / 86400);
    let Ok(client) = reqwest::Client::builder().use_rustls_tls().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build() else { return };
    let sent = client.post(url).header(reqwest::header::USER_AGENT, format!("framecorder-{app}/{version}")).json(&body(app, version, settings, clips, &state, first, secs)).send().await;
    if sent.is_ok_and(|r| r.status().is_success()) {
        let _ = std::fs::write(&path, json!({"last": today, "ever": true, "first": first, "at": secs}).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(synced_at: i64, size: u64) -> Entry {
        Entry { host: "h".into(), id: synced_at.to_string(), name: "a.mp4".into(), kind: "clip".into(), size, created: 0, duration_s: None, location: String::new(), synced_at }
    }

    #[test]
    fn a_ping_says_which_app_it_is_and_nothing_else() {
        let day = 86400;
        let clips = [clip(20_000 * day - 2 * day, 9_000_000), clip(20_000 * day - 60, 40_500_000), clip(20_000 * day - 30, 2_000_000)];
        let first = body("desktop", "0.2.0", &[("view", "grid".into()), ("paired", paired(4))], &clips, &Value::Null, 20_000, 20_000 * day as u64);
        assert_eq!(
            first.to_string(),
            format!(r#"{{"age":"0","app":"desktop","counts":{{"synced":2,"synced_mb":42}},"new":true,"os":"{}","settings":{{"paired":"3+","view":"grid"}},"version":"0.2.0"}}"#, os())
        );
        // later pings count from the one before
        let state = json!({"last": "2026-11-13", "ever": true, "at": 20_000 * day - 45});
        let later = body("android", "0.2.0", &[], &clips, &state, 20_000, 20_045 * day as u64);
        assert_eq!((later["new"].clone(), later["age"].clone(), later["counts"]["synced"].clone()), (json!(false), json!("30-89"), json!(1)));
        assert_eq!(super::day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(super::day(951_782_400), "2000-02-29");
    }
}
