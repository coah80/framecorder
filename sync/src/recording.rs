//! The recording settings, which live in the dashboard tab's `ui.conf`.
//! Paired phones can read and change them; the tab notices and uses them
//! from the next recording on.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::config::write_atomic;

/// What a phone may change, with the tab's defaults. `clip` is seconds.
const KEYS: [(&str, &str); 7] = [
    ("shape", "wide"),
    ("quality", "high"),
    ("fps", "auto"),
    ("game_audio", "true"),
    ("mic", "true"),
    ("clips", "true"),
    ("clip", "30"),
];

fn allowed(key: &str, value: &str) -> bool {
    match key {
        "shape" => matches!(value, "wide" | "square" | "tall" | "both"),
        "quality" => matches!(value, "standard" | "high" | "max"),
        "fps" => matches!(value, "auto" | "60" | "30"),
        "game_audio" | "mic" | "clips" => matches!(value, "true" | "false"),
        "clip" => matches!(value, "15" | "30" | "60" | "120"),
        _ => false,
    }
}

/// The last value for `key`, the way the tab reads the file.
fn lookup<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim())
        .last()
}

fn to_json(key: &str, value: &str) -> Value {
    match key {
        "game_audio" | "mic" | "clips" => json!(value == "true"),
        "clip" => json!(value.parse::<u32>().unwrap_or(30)),
        _ => json!(value),
    }
}

/// All of them, as the tab would use them right now.
pub fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = Map::new();
    for (key, default) in KEYS {
        let value = lookup(&text, key).filter(|v| allowed(key, v)).unwrap_or(default);
        out.insert(key.into(), to_json(key, value));
    }
    // older configs turned clips off with the length
    if lookup(&text, "clip") == Some("off") && lookup(&text, "clips").is_none() {
        out.insert("clips".into(), json!(false));
    }
    Value::Object(out)
}

/// Sets any of them and writes the file, keeping the lines it doesn't know
/// (the eye, whether the setup's done). Returns them all, or what was wrong.
pub fn change(path: &Path, changes: &Value) -> Result<Value, String> {
    let Some(changes) = changes.as_object().filter(|c| !c.is_empty()) else {
        return Err("expected an object with some of shape, quality, fps, game_audio, mic, clips, clip".into());
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
    let changing = |line: &str| line.split_once('=').is_some_and(|(k, _)| updates.iter().any(|(u, _)| u == k.trim()));
    let mut lines: Vec<String> = text.lines().filter(|l| !changing(l)).map(String::from).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = read(&dir.path().join("ui.conf"));
        assert_eq!(s, json!({"shape": "wide", "quality": "high", "fps": "auto", "game_audio": true, "mic": true, "clips": true, "clip": 30}));
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
        assert!(change(&path, &json!({"eye": "left"})).is_err());
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
