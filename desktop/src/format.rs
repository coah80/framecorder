//! sizes, lengths and dates the way people say them. same as the old app's format.js

use chrono::{DateTime, Local, TimeZone};

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut n, mut i) = (bytes as f64, 0);
    while n >= 1000.0 && i < UNITS.len() - 1 {
        n /= 1000.0;
        i += 1;
    }
    if n < 10.0 && i > 0 {
        format!("{n:.1} {}", UNITS[i])
    } else {
        format!("{n:.0} {}", UNITS[i])
    }
}

pub fn length(secs: f64) -> String {
    let s = secs.round().max(0.0) as u64;
    let (h, m, r) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

/// a short stretch of time the way it's said: "30 s", then "2:00" from a minute up
pub fn span(ms: u64) -> String {
    if ms < 60_000 {
        format!("{} s", (ms + 500) / 1000)
    } else {
        length(ms as f64 / 1000.0)
    }
}

/// "4.2 MB/s"
pub fn rate(bytes_per_second: f64) -> String {
    format!("{}/s", size(bytes_per_second.max(0.) as u64))
}

/// "just now", "12 min ago", "3 h ago", then the day
pub fn ago(unix: i64) -> String {
    ago_at(unix, Local::now())
}

fn ago_at(unix: i64, now: DateTime<Local>) -> String {
    let s = (now.timestamp() - unix).max(0);
    if s < 60 {
        "just now".into()
    } else if s < 3600 {
        format!("{} min ago", s / 60)
    } else if day_key(unix) == now.date_naive() {
        format!("{} h ago", s / 3600)
    } else {
        day_on(unix, now.date_naive())
    }
}

pub fn local(unix: i64) -> DateTime<Local> {
    Local.timestamp_opt(unix, 0).single().unwrap_or_else(Local::now)
}

/// "3:42 pm"
pub fn time(unix: i64) -> String {
    local(unix).format("%-I:%M %P").to_string()
}

/// the day a clip belongs to, for grouping
pub fn day_key(unix: i64) -> chrono::NaiveDate {
    local(unix).date_naive()
}

/// "today", "yesterday", then "tue 29 sep"
pub fn day(unix: i64) -> String {
    day_on(unix, Local::now().date_naive())
}

/// the same, seen from a given day
pub fn day_on(unix: i64, today: chrono::NaiveDate) -> String {
    let d = day_key(unix);
    match (today - d).num_days() {
        0 => "today".into(),
        1 => "yesterday".into(),
        _ => d.format("%a %-d %b").to_string().to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(48_000_000), "48 MB");
        assert_eq!(size(1_200_000_000), "1.2 GB");
    }

    #[test]
    fn lengths() {
        assert_eq!(length(30.0), "0:30");
        assert_eq!(length(761.0), "12:41");
        assert_eq!(length(3725.0), "1:02:05");
        assert_eq!(span(15_000), "15 s");
        assert_eq!(span(120_000), "2:00");
        assert_eq!(rate(4_200_000.), "4.2 MB/s");
    }

    #[test]
    fn how_long_ago() {
        // wednesday 30 september 2026, 3 pm
        let now = Local.with_ymd_and_hms(2026, 9, 30, 15, 0, 0).single().unwrap();
        let t = now.timestamp();
        assert_eq!(ago_at(t - 10, now), "just now");
        assert_eq!(ago_at(t - 12 * 60, now), "12 min ago");
        assert_eq!(ago_at(t - 3 * 3600, now), "3 h ago");
        assert_eq!(ago_at(t - 20 * 3600, now), "yesterday");
        assert_eq!(ago_at(t - 5 * 86400, now), "fri 25 sep");
    }
}
