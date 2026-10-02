//! How the headset itself is doing, for the phone: its battery and how full
//! it is.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use serde_json::{json, Value};

use crate::config::Paths;
use crate::library::Library;

fn read(path: &Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

/// The headset's own battery, from the kernel. Batteries that belong to
/// something plugged in (a controller, say) say so with scope=Device.
fn battery(power: &Path) -> Value {
    let Ok(dir) = std::fs::read_dir(power) else { return Value::Null };
    let mut entries: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if read(&p.join("type")).as_deref() != Some("Battery") || read(&p.join("scope")).as_deref() == Some("Device") {
            continue;
        }
        let Some(percent) = read(&p.join("capacity")).and_then(|c| c.parse::<u8>().ok()) else { continue };
        let status = read(&p.join("status")).unwrap_or_default();
        return json!({ "percent": percent.min(100), "charging": status == "Charging" || status == "Full" });
    }
    Value::Null
}

/// Free and total bytes on the disk `dir` is on.
fn space(dir: &Path) -> Option<(u64, u64)> {
    let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let block = s.f_frsize as u64;
    Some((s.f_bavail as u64 * block, s.f_blocks as u64 * block))
}

pub fn about(paths: &Paths, library: &Library) -> Value {
    let storage = match space(&paths.videos) {
        Some((free, total)) => {
            let videos: u64 = library.list().iter().map(|c| c.size).sum();
            json!({ "free": free, "total": total, "videos": videos })
        }
        None => Value::Null,
    };
    json!({ "battery": battery(&paths.power), "storage": storage })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(dir: &Path, name: &str, files: &[(&str, &str)]) {
        let d = dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        for (f, v) in files {
            std::fs::write(d.join(f), format!("{v}\n")).unwrap();
        }
    }

    #[test]
    fn reads_the_headsets_battery_not_a_controllers() {
        let dir = tempfile::tempdir().unwrap();
        supply(dir.path(), "AC", &[("type", "Mains"), ("online", "1")]);
        supply(dir.path(), "controller", &[("type", "Battery"), ("scope", "Device"), ("capacity", "12"), ("status", "Discharging")]);
        supply(dir.path(), "battery", &[("type", "Battery"), ("capacity", "82"), ("status", "Charging")]);
        assert_eq!(battery(dir.path()), json!({ "percent": 82, "charging": true }));
    }

    #[test]
    fn no_battery_is_null() {
        let dir = tempfile::tempdir().unwrap();
        supply(dir.path(), "AC", &[("type", "Mains")]);
        assert_eq!(battery(dir.path()), Value::Null);
        assert_eq!(battery(&dir.path().join("missing")), Value::Null);
    }

    #[test]
    fn knows_how_full_the_disk_is() {
        let dir = tempfile::tempdir().unwrap();
        let (free, total) = space(dir.path()).unwrap();
        assert!(total > 0 && free <= total);
    }
}
