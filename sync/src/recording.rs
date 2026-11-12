//! The recording settings, which live in the dashboard tab's `ui.conf`.
//! Paired phones can read and change them; the tab notices and uses them
//! from the next recording on.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::config::write_atomic;

#[path = "../../src/mics.rs"]
mod mics;

static CHANGES: Mutex<()> = Mutex::new(());
/// The mics, looked up at most this often: pw-dump takes ~30 ms.
const MICS_FOR: Duration = Duration::from_secs(3);
static MICS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

/// What a phone may change, with the tab's defaults. `clip` is seconds.
const KEYS: [(&str, &str); 14] = [
    ("shape", "wide"),
    ("eye", "left"),
    ("quality", "high"),
    ("fps", "auto"),
    ("codec", "hevc"),
    ("format", "mp4"),
    // recordings in mkv come out as mp4 once they're done
    ("remux", "false"),
    // a framecorder folder on a drive, empty for ~/Videos/framecorder
    ("folder", ""),
    ("game_audio", "true"),
    ("mic", "true"),
    // a PipeWire node name, empty for the one SteamOS uses
    ("mic_source", ""),
    ("split_audio", "true"),
    ("clips", "true"),
    ("clip", "30"),
];

fn allowed(key: &str, value: &str) -> bool {
    match key {
        "eye" => matches!(value, "left" | "right" | "both"),
        "shape" => matches!(value, "wide" | "square" | "tall" | "both"),
        "quality" => matches!(value, "standard" | "high" | "max"),
        "fps" => matches!(value, "auto" | "60" | "30"),
        "codec" => matches!(value, "hevc" | "h264"),
        "format" => matches!(value, "mp4" | "mkv"),
        "game_audio" | "mic" | "split_audio" | "clips" | "remux" => matches!(value, "true" | "false"),
        "clip" => matches!(value, "15" | "30" | "60" | "120"),
        "mic_source" => value.is_empty() || mics::valid_name(value),
        // a phone picks from what's plugged in
        "folder" => value.is_empty() || crate::folder::drives().iter().any(|d| d.folder.as_os_str() == value),
        _ => false,
    }
}

/// What the tab takes from the file. A picked drive that's been taken out
/// is still the pick, it just isn't in.
fn readable(key: &str, value: &str) -> bool {
    match key {
        "folder" => crate::folder::valid(value),
        _ => allowed(key, value),
    }
}

/// The folder picked in the tab, empty for ~/Videos/framecorder.
pub fn folder(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    lookup(&text, "folder").filter(|v| crate::folder::valid(v)).unwrap_or_default().to_string()
}

/// The last value for `key`, the way the tab reads the file.
fn lookup<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
        .next_back()
}

fn to_json(key: &str, value: &str) -> Value {
    match key {
        "game_audio" | "mic" | "split_audio" | "clips" | "remux" => json!(value == "true"),
        "clip" => json!(value.parse::<u32>().unwrap_or(30)),
        _ => json!(value),
    }
}

/// All of them, as the tab would use them right now.
pub fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = Map::new();
    for (key, default) in KEYS {
        let value = lookup(&text, key).filter(|v| readable(key, v)).unwrap_or(default);
        out.insert(key.into(), to_json(key, value));
    }
    // older configs turned clips off with the length
    if lookup(&text, "clip") == Some("off") && lookup(&text, "clips").is_none() {
        out.insert("clips".into(), json!(false));
    }
    Value::Object(out)
}

pub fn change(path: &Path, changes: &Value) -> Result<Value, String> {
    let _guard = CHANGES.lock().unwrap();
    let Some(changes) = changes.as_object().filter(|c| !c.is_empty()) else {
        return Err("expected an object with some of eye, shape, quality, fps, codec, format, remux, folder, game_audio, mic, mic_source, split_audio, clips, clip".into());
    };
    let mut updates: Vec<(String, String)> = Vec::new();
    for (key, v) in changes {
        let value = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        if !allowed(key, &value) {
            return Err(format!("{key} can't be {v}"));
        }
        updates.push((key.clone(), value));
    }

    let existing = std::fs::read_to_string(path).ok();
    let text = existing.as_deref().unwrap_or("");
    let changing = |line: &str| {
        line.split_once('=')
            .is_some_and(|(k, _)| updates.iter().any(|(u, _)| u == k.trim()))
    };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !changing(l))
        .map(String::from)
        .collect();
    // a new length replaces "clip=off", which also meant clips were off
    if lookup(text, "clip") == Some("off") && lookup(text, "clips").is_none() && !updates.iter().any(|(k, _)| k == "clips") {
        lines.push("clips=false".into());
    }
    // without a file the tab hasn't been set up yet, and should still offer it
    if existing.is_none() {
        lines.push("onboarded=false".into());
    }
    lines.extend(updates.iter().map(|(k, v)| format!("{k}={v}")));

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't save them: {e}"))?;
    }
    write_atomic(path, (lines.join("\n") + "\n").as_bytes()).map_err(|e| format!("couldn't save them: {e}"))?;
    Ok(read(path))
}

/// The mics there are, for picking one, the Frame's own first.
fn mics() -> Value {
    let mut cached = MICS.lock().unwrap();
    if let Some((_, mics)) = cached.as_ref().filter(|(at, _)| at.elapsed() < MICS_FOR) {
        return mics.clone();
    }
    let mics: Value = mics::list().into_iter().map(|m| json!({"name": m.name, "label": m.label, "default": m.default})).collect();
    *cached = Some((Instant::now(), mics.clone()));
    mics
}

/// Where recordings can go: Videos, the drives plugged in, and a picked
/// one that's been taken out.
fn folders(picked: &str) -> Value {
    let mut list = vec![json!({"path": "", "name": "Videos", "plugged_in": true})];
    let drives = crate::folder::drives();
    list.extend(drives.iter().map(|d| json!({"path": d.folder, "name": d.name, "plugged_in": true})));
    if !picked.is_empty() && !drives.iter().any(|d| d.folder.as_os_str() == picked) {
        list.push(json!({"path": picked, "name": crate::folder::name_of(picked), "plugged_in": false}));
    }
    Value::Array(list)
}

pub fn status(paths: &crate::config::Paths) -> Value {
    let mut settings = read(&paths.recording());
    settings["mics"] = mics();
    settings["folders"] = folders(settings["folder"].as_str().unwrap_or_default());
    settings["panels"] = std::fs::read(paths.status())
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
        .and_then(|status| status["panels"].as_bool())
        .unwrap_or(true)
        .into();
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = read(&dir.path().join("ui.conf"));
        assert_eq!(
            s,
            json!({"shape": "wide", "eye": "left", "quality": "high", "fps": "auto", "codec": "hevc", "format": "mp4", "remux": false, "folder": "", "game_audio": true, "mic": true, "mic_source": "", "split_audio": true, "clips": true, "clip": 30})
        );
    }

    #[test]
    fn blended_eye_persists_and_can_be_changed_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.conf");
        for eye in ["both", "right", "left"] {
            let changed = change(&path, &json!({"eye": eye})).unwrap();
            assert_eq!(changed["eye"], eye);
            assert_eq!(read(&path)["eye"], eye);
        }
    }

    #[test]
    fn changes_keep_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.conf");
        std::fs::write(&path, "shape=wide\neye=right\nquality=high\nclip=30\nonboarded=true\n").unwrap();

        let s = change(&path, &json!({"shape": "tall", "clip": 60, "mic": false})).unwrap();
        assert_eq!((s["shape"].as_str(), s["clip"].as_u64(), s["mic"].as_bool()), (Some("tall"), Some(60), Some(false)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("eye=right") && text.contains("onboarded=true"));
        assert_eq!(text.matches("shape=").count(), 1);
    }

    #[test]
    fn refuses_what_the_tab_wouldnt_take() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.conf");
        assert!(change(&path, &json!({"clip": 45})).is_err());
        assert!(change(&path, &json!({"shape": "round"})).is_err());
        assert!(change(&path, &json!({"eye": "middle"})).is_err());
        assert!(change(&path, &json!({"codec": "av1"})).is_err());
        assert!(change(&path, &json!({"format": "avi"})).is_err());
        assert!(change(&path, &json!({})).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn a_first_change_still_leaves_the_setup_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.conf");
        change(&path, &json!({"quality": "max"})).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("onboarded=false"));
    }

    #[test]
    fn old_clip_off_stays_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui.conf");
        std::fs::write(&path, "clip=off\n").unwrap();
        assert_eq!(read(&path)["clips"], json!(false));
        let s = change(&path, &json!({"clip": 15})).unwrap();
        assert_eq!((s["clips"].as_bool(), s["clip"].as_u64()), (Some(false), Some(15)));
    }
}
