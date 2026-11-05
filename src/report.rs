use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::{json, Value};

const LOG_LIMIT: usize = 48 * 1024;
const RELAY: &str = "https://framecorder-report.coah80.com/v1/report";
const UNITS: [&str; 3] = ["framecorder-ui", "framecorder-sync", "framecorder-update"];

pub struct Sent {
    pub number: u64,
    pub url: String,
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(1024 * 1024).read_to_string(&mut text).ok()?;
    Some(text.trim_matches(['\0', '\n', '\r']).to_owned())
}

fn tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let size = file.metadata().ok()?.len();
    let start = size.saturating_sub(LOG_LIMIT as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(LOG_LIMIT as u64).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(
        if start > 0 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            &text
        }
        .to_owned(),
    )
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("timeout");
    cmd.args(["--signal=KILL", "0.25s", program])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let runtime = format!("/run/user/{}", unsafe { libc::getuid() });
    if Path::new(&runtime).join("bus").exists() {
        cmd.env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={runtime}/bus"),
        )
        .env("XDG_RUNTIME_DIR", runtime);
    }
    cmd.stdout(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()?
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes);
    let status = child.wait().ok()?;
    read.ok()?;
    if !status.success() && program != "systemctl" {
        return None;
    }
    if status.code().is_none() || status.code() == Some(124) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text.trim_end().to_owned())
    }
}

fn pairs(text: &str, separator: char) -> Value {
    let mut map = serde_json::Map::new();
    for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
        if let Some((key, value)) = line.split_once(separator) {
            map.insert(
                key.trim().into(),
                json!(value.trim().trim_matches(['"', '\''])),
            );
        }
    }
    Value::Object(map)
}

fn meminfo(text: &str) -> Value {
    let fields = pairs(text, ':');
    let kb = |key: &str| {
        fields[key]
            .as_str()
            .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok())
    };
    json!({"ram_total_mb": kb("MemTotal").map(|v| v / 1024),
        "ram_available_mb": kb("MemAvailable").map(|v| v / 1024),
        "swap_total_mb": kb("SwapTotal").map(|v| v / 1024),
        "swap_used_mb": kb("SwapTotal").zip(kb("SwapFree")).map(|(total, free)| total.saturating_sub(free) / 1024)})
}

fn vdf(text: &str) -> Option<Value> {
    fn token(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
        loop {
            let c = chars.next()?;
            if c.is_whitespace() {
                continue;
            }
            if c == '/' && chars.peek() == Some(&'/') {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            if c == '{' || c == '}' {
                return Some(c.to_string());
            }
            if c == '"' {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' {
                        return Some(s);
                    }
                    if c == '\\' {
                        s.push(chars.next()?);
                    } else {
                        s.push(c);
                    }
                }
                return None;
            }
            let mut s = c.to_string();
            while chars
                .peek()
                .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
            {
                s.push(chars.next()?);
            }
            return Some(s);
        }
    }
    fn object(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, depth: usize) -> Option<Value> {
        if depth > 16 {
            return None;
        }
        let mut map = serde_json::Map::new();
        while let Some(key) = token(chars) {
            if key == "}" {
                return (depth > 0).then_some(Value::Object(map));
            }
            let value = token(chars)?;
            map.insert(
                key.to_ascii_lowercase(),
                if value == "{" {
                    object(chars, depth + 1)?
                } else if value == "}" {
                    return None;
                } else {
                    json!(value)
                },
            );
        }
        (depth == 0).then_some(Value::Object(map))
    }
    object(&mut text.chars().peekable(), 0)
}

fn persona(users: &Value) -> Value {
    let Some(users) = users["users"].as_object() else {
        return Value::Null;
    };
    // the frame's steam doesn't write MostRecent, so fall back to whoever
    // signed in last
    let signed_in = |u: &&Value| u["timestamp"].as_str().and_then(|t| t.parse::<u64>().ok());
    users
        .values()
        .find(|u| u["mostrecent"] == "1")
        .or_else(|| users.values().filter(|u| signed_in(u).is_some()).max_by_key(signed_in))
        .map(|u| u["personaname"].clone())
        .unwrap_or(Value::Null)
}

fn steamvr(manifest: &Value) -> Value {
    let app = &manifest["appstate"];
    let branch = app["userconfig"]["betakey"]
        .as_str()
        .or_else(|| app["mountedconfig"]["betakey"].as_str());
    json!({"steamvr_version": app["buildid"], "steamvr_branch": app.is_object().then_some(branch.filter(|s| !s.is_empty()).unwrap_or("public"))})
}

#[path = "../report/src/shared.rs"]
mod shared;

static MAC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b").expect("mac regex"));
static SERIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:LHR|LHB|HMD|WM|FPR)-[a-z0-9-]+\b|(?i:serial(?:number| number|_number)?)[\s\"':=]+[^\s,;\"']+"#).expect("serial regex")
});
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)token|authorization|pairing.?code|certificate|private.?key|password|accountname|account name|BEGIN .* KEY").expect("secret regex")
});

/// SteamOS's own user names, the same on every headset, so they identify nobody
/// and hiding them would mangle every "steamos" in the logs.
const SHARED_USERS: [&str; 2] = ["steamos", "deck"];

fn redact(text: &str, home: &str, user: &str, accounts: &[String]) -> String {
    let mut pem = false;
    let mut text = text
        .lines()
        .map(|line| {
            if line.contains("-----BEGIN ") {
                pem = true;
            }
            let hidden = pem || SECRET.is_match(line);
            if line.contains("-----END ") {
                pem = false;
            }
            if hidden {
                "<redacted>"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !home.is_empty() {
        text = text.replace(home, "~");
    }
    if !user.is_empty() && !SHARED_USERS.contains(&user) {
        let pattern = format!(r"\b{}\b", regex::escape(user));
        if let Ok(re) = Regex::new(&pattern) {
            text = re.replace_all(&text, "<user>").into_owned();
        }
    }
    for account in accounts.iter().filter(|s| !s.is_empty()) {
        text = text.replace(account, "<user>");
    }
    text = MAC.replace_all(&text, "<mac>").into_owned();
    text = SERIAL.replace_all(&text, "<serial>").into_owned();
    shared::hide(&text)
}

fn scrub(value: &mut Value, home: &str, user: &str, accounts: &[String]) {
    match value {
        Value::String(s) => *s = redact(s, home, user, accounts),
        Value::Array(a) => a.iter_mut().for_each(|v| scrub(v, home, user, accounts)),
        Value::Object(o) => o.values_mut().for_each(|v| scrub(v, home, user, accounts)),
        _ => (),
    }
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) }
}

fn timestamp(seconds: i64) -> String {
    let mut tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::gmtime_r(&seconds, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn line_time(line: &str) -> Option<i64> {
    let text = std::ffi::CString::new(line.trim_start_matches('[')).ok()?;
    for format in [
        c"%Y-%m-%dT%H:%M:%S",
        c"%Y-%m-%d %H:%M:%S",
        c"%a %b %d %Y %H:%M:%S",
    ] {
        let mut tm = unsafe { std::mem::zeroed() };
        let end = unsafe { libc::strptime(text.as_ptr(), format.as_ptr(), &mut tm) };
        if end.is_null() {
            continue;
        }
        let rest = unsafe { std::ffi::CStr::from_ptr(end) }.to_str().ok()?;
        let rest = rest
            .trim_start_matches('.')
            .trim_start_matches(|c: char| c.is_ascii_digit());
        if rest.starts_with('Z') {
            return Some(unsafe { libc::timegm(&mut tm) });
        }
        if rest.starts_with('+') || rest.starts_with('-') {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let digits: String = rest[1..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == ':')
                .filter(|c| *c != ':')
                .collect();
            if digits.len() == 4 {
                let hours = digits[..2].parse::<i64>().ok()?;
                let minutes = digits[2..].parse::<i64>().ok()?;
                if hours <= 23 && minutes <= 59 {
                    return Some(
                        unsafe { libc::timegm(&mut tm) } - sign * (hours * 3600 + minutes * 60),
                    );
                }
            }
        }
        tm.tm_isdst = -1;
        return Some(unsafe { libc::mktime(&mut tm) });
    }
    None
}

fn recent_errors(logs: &[(String, String)], now: i64) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, text) in logs {
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            if ![
                "error", "failed", "panic", "warn", "cannot", "denied", "killed", "segfault",
            ]
            .iter()
            .any(|word| lower.contains(word))
            {
                continue;
            }
            if let Some(time) =
                line_time(line).filter(|time| *time >= now - 48 * 3600 && *time <= now + 60)
            {
                errors.push((time, format!("{name}: {line}")));
            }
        }
    }
    errors.sort_by_key(|(time, _)| *time);
    errors
        .into_iter()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(_, line)| line)
        .collect()
}

fn free_mb(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = unsafe { std::mem::zeroed::<libc::statvfs>() };
    (unsafe { libc::statvfs(path.as_ptr(), &mut stat) } == 0)
        .then(|| stat.f_bavail.saturating_mul(stat.f_frsize) / 1024 / 1024)
}

pub fn collect() -> Value {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("framecorder");
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("framecorder");
    let steam = home.join(".local/share/Steam");
    let users = read(steam.join("config/loginusers.vdf"))
        .and_then(|s| vdf(&s))
        .unwrap_or_default();
    let accounts: Vec<String> = users["users"]
        .as_object()
        .map(|o| {
            o.values()
                .filter_map(|u| u["accountname"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let manifest = read(steam.join("steamapps/appmanifest_250820.acf"))
        .and_then(|s| vdf(&s))
        .unwrap_or_default();
    let mut steam_info = steamvr(&manifest);
    if let Some(version) = read(steam.join("steamapps/common/SteamVR/bin/version.txt")) {
        steam_info["steamvr_version"] = json!(version);
    } else if let Some(version) = read("/opt/steamvr/bin/version.txt") {
        // on the frame steamvr comes with steamos, not from steam
        steam_info["steamvr_version"] = json!(version);
        steam_info["steamvr_branch"] = json!("comes with SteamOS");
    }
    let os = read("/etc/os-release")
        .map(|s| pairs(&s, '='))
        .unwrap_or_default();
    let cpu = read("/proc/cpuinfo").unwrap_or_default();
    let cpu_fields = pairs(&cpu, ':');
    let mut device = read("/proc/meminfo")
        .map(|s| meminfo(&s))
        .unwrap_or_else(|| json!({}));
    device["model"] = json!(read("/sys/firmware/devicetree/base/model")
        .or_else(|| read("/sys/class/dmi/id/product_name")));
    device["cpu"] = cpu_fields["model name"]
        .as_str()
        .or(cpu_fields["Hardware"].as_str())
        .map_or(Value::Null, |s| json!(s));
    device["cores"] = if cpu.is_empty() {
        Value::Null
    } else {
        json!(cpu
            .lines()
            .filter(|l| l
                .split_once(':')
                .is_some_and(|(k, _)| k.trim() == "processor"))
            .count())
    };
    device["disk_free_mb"] = json!(free_mb(&home.join("Videos")));
    device["uptime_s"] = json!(read("/proc/uptime")
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64));
    device["battery_percent"] =
        json!(std::fs::read_dir("/sys/class/power_supply")
            .ok()
            .and_then(|dirs| dirs.flatten().find_map(|e| {
                (read(e.path().join("type")).as_deref() == Some("Battery")
                    && read(e.path().join("scope")).as_deref() != Some("Device"))
                .then(|| read(e.path().join("capacity"))?.parse::<u64>().ok())
                .flatten()
            })));
    device["refresh_hz"] = Value::Null;
    let helper = crate::grab::helper_path();
    let helper_state = if helper.as_ref().is_none_or(|p| !p.exists()) {
        "missing"
    } else if crate::grab::helper_ready() {
        "ok"
    } else {
        "no permission"
    };
    let mut services = json!({});
    for unit in [
        "framecorder-ui",
        "framecorder-sync",
        "framecorder-update.timer",
    ] {
        services[unit] = json!(command("systemctl", &["--user", "is-active", unit]));
    }
    let clients =
        read(config.join("sync/clients.json")).and_then(|s| serde_json::from_str::<Value>(&s).ok());
    // what this headset and the devices paired with it are called can carry
    // someone's name ("Cole's S25 Ultra"), and so can the Steam name
    let mut names: Vec<String> = clients
        .iter()
        .flat_map(|c| c["devices"].as_array().into_iter().flatten())
        .filter_map(|d| d["name"].as_str().map(String::from))
        .collect();
    names.extend(
        read(config.join("sync/settings.json"))
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["name"].as_str().map(String::from)),
    );
    names.extend(read("/etc/hostname"));
    names.extend(persona(&users).as_str().map(String::from));
    let sync = clients.as_ref().and_then(|c| c["devices"].as_array()).map(|devices| json!({"paired": devices.len(), "apps": devices.iter().filter_map(|d| d["app"].as_str()).collect::<Vec<_>>()}));
    let mut logs = Vec::new();
    for (name, path) in [
        ("recorder.log", state.join("recorder.log")),
        ("vrserver.txt", steam.join("logs/vrserver.txt")),
        ("vrcompositor.txt", steam.join("logs/vrcompositor.txt")),
    ] {
        if let Some(text) = tail(&path) {
            logs.push((name.to_owned(), text));
        }
    }
    if let Some(journal) = command(
        "journalctl",
        &[
            "--user",
            "--no-pager",
            "--since=-48h",
            "--output=json",
            "--output-fields=MESSAGE,__REALTIME_TIMESTAMP,_SYSTEMD_USER_UNIT",
            "-n",
            "1200",
            "-u",
            "framecorder-ui",
            "-u",
            "framecorder-sync",
            "-u",
            "framecorder-update",
        ],
    ) {
        for unit in UNITS {
            let text = journal
                .lines()
                .filter_map(|line| {
                    let entry: Value = serde_json::from_str(line).ok()?;
                    if entry["_SYSTEMD_USER_UNIT"].as_str()? != format!("{unit}.service") {
                        return None;
                    }
                    let seconds = entry["__REALTIME_TIMESTAMP"]
                        .as_str()?
                        .parse::<i64>()
                        .ok()?
                        / 1_000_000;
                    Some(format!(
                        "{} {}",
                        timestamp(seconds),
                        entry["MESSAGE"].as_str()?
                    ))
                })
                .collect::<Vec<_>>()
                .join("\n");
            let mut start = text.len().saturating_sub(LOG_LIMIT);
            while !text.is_char_boundary(start) {
                start += 1;
            }
            logs.push((format!("{unit}.journal"), text[start..].to_owned()));
        }
    }
    let last_update = read(config.join("sync/update.json"))
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["failed"].as_str().map(String::from));
    let time = now();
    let mut result = json!({
        "collected_at": timestamp(time),
        "framecorder": {"version": env!("CARGO_PKG_VERSION"), "channel": if home.join(".local/share/framecorder/source").exists() { "dev" } else { "release" },
            "unlocked": crate::setup::panels_unlocked(), "helper": helper_state, "services": services,
            "last_update": last_update},
        "steam": steam_info,
        "os": {"name": os["NAME"], "version": os["VERSION_ID"], "build_id": os["BUILD_ID"], "branch": command("steamos-select-branch", &["-c"]), "kernel": command("uname", &["-r"])},
        "device": device,
        "settings": read(config.join("ui.conf")).map(|s| pairs(&s, '=')),
        "sync": sync,
        "recent_errors": recent_errors(&logs, time),
        "logs": logs.into_iter().map(|(name, text)| json!({"name": name, "text": text})).collect::<Vec<_>>()
    });
    scrub(
        &mut result,
        &home.to_string_lossy(),
        &std::env::var("USER").unwrap_or_default(),
        &accounts,
    );
    hide_names(&mut result, &names);
    if let Some(logs) = result["logs"].as_array_mut() {
        for log in logs {
            if let Some(text) = log["text"].as_str() {
                let mut start = text.len().saturating_sub(LOG_LIMIT);
                while !text.is_char_boundary(start) {
                    start += 1;
                }
                log["text"] = json!(&text[start..]);
            }
        }
    }
    result
}

fn hide_names(value: &mut Value, names: &[String]) {
    match value {
        Value::String(s) => *s = shared::hide_names(s, names),
        Value::Array(a) => a.iter_mut().for_each(|v| hide_names(v, names)),
        Value::Object(o) => o.values_mut().for_each(|v| hide_names(v, names)),
        _ => (),
    }
}

pub fn send(description: &str, diagnostics: &Value) -> Result<Sent> {
    let description = description.trim();
    if !(1..=4000).contains(&description.chars().count()) {
        bail!("describe what went wrong in 1 to 4000 characters");
    }
    let body = serde_json::to_vec(
        &json!({"version": 1, "source": "headset", "description": description,
        "app": null, "headset": diagnostics, "headset_unavailable": null, "app_logs": []}),
    )?;
    let url = std::env::var("FRAMECORDER_REPORT_URL").unwrap_or_else(|_| RELAY.into());
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "20",
            "--connect-timeout",
            "5",
            "--proto",
            "=http,https",
            "-H",
            "Content-Type: application/json",
            "-A",
            concat!("framecorder-headset/", env!("CARGO_PKG_VERSION")),
            "--data-binary",
            "@-",
            &url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't reach the issue service")?;
    let written = child
        .stdin
        .take()
        .context("couldn't write the report")?
        .write_all(&body);
    let out = child
        .wait_with_output()
        .context("couldn't send the report")?;
    written.context("couldn't send the report")?;
    let reply: Value = serde_json::from_slice(&out.stdout)
        .context("the issue service didn't return an answer, try again later")?;
    if reply["ok"] != true {
        bail!(
            "{}",
            reply["error"]
                .as_str()
                .unwrap_or("the issue service couldn't send the report, try again later")
        );
    }
    if !out.status.success() {
        bail!("couldn't reach the issue service, check your connection and try again");
    }
    Ok(Sent {
        number: reply["number"]
            .as_u64()
            .context("the issue service didn't return an issue number")?,
        url: reply["url"]
            .as_str()
            .context("the issue service didn't return an issue link")?
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_keep_only_the_requested_steam_identity_and_build() {
        for (text, expected) in [
            (
                r#""users" { "76561190000000001" { "AccountName" "private_login" "PersonaName" "Older" "MostRecent" "0" } "76561190000000002" { "AccountName" "secret_login" "PersonaName" "My \"name\"" "MostRecent" "1" } }"#,
                json!("My \"name\""),
            ),
            (
                r#""users" { "123" { "PersonaName" "Inactive" "MostRecent" "0" } }"#,
                Value::Null,
            ),
            (
                r#""users" { "1" { "AccountName" "a" "PersonaName" "Before" "timestamp" "1700000000" } "2" { "AccountName" "b" "PersonaName" "Frame" "timestamp" "1790000000" } }"#,
                json!("Frame"),
            ),
            ("broken {", Value::Null),
        ] {
            assert_eq!(persona(&vdf(text).unwrap_or_default()), expected);
        }
        for (text, version, branch) in [
            (
                r#""AppState" { "appid" "250820" "buildid" "20304050" "UserConfig" { "BetaKey" "beta" } }"#,
                json!("20304050"),
                json!("beta"),
            ),
            (
                r#""AppState" { "buildid" "456" "MountedConfig" { "BetaKey" "previous" } }"#,
                json!("456"),
                json!("previous"),
            ),
            (
                r#""AppState" { "buildid" "789" }"#,
                json!("789"),
                json!("public"),
            ),
            ("", Value::Null, Value::Null),
        ] {
            let info = steamvr(&vdf(text).unwrap_or_default());
            assert_eq!(info["steamvr_version"], version);
            assert_eq!(info["steamvr_branch"], branch);
        }
        for (text, version, build) in [
            (
                "NAME=SteamOS\nVERSION_ID=3.7.0\nBUILD_ID=20261020.1000\n",
                json!("3.7.0"),
                json!("20261020.1000"),
            ),
            (
                "NAME=\"SteamOS\"\nVERSION_ID='3.8'\n#BUILD_ID=hidden",
                json!("3.8"),
                Value::Null,
            ),
            ("garbage", Value::Null, Value::Null),
        ] {
            let os = pairs(text, '=');
            assert_eq!(os["VERSION_ID"], version);
            assert_eq!(os["BUILD_ID"], build);
        }
        for (text, expected) in [
            ("MemTotal:       16384000 kB\nMemAvailable: 4096000 kB\nSwapTotal: 8192000 kB\nSwapFree: 6144000 kB\n", json!({"ram_total_mb":16000,"ram_available_mb":4000,"swap_total_mb":8000,"swap_used_mb":2000})),
            ("MemTotal: broken\n", json!({"ram_total_mb":null,"ram_available_mb":null,"swap_total_mb":null,"swap_used_mb":null})),
        ] { assert_eq!(meminfo(text), expected); }
    }

    #[test]
    fn public_reports_redact_private_details() {
        for (input, expected) in [
            ("/home/player/Videos/framecorder/2026.mp4 player", "~/Videos/framecorder/2026.mp4 <user>"),
            ("8.8.8.8 192.168.1.2 172.16.0.1 10.0.0.2 127.0.0.1", "<ip> <ip> <ip> <ip> 127.0.0.1"),
            ("[2001:4860:4860::8888] ::1 fe80::1", "[<ip>] ::1 <ip>"),
            // what the old address pattern mangled
            ("framecorder_sync::server] trackpad.position", "framecorder_sync::server] trackpad.position"),
            ("device aa:bb:cc:dd:ee:ff LHR-1234ABCD serial=F12345", "device <mac> <serial> <serial>"),
            ("Authorization: Bearer fake\nwarn pairing_code=123456\nfailed token=synthetic\nsafe", "<redacted>\n<redacted>\n<redacted>\nsafe"),
            ("private_login signed in", "<user> signed in"),
            ("8.8.8.8:443 1.1.1.1.", "<ip>:443 <ip>."),
            ("-----BEGIN PRIVATE KEY-----\nsynthetic-key-data\n-----END PRIVATE KEY-----\nsafe", "<redacted>\n<redacted>\n<redacted>\nsafe"),
        ] { assert_eq!(redact(input, "/home/player", "player", &["private_login".into()]), expected); }
        assert_eq!(
            redact("/home/steamos/x on steamos 3.8", "/home/steamos", "steamos", &[]),
            "~/x on steamos 3.8"
        );
    }

    #[test]
    fn errors_are_recent_sorted_source_prefixed_and_bounded() {
        let time = line_time("2026-11-04T12:00:00Z").unwrap();
        for stamp in [
            "2026-11-04T14:00:00+02:00",
            "2026-11-04T07:00:00.123-0500",
            "2026-11-04T12:00:00.001Z",
        ] {
            assert_eq!(line_time(stamp), Some(time));
        }
        let logs = vec![("recorder.log".into(), "2026-11-01T12:00:00Z ERROR old\n2026-11-04T11:59:00Z WARN encoder\nERROR no timestamp\n2026-11-04T12:00:00Z info normal".into()),
            ("vrserver.txt".into(), "Tue Nov 03 2026 12:00:00.100 - failed display\n2026-11-05T12:00:00Z ERROR future".into())];
        assert_eq!(
            recent_errors(&logs, time),
            vec![
                "vrserver.txt: Tue Nov 03 2026 12:00:00.100 - failed display",
                "recorder.log: 2026-11-04T11:59:00Z WARN encoder"
            ]
        );
        let lines = (0..250)
            .map(|n| format!("2026-11-04T11:59:00Z ERROR {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let errors = recent_errors(&[("test".into(), lines)], time);
        assert_eq!(errors.len(), 200);
        assert!(errors[0].ends_with(" 50"));
        assert!(errors[199].ends_with(" 249"));
    }
}
