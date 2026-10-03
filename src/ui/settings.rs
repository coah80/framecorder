//! What the dashboard tab remembers between sessions, and how it turns into
//! recorder arguments.

use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Wide,
    Square,
    Tall,
    BothEyes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eye {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Standard,
    High,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRate {
    Auto,
    Sixty,
    Thirty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub shape: Shape,
    pub eye: Eye,
    pub quality: Quality,
    pub fps: FrameRate,
    pub game_audio: bool,
    pub mic: bool,
    /// Keep a replay buffer for clips.
    pub clips: bool,
    /// How much of it, in seconds; remembered while clips are off.
    pub clip_secs: u32,
    /// The first-run setup has been finished or skipped.
    pub onboarded: bool,
}

/// Clip lengths the tab offers.
pub const CLIP_LENGTHS: [u32; 4] = [15, 30, 60, 120];

impl Default for Settings {
    fn default() -> Self {
        Self {
            shape: Shape::Wide,
            eye: Eye::Left,
            quality: Quality::High,
            fps: FrameRate::Auto,
            game_audio: true,
            mic: true,
            clips: true,
            clip_secs: 30,
            onboarded: false,
        }
    }
}

impl Quality {
    /// HEVC bitrate in Mbit/s.
    pub fn mbps(self) -> u32 {
        match self {
            Quality::Standard => 20,
            Quality::High => 40,
            Quality::Max => 80,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("framecorder/ui.conf"))
}

impl Settings {
    /// No config at all means nobody's set this up yet.
    pub fn load() -> Self {
        config_path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Self::default, |text| Self::parse(&text))
    }

    fn parse(text: &str) -> Self {
        // Configs from before the setup existed belong to people who've
        // already picked their settings, so they don't get walked through it.
        let mut s = Self { onboarded: true, ..Self::default() };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match (key.trim(), value.trim()) {
                ("shape", "wide") => s.shape = Shape::Wide,
                ("shape", "square") => s.shape = Shape::Square,
                ("shape", "tall") => s.shape = Shape::Tall,
                ("shape", "both") => s.shape = Shape::BothEyes,
                ("eye", "left") => s.eye = Eye::Left,
                ("eye", "right") => s.eye = Eye::Right,
                ("quality", "standard") => s.quality = Quality::Standard,
                ("quality", "high") => s.quality = Quality::High,
                ("quality", "max") => s.quality = Quality::Max,
                ("fps", "auto") => s.fps = FrameRate::Auto,
                ("fps", "60") => s.fps = FrameRate::Sixty,
                ("fps", "30") => s.fps = FrameRate::Thirty,
                ("game_audio", v) => s.game_audio = v == "true",
                ("mic", v) => s.mic = v == "true",
                ("clips", v) => s.clips = v == "true",
                ("onboarded", v) => s.onboarded = v == "true",
                // Older configs turned clips off with the length.
                ("clip", "off") => s.clips = false,
                ("clip", v) => {
                    if let Some(secs) = v.parse().ok().filter(|n| CLIP_LENGTHS.contains(n)) {
                        s.clip_secs = secs;
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// Returns whether it got written.
    pub fn save(&self) -> bool {
        let Some(path) = config_path() else {
            log::warn!("couldn't save settings: no config directory");
            return false;
        };
        // Written whole and renamed over, since framecorder-sync reads it too.
        let tmp = path.with_extension("conf.tmp");
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, self.text()))
            .and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = &result {
            log::warn!("couldn't save settings to {}: {e}", path.display());
        }
        result.is_ok()
    }

    fn text(&self) -> String {
        format!(
            "shape={}\neye={}\nquality={}\nfps={}\ngame_audio={}\nmic={}\nclips={}\nclip={}\nonboarded={}\n",
            match self.shape {
                Shape::Wide => "wide",
                Shape::Square => "square",
                Shape::Tall => "tall",
                Shape::BothEyes => "both",
            },
            match self.eye {
                Eye::Left => "left",
                Eye::Right => "right",
            },
            match self.quality {
                Quality::Standard => "standard",
                Quality::High => "high",
                Quality::Max => "max",
            },
            match self.fps {
                FrameRate::Auto => "auto",
                FrameRate::Sixty => "60",
                FrameRate::Thirty => "30",
            },
            self.game_audio,
            self.mic,
            self.clips,
            self.clip_secs,
            self.onboarded
        )
    }

    /// When the file last changed. A phone changes it through framecorder-sync.
    pub fn changed_at() -> Option<SystemTime> {
        config_path().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }

    pub fn clipping(&self) -> Option<u32> {
        self.clips.then_some(self.clip_secs)
    }

    /// Recorder arguments, minus the output path.
    /// `panel` is whether the recorder may read the panels. Without that it
    /// records SteamVR's headset view, which is 16:9 of the left eye only.
    pub fn recorder_args(&self, panel: bool) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        match self.shape {
            _ if !panel => args.extend(["--view", "eye", "--aspect", "16:9", "--source", "headset"].map(String::from)),
            Shape::BothEyes => args.extend(["--view".into(), "raw".into()]),
            shape => {
                let aspect = match shape {
                    Shape::Square => "1:1",
                    Shape::Tall => "9:16",
                    _ => "16:9",
                };
                args.extend(["--view".into(), "eye".into(), "--aspect".into(), aspect.into()]);
                let eye = if self.eye == Eye::Right { "right" } else { "left" };
                args.extend(["--eye".into(), eye.into()]);
            }
        }
        args.extend(["--bitrate".into(), self.quality.mbps().to_string()]);
        match self.fps {
            FrameRate::Auto => {}
            FrameRate::Sixty => args.extend(["--fps".into(), "60".into()]),
            FrameRate::Thirty => args.extend(["--fps".into(), "30".into()]),
        }
        if !self.game_audio {
            args.push("--no-audio".into());
        }
        if self.mic {
            args.push("--mic".into());
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_for_defaults() {
        let args = Settings::default().recorder_args(true).join(" ");
        assert_eq!(args, "--view eye --aspect 16:9 --eye left --bitrate 40 --mic");
    }

    #[test]
    fn args_without_the_panel() {
        let s = Settings { shape: Shape::Tall, ..Settings::default() };
        assert_eq!(s.recorder_args(false).join(" "), "--view eye --aspect 16:9 --source headset --bitrate 40 --mic");
    }

    #[test]
    fn args_for_both_eyes_with_mic() {
        let s = Settings { shape: Shape::BothEyes, mic: true, game_audio: false, ..Settings::default() };
        assert_eq!(s.recorder_args(true).join(" "), "--view raw --bitrate 40 --no-audio --mic");
    }

    #[test]
    fn clip_length_round_trips() {
        let dir = std::env::temp_dir().join(format!("framecorder-settings-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        // Nothing saved yet: a new user, who gets the setup.
        assert!(!Settings::load().onboarded);
        let s = Settings { clip_secs: 60, ..Settings::default() };
        s.save();
        assert_eq!(Settings::load().clipping(), Some(60));
        Settings { clips: false, ..s }.save();
        let off = Settings::load();
        assert_eq!(off.clipping(), None);
        // The length sticks around for when they're turned back on.
        assert_eq!(off.clip_secs, 60);
        std::fs::write(dir.join("framecorder/ui.conf"), "clip=off\n").unwrap();
        assert_eq!(Settings::load().clipping(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn setup_shows_once_and_never_for_older_configs() {
        let halfway = Settings { quality: Quality::Max, ..Settings::default() };
        assert!(!Settings::parse(&halfway.text()).onboarded);
        let done = Settings { onboarded: true, ..halfway };
        assert_eq!(Settings::parse(&done.text()), done);
        // Written before the setup existed.
        let old = Settings::parse("shape=tall\nclip=60\n");
        assert!(old.onboarded);
        assert_eq!(old.shape, Shape::Tall);
    }
}
