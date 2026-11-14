//! the one thing an app says about itself, and only with anonymous stats on:
//! once a day, which app and version it is and what it runs on. no id, no
//! names, nothing about a frame or a video. the service on the other end
//! keeps daily totals only (`stats/` in this repo), so one install can't be
//! told from another

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

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

/// what a ping says. `state` is what's on disk from the last one, if any
fn body(app: &str, version: &str, state: &Value) -> Value {
    json!({"app": app, "version": version, "os": os(), "new": state["ever"] != true})
}

/// sends today's ping unless one already went today. `dir` is the app's
/// state folder, `app` "desktop" or "android". a ping that doesn't make it
/// changes nothing, the next launch tries again
pub async fn ping(dir: &Path, app: &str, version: &str) {
    let url = std::env::var("FRAMECORDER_STATS_URL").unwrap_or_else(|_| URL.into());
    let today = day(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    let path = dir.join(FILE);
    let state: Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if state["last"] == today.as_str() {
        return;
    }
    let Ok(client) = reqwest::Client::builder().use_rustls_tls().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build() else { return };
    let sent = client.post(url).header(reqwest::header::USER_AGENT, format!("framecorder-{app}/{version}")).json(&body(app, version, &state)).send().await;
    if sent.is_ok_and(|r| r.status().is_success()) {
        let _ = std::fs::write(&path, json!({"last": today, "ever": true}).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_says_which_app_it_is_and_nothing_else() {
        let first = body("desktop", "0.2.0", &Value::Null);
        assert_eq!(first.to_string(), format!(r#"{{"app":"desktop","new":true,"os":"{}","version":"0.2.0"}}"#, os()));
        assert_eq!(body("android", "0.2.0", &json!({"last": "2026-11-13", "ever": true}))["new"], false);
        assert_eq!(day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(day(951_782_400), "2000-02-29");
    }
}
