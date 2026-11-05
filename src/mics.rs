//! The mics a recording can use: the Frame's own, and any plugged in over USB
//! or connected over Bluetooth. SteamOS puts a loopback in front of every mic
//! and lets apps record only from that (`/etc/wireplumber/wireplumber.conf.d/
//! 10-access.conf`): the raw mic, and the EQ, echo cancelling and noise
//! suppression stages behind the Frame's own, refuse a link. So those don't count.
//!
//! Read from `pw-dump`, which takes ~30 ms on the Frame, so only while
//! someone's picking. Shared with framecorder-sync.

use std::process::Command;

use serde_json::Value;

/// The loopback SteamOS records the Frame's own mic through.
const FRAME_MIC: &str = "alsa_loopback_device.alsa_input.platform-sound.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mic {
    /// PipeWire's node name, what the recorder's `--mic-source` takes.
    pub name: String,
    pub label: String,
    /// What SteamOS records from when nothing's picked.
    pub default: bool,
}

/// The mics there are right now, the Frame's own first. Empty when PipeWire
/// can't be asked.
pub fn list() -> Vec<Mic> {
    match Command::new("pw-dump").output() {
        Ok(out) if out.status.success() => parse(&String::from_utf8_lossy(&out.stdout)),
        Ok(out) => {
            log::warn!("pw-dump failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            Vec::new()
        }
        Err(e) => {
            log::warn!("couldn't run pw-dump: {e}");
            Vec::new()
        }
    }
}

/// The mics in a `pw-dump`.
pub fn parse(dump: &str) -> Vec<Mic> {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(dump) else { return Vec::new() };
    let default = objects
        .iter()
        .filter(|o| o["props"]["metadata.name"] == "default")
        .flat_map(|o| o["metadata"].as_array().into_iter().flatten())
        .find(|m| m["key"] == "default.audio.source")
        .and_then(|m| m["value"]["name"].as_str())
        .map(String::from);
    let sources: Vec<&Value> = objects.iter().map(|o| &o["info"]["props"]).filter(|p| p["media.class"] == "Audio/Source").collect();
    let loopback = |p: &Value| p["alsa.loopback"] == true || p["bluez5.loopback"] == true;
    // a bluetooth headset's own node, next to the loopback SteamOS made for it
    let wrapped: Vec<&Value> = sources.iter().filter(|p| loopback(p)).map(|p| &p["device.id"]).filter(|id| !id.is_null()).collect();
    let usable = |p: &Value| {
        let device = matches!(p["device.api"].as_str(), Some("alsa" | "bluez5")) && !wrapped.contains(&&p["device.id"]);
        (loopback(p) || device) && p["node.virtual"] != true && p["node.create-loopback"] != true && p["steamos.mic_filter"] != true
    };
    let mut mics: Vec<Mic> = sources
        .iter()
        .filter(|p| usable(p))
        .filter_map(|p| {
            let name = p["node.name"].as_str()?;
            let label = if name.starts_with(FRAME_MIC) {
                "Frame"
            } else {
                p["node.description"].as_str().or(p["node.nick"].as_str()).unwrap_or(name)
            };
            Some(Mic { default: default.as_deref() == Some(name), name: name.to_string(), label: label.to_string() })
        })
        .collect();
    mics.sort_by_key(|m| (m.label != "Frame", m.label.clone()));
    mics
}

/// Whether a name could be a PipeWire node's: what's allowed into the
/// settings file and onto the recorder's command line.
pub fn valid_name(name: &str) -> bool {
    name.len() <= 200
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The Frame's own chain as its pw-dump shows it, plus a USB mic and a
    /// Bluetooth headset, each behind the loopback SteamOS makes for it.
    #[test]
    fn only_mics_apps_may_record_count_and_the_frame_comes_first() {
        let node = |name: &str, description: &str, extra: Value| {
            let mut props = json!({"media.class": "Audio/Source", "node.name": name, "node.description": description});
            props.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            json!({"type": "PipeWire:Interface:Node", "info": {"props": props}})
        };
        let filter = || json!({"node.virtual": true, "steamos.mic_filter": true});
        let raw = |id: u32| json!({"device.api": "alsa", "device.id": id, "node.create-loopback": true});
        let frame = "alsa_loopback_device.alsa_input.platform-sound.HiFi__Mic__source";
        let yeti = "alsa_loopback_device.alsa_input.usb-Blue_Yeti-00.analog-stereo";
        let headset = "bluez_input.00_1B_66_A1_B2_C3";
        let dump = json!([
            node("eq_source", "Mic EQ", filter()),
            node("echo_cancel_source", "Echo-Cancel Source", filter()),
            node("echo_cancel_fallback_source", "Echo-Cancel Source", filter()),
            node("ns_source", "Noise Suppressor", filter()),
            node("alsa_input.platform-sound.HiFi__Mic__source", "Built-in Audio Capture", raw(67)),
            node(frame, "Built-in Audio Capture", json!({"alsa.loopback": true, "node.virtual": false})),
            node("alsa_input.usb-Blue_Yeti-00.analog-stereo", "Yeti Stereo Microphone", raw(80)),
            node(yeti, "Yeti Stereo Microphone", json!({"alsa.loopback": true})),
            node("bluez_input.00_1B_66_A1_B2_C3.0", "Moded Headset", json!({"device.api": "bluez5", "device.id": 90})),
            node(headset, "Moded Headset", json!({"bluez5.loopback": true, "device.id": 90, "node.virtual": false})),
            {"type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "speakers"}}},
            {"type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"}, "metadata": [
                {"key": "default.audio.source", "value": {"name": frame}},
            ]},
        ])
        .to_string();
        let mics = parse(&dump);
        let picked: Vec<_> = mics.iter().map(|m| (m.label.as_str(), m.name.as_str(), m.default)).collect();
        assert_eq!(picked, [("Frame", frame, true), ("Moded Headset", headset, false), ("Yeti Stereo Microphone", yeti, false)]);
        assert!(mics.iter().all(|m| valid_name(&m.name)));
        assert!(parse("not json").is_empty());
    }

    #[test]
    fn names_that_could_break_a_command_line_are_refused() {
        for bad in ["", "a b", "x;rm", "--mic", &"a".repeat(201)] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(valid_name("alsa_input.usb-Blue_Yeti-00.analog-stereo"));
    }
}
