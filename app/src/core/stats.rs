//! the one thing an app says about itself, and only with anonymous stats on:
//! once a day, which app and version it is, what it runs on, roughly how long
//! it's been installed, how a few of its settings stand, and how many files
//! it brought over since the last one and their size, plus what it costs:
//! its memory by how long it had been open (a leak shows as memory climbing
//! with time), its cpu, and how many cores and how much ram the machine has,
//! all in coarse buckets. no id, no names,
//! nothing about a frame or what's in a video. the service on the other end
//! keeps daily totals only (`stats/` in this repo), so one install can't be
//! told from another

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// which of `edges` `v` falls between, named the way the stats service has it
fn bucket(v: f64, edges: &[u32]) -> String {
    match edges.iter().position(|&e| v < f64::from(e)) {
        Some(0) => format!("<{}", edges[0]),
        Some(i) => format!("{}-{}", edges[i - 1], edges[i]),
        None => format!("{}+", edges[edges.len() - 1]),
    }
}

/// how long the app had been open
fn up(secs: f64) -> &'static str {
    match secs / 3600.0 {
        h if h < 1.0 => "<1h",
        h if h < 6.0 => "1-6h",
        h if h < 24.0 => "6-24h",
        _ => "1d+",
    }
}

/// what this process is using right now: resident memory in mb and cpu time so far
mod usage {
    use std::time::Duration;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn memory_mb() -> Option<f64> {
        let pages: f64 = std::fs::read_to_string("/proc/self/statm").ok()?.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: sysconf only reads a value
        Some(pages * unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64 / 1e6)
    }

    #[cfg(target_os = "macos")]
    pub fn memory_mb() -> Option<f64> {
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: the buffer is a proc_taskinfo of the size given
        let got = unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, (&raw mut info).cast(), size) };
        (got == size).then(|| info.pti_resident_size as f64 / 1e6)
    }

    #[cfg(windows)]
    pub fn memory_mb() -> Option<f64> {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        c.cb = size;
        // SAFETY: GetCurrentProcess is a pseudo handle that needs no closing, the counters are the size given
        (unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, size) } != 0).then(|| c.WorkingSetSize as f64 / 1e6)
    }

    #[cfg(unix)]
    pub fn cpu() -> Option<Duration> {
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: the struct is the one getrusage fills
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) } != 0 {
            return None;
        }
        let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        Some(Duration::from_secs_f64(secs(u.ru_utime) + secs(u.ru_stime)))
    }

    #[cfg(windows)]
    pub fn cpu() -> Option<Duration> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: four FILETIMEs to fill, a pseudo handle that needs no closing
        if unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
            return None;
        }
        // in 100 ns steps
        let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
    }

    /// the machine's memory, gb
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn ram_gb() -> Option<f64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: f64 = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(kb / 1024.0 / 1024.0)
    }

    #[cfg(target_os = "macos")]
    pub fn ram_gb() -> Option<f64> {
        let mut bytes: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: hw.memsize is a u64 and the buffer is one
        let ok = unsafe { libc::sysctlbyname(c"hw.memsize".as_ptr(), (&raw mut bytes).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
        ok.then(|| bytes as f64 / 1024f64.powi(3))
    }

    #[cfg(windows)]
    pub fn ram_gb() -> Option<f64> {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: the struct with its length set, as the call wants it
        (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then(|| m.ullTotalPhys as f64 / 1024f64.powi(3))
    }
}

/// when this process first asked, close enough to when the app opened
fn opened() -> Instant {
    static OPENED: OnceLock<Instant> = OnceLock::new();
    *OPENED.get_or_init(Instant::now)
}

/// notes the app's memory by how long it's been open, and its cpu since the
/// last note, as `name=bucket`s to count. the first note only starts the clock
fn sample() -> Vec<String> {
    static LAST: Mutex<Option<(Instant, Duration)>> = Mutex::new(None);
    let open = opened().elapsed().as_secs_f64();
    let Some(cpu) = usage::cpu() else { return Vec::new() };
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let before = last.replace((Instant::now(), cpu));
    let mut keys = Vec::new();
    if let Some(mb) = usage::memory_mb() {
        keys.push(format!("app_mb@{}={}", up(open), bucket(mb, &[25, 50, 100, 200, 400, 800])));
    }
    if let Some((at, then)) = before {
        let secs = at.elapsed().as_secs_f64();
        if secs > 60.0 {
            keys.push(format!("app_cpu={}", bucket(100.0 * cpu.saturating_sub(then).as_secs_f64() / secs, &[1, 5, 15, 40])));
        }
    }
    keys
}

/// the machine it runs on, once a ping
fn machine() -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(n) = std::thread::available_parallelism() {
        keys.push(format!("cores={}", bucket(n.get() as f64, &[4, 8, 16])));
    }
    if let Some(gb) = usage::ram_gb() {
        // 16 gb sticks show as a hair under 16 once the system takes its share
        keys.push(format!("ram_gb={}", bucket(gb.round(), &[4, 8, 16, 32])));
    }
    keys
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
        "perf": perf(state, &machine()),
    })
}

/// the samples kept since the last ping, plus `extra` once each, capped at
/// what the service takes
fn perf(state: &Value, extra: &[String]) -> Value {
    let mut out = state["perf"].as_object().cloned().unwrap_or_default();
    for key in extra {
        out.insert(key.clone(), json!(out.get(key).and_then(Value::as_u64).unwrap_or(0) + 1));
    }
    for n in out.values_mut() {
        *n = json!(n.as_u64().unwrap_or(0).min(10_000));
    }
    Value::Object(out)
}

/// sends today's ping unless one already went today. `dir` is the app's
/// state folder, `app` "desktop" or "android", `settings` the app's own, in
/// the service's words, `clips` everything it has synced. a ping that
/// doesn't make it changes nothing, the next launch tries again
pub async fn ping(dir: &Path, app: &str, version: &str, settings: &[(&str, String)], clips: &[Entry]) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let today = day(secs);
    let path = dir.join(FILE);
    let mut state: Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    // every call notes what the app is using, whether or not today's ping went
    let noted = sample();
    if !noted.is_empty() {
        if !state.is_object() {
            state = json!({});
        }
        state["perf"] = perf(&state, &noted);
        let _ = std::fs::write(&path, state.to_string());
    }
    if state["last"] == today.as_str() {
        return;
    }
    let first = state["first"].as_u64().unwrap_or(secs / 86400);
    let plain = body(app, version, settings, clips, &state, first, secs).to_string();
    let Some(sealed) = super::seal::seal(&super::seal::SERVICE, plain.as_bytes()) else { return };
    let Ok(client) = reqwest::Client::builder().use_rustls_tls().https_only(true).timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build() else { return };
    let sent = client
        .post(URL)
        .header(reqwest::header::USER_AGENT, "framecorder")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(sealed)
        .send()
        .await;
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
        let mut first = body("desktop", "0.2.0", &[("view", "grid".into()), ("paired", paired(4))], &clips, &Value::Null, 20_000, 20_000 * day as u64);
        // the machine it runs on goes in every ping
        let machine = first.as_object_mut().unwrap().remove("perf").unwrap();
        assert!(machine.as_object().unwrap().keys().any(|k| k.starts_with("cores=")) && machine.as_object().unwrap().keys().any(|k| k.starts_with("ram_gb=")));
        assert_eq!(
            first.to_string(),
            format!(r#"{{"age":"0","app":"desktop","counts":{{"synced":2,"synced_mb":42}},"new":true,"os":"{}","settings":{{"paired":"3+","view":"grid"}},"version":"0.2.0"}}"#, os())
        );
        // later pings count from the one before
        let state = json!({"last": "2026-11-13", "ever": true, "at": 20_000 * day - 45, "perf": {"app_mb@1d+=100-200": 30_000, "app_cpu=<1": 4}});
        let later = body("android", "0.2.0", &[], &clips, &state, 20_000, 20_045 * day as u64);
        assert_eq!((later["new"].clone(), later["age"].clone(), later["counts"]["synced"].clone()), (json!(false), json!("30-89"), json!(1)));
        // and what it noted since the last one, capped
        assert_eq!((later["perf"]["app_mb@1d+=100-200"].clone(), later["perf"]["app_cpu=<1"].clone()), (json!(10_000), json!(4)));
        assert_eq!([bucket(3.0, &[25, 50]), bucket(30.0, &[25, 50]), bucket(50.0, &[25, 50])], ["<25", "25-50", "50+"]);
        assert_eq!([up(10.0), up(4.0 * 3600.0), up(90_000.0)], ["<1h", "1-6h", "1d+"]);
        // this process can be read
        assert!(usage::memory_mb().is_some_and(|m| m > 0.0) && usage::cpu().is_some() && usage::ram_gb().is_some_and(|g| g > 0.5));
        assert_eq!(super::day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(super::day(951_782_400), "2000-02-29");
    }
}
