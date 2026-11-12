//! What the dashboard tab remembers between sessions, and how it turns into
//! recorder arguments.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

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
    Both,
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
pub enum Codec {
    Hevc,
    /// Plays on more things, bigger for the same picture.
    H264,
}

/// The file recordings and clips are saved as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Hybrid: fragmented while it's written, a regular MP4 once it's done.
    Mp4,
    Mkv,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
            Format::Mkv => "mkv",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub shape: Shape,
    pub eye: Eye,
    pub quality: Quality,
    pub fps: FrameRate,
    pub codec: Codec,
    pub format: Format,
    /// Recordings in MKV get remuxed to MP4 once they're done. Not clips.
    pub remux: bool,
    pub game_audio: bool,
    pub mic: bool,
    /// Which mic, by PipeWire node name. Empty is the one SteamOS uses.
    pub mic_source: String,
    /// Also keep the game and the mic on their own tracks, for editing.
    pub split_audio: bool,
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
            codec: Codec::Hevc,
            format: Format::Mp4,
            remux: false,
            game_audio: true,
            mic: true,
            mic_source: String::new(),
            split_audio: true,
            clips: true,
            clip_secs: 30,
            onboarded: false,
        }
    }
}

impl Quality {
    /// Video bitrate in Mbit/s.
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
                ("eye", "both") => s.eye = Eye::Both,
                ("quality", "standard") => s.quality = Quality::Standard,
                ("quality", "high") => s.quality = Quality::High,
                ("quality", "max") => s.quality = Quality::Max,
                ("fps", "auto") => s.fps = FrameRate::Auto,
                ("fps", "60") => s.fps = FrameRate::Sixty,
                ("fps", "30") => s.fps = FrameRate::Thirty,
                ("codec", "hevc") => s.codec = Codec::Hevc,
                ("codec", "h264") => s.codec = Codec::H264,
                ("format", "mp4") => s.format = Format::Mp4,
                ("format", "mkv") => s.format = Format::Mkv,
                ("remux", v) => s.remux = v == "true",
                ("game_audio", v) => s.game_audio = v == "true",
                ("mic", v) => s.mic = v == "true",
                ("split_audio", v) => s.split_audio = v == "true",
                ("mic_source", v) if v.is_empty() || crate::mics::valid_name(v) => s.mic_source = v.to_string(),
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
            "shape={}\neye={}\nquality={}\nfps={}\ncodec={}\nformat={}\nremux={}\ngame_audio={}\nmic={}\nmic_source={}\nsplit_audio={}\nclips={}\nclip={}\nonboarded={}\n",
            match self.shape {
                Shape::Wide => "wide",
                Shape::Square => "square",
                Shape::Tall => "tall",
                Shape::BothEyes => "both",
            },
            match self.eye {
                Eye::Left => "left",
                Eye::Right => "right",
                Eye::Both => "both",
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
            match self.codec {
                Codec::Hevc => "hevc",
                Codec::H264 => "h264",
            },
            self.format.ext(),
            self.remux,
            self.game_audio,
            self.mic,
            self.mic_source,
            self.split_audio,
            self.clips,
            self.clip_secs,
            self.onboarded
        )
    }

    /// When the file last changed. A phone changes it through framecorder-sync.
    pub fn changed_at() -> Option<SystemTime> {
        config_path().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }

    /// The folder recordings go in, clips in its `clips` folder.
    pub fn folder(&self) -> Result<PathBuf> {
        let home = std::env::var_os("HOME").context("HOME isn't set")?;
        Ok(PathBuf::from(home).join("Videos/framecorder"))
    }

    /// A new file for a recording (`sub` empty) or a clip (`sub` "clips"),
    /// named for now, with -2, -3... on the end if that second's taken.
    pub fn new_file(&self, sub: &str) -> Result<PathBuf> {
        let dir = self.folder()?.join(sub);
        let mut t: libc::tm = unsafe { std::mem::zeroed() };
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        unsafe { libc::localtime_r(&now, &mut t) };
        let stem = format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            t.tm_year + 1900,
            t.tm_mon + 1,
            t.tm_mday,
            t.tm_hour,
            t.tm_min,
            t.tm_sec
        );
        Ok(unique(&dir, &stem, self.format.ext()))
    }

    /// Splitting needs both the game and the mic.
    pub fn splits_audio(&self) -> bool {
        self.game_audio && self.mic && self.split_audio
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
                let eye = match self.eye { Eye::Left => "left", Eye::Right => "right", Eye::Both => "both" };
                args.extend(["--eye".into(), eye.into()]);
            }
        }
        args.extend(["--bitrate".into(), self.quality.mbps().to_string()]);
        match self.fps {
            FrameRate::Auto => {}
            FrameRate::Sixty => args.extend(["--fps".into(), "60".into()]),
            FrameRate::Thirty => args.extend(["--fps".into(), "30".into()]),
        }
        if self.codec == Codec::H264 {
            args.extend(["--codec".into(), "h264".into()]);
        }
        if self.format == Format::Mkv && self.remux {
            args.push("--remux".into());
        }
        if !self.game_audio {
            args.push("--no-audio".into());
        }
        if self.mic {
            args.push("--mic".into());
            if !self.mic_source.is_empty() {
                args.extend(["--mic-source".into(), self.mic_source.clone()]);
            }
        }
        if self.splits_audio() {
            args.push("--mic-track".into());
        }
        args
    }
}

/// `dir/stem.ext`, or `stem-2.ext` and on when that or its unfinished
/// `.part` is already there.
fn unique(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let taken = |p: &Path| p.exists() || p.with_extension(format!("{ext}.part")).exists();
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while taken(&path) {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}

/// What the recorder leaves when it's cut off (the headset died, or the
/// recorder did) still plays, if it's an MKV or a hybrid MP4, so those get
/// their real name back. An MP4 from before hybrid has nothing to play from
/// and stays as it is. A half done remux next to its MKV goes, the MKV has
/// it all. Only call this while no recorder is writing there.
pub fn finish_leftovers(folder: &Path) {
    for dir in [folder.to_path_buf(), folder.join("clips")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let names: Vec<String> = entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
        for name in &names {
            let Some(done) = name.strip_suffix(".part") else { continue };
            let path = dir.join(name);
            if let Some(stem) = done.strip_suffix(".mp4") {
                if names.contains(&format!("{stem}.mkv.part")) {
                    log::info!("{} was a remux that didn't finish, its mkv stays", path.display());
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if !hybrid_mp4(&path) {
                    continue;
                }
            } else if !done.ends_with(".mkv") {
                continue;
            }
            let (stem, ext) = done.rsplit_once('.').unwrap_or((done, ""));
            let mut to = dir.join(done);
            let mut n = 2;
            while to.exists() {
                to = dir.join(format!("{stem}-{n}.{ext}"));
                n += 1;
            }
            match std::fs::rename(&path, &to) {
                Ok(()) => log::info!("{} was left unfinished, it's {} now", path.display(), to.display()),
                Err(e) => log::warn!("couldn't rename {}: {e}", path.display()),
            }
        }
    }
}

/// Whether an unfinished MP4 has its index up front, the way a hybrid one
/// does: a moov before the media data.
fn hybrid_mp4(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut at = 0u64;
    for _ in 0..8 {
        let mut head = [0u8; 8];
        if f.seek(SeekFrom::Start(at)).and_then(|_| f.read_exact(&mut head)).is_err() {
            return false;
        }
        let size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
        match &head[4..] {
            b"moov" => return true,
            b"mdat" => return false,
            _ if size < 8 => return false,
            _ => at += size,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_for_defaults() {
        let args = Settings::default().recorder_args(true).join(" ");
        assert_eq!(args, "--view eye --aspect 16:9 --eye left --bitrate 40 --mic --mic-track");
        let mixed = Settings { split_audio: false, ..Settings::default() };
        assert_eq!(mixed.recorder_args(true).join(" "), "--view eye --aspect 16:9 --eye left --bitrate 40 --mic");
        assert_eq!(Settings::parse(&mixed.text()), mixed);
    }

    #[test]
    fn args_without_the_panel() {
        let s = Settings { shape: Shape::Tall, ..Settings::default() };
        assert_eq!(s.recorder_args(false).join(" "), "--view eye --aspect 16:9 --source headset --bitrate 40 --mic --mic-track");
    }

    #[test]
    fn args_for_both_eyes_with_mic() {
        let s = Settings { shape: Shape::BothEyes, mic: true, game_audio: false, ..Settings::default() };
        assert_eq!(s.recorder_args(true).join(" "), "--view raw --bitrate 40 --no-audio --mic");
    }

    #[test]
    fn what_a_cut_off_recorder_left_gets_its_name_back() {
        let dir = std::env::temp_dir().join(format!("framecorder-leftovers-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        let mp4 = |boxes: &[&[u8; 4]]| boxes.iter().flat_map(|b| [&8u32.to_be_bytes()[..], &b[..]].concat()).collect::<Vec<u8>>();
        let files: [(&str, Vec<u8>); 7] = [
            ("a.mkv.part", b"a".to_vec()),
            ("clips/b.mkv.part", b"b".to_vec()),
            // hybrid: ftyp, the size placeholder, then the first moov
            ("c.mp4.part", mp4(&[b"ftyp", b"free", b"moov"])),
            // from before hybrid, nothing to play from
            ("d.mp4.part", mp4(&[b"ftyp", b"mdat"])),
            ("e.mkv", b"e".to_vec()),
            ("e.mkv.part", b"e part".to_vec()),
            // a remux cut off halfway, next to the mkv it came from
            ("f.mp4.part", mp4(&[b"ftyp", b"free", b"moov"])),
        ];
        for (name, data) in &files {
            std::fs::write(dir.join(name), data).unwrap();
        }
        std::fs::write(dir.join("f.mkv.part"), b"f").unwrap();
        finish_leftovers(&dir);
        let mut left: Vec<String> = ["", "clips"]
            .iter()
            .flat_map(|sub| std::fs::read_dir(dir.join(sub)).unwrap().flatten().map(move |e| format!("{sub}/{}", e.file_name().to_string_lossy())))
            .filter(|n| !n.ends_with("/clips"))
            .collect();
        left.sort();
        assert_eq!(left, ["/a.mkv", "/c.mp4", "/d.mp4.part", "/e-2.mkv", "/e.mkv", "/f.mkv", "clips/b.mkv"]);
        assert_eq!(std::fs::read(dir.join("e-2.mkv")).unwrap(), b"e part");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remux_only_reaches_the_recorder_for_mkv() {
        let mkv = Settings { format: Format::Mkv, remux: true, onboarded: true, ..Settings::default() };
        assert_eq!(Settings::parse(&mkv.text()), mkv);
        assert!(mkv.recorder_args(true).contains(&"--remux".to_string()));
        let mp4 = Settings { format: Format::Mp4, ..mkv };
        assert!(!mp4.recorder_args(true).contains(&"--remux".to_string()));
    }

    #[test]
    fn h264_round_trips_and_reaches_the_recorder() {
        let s = Settings { codec: Codec::H264, onboarded: true, ..Settings::default() };
        assert_eq!(Settings::parse(&s.text()), s);
        assert!(s.recorder_args(true).join(" ").contains("--codec h264"));
        assert!(s.recorder_args(false).join(" ").contains("--codec h264"));
    }

    #[test]
    fn blended_eye_round_trips_and_falls_back_without_panels() {
        for shape in [Shape::Wide, Shape::Square, Shape::Tall] {
            let settings = Settings { eye: Eye::Both, shape, ..Settings::default() };
            assert_eq!(Settings::parse(&settings.text()), Settings { onboarded: false, ..settings.clone() });
            assert!(settings.recorder_args(true).join(" ").contains("--eye both"));
            assert_eq!(settings.recorder_args(false).join(" "), "--view eye --aspect 16:9 --source headset --bitrate 40 --mic --mic-track");
        }
    }

    #[test]
    fn clip_length_round_trips() {
        let dir = std::env::temp_dir().join(format!("framecorder-settings-{}", std::process::id()));
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        // Nothing saved yet: a new user, who gets the setup.
        assert!(!Settings::load().onboarded);
        let s = Settings { clip_secs: 60, ..Settings::default() };
        s.save();
        assert_eq!(Settings::load().clipping(), Some(60));
        Settings { clips: false, ..s.clone() }.save();
        let off = Settings::load();
        assert_eq!(off.clipping(), None);
        // The length sticks around for when they're turned back on.
        assert_eq!(off.clip_secs, 60);
        std::fs::write(dir.join("framecorder/ui.conf"), "clip=off\n").unwrap();
        assert_eq!(Settings::load().clipping(), None);
        let initial = Settings { onboarded: true, ..s };
        initial.save();
        let mut app = super::super::App::new(initial.clone(), false, false);
        let changed = Settings { mic: false, quality: Quality::Standard, ..initial };
        changed.save();
        assert!(app.prepare_recording().is_ok());
        assert_eq!(app.settings, changed);
        assert!(app.settings_checked.elapsed() < super::super::SETTINGS_EVERY);
        if let Some(previous) = previous {
            std::env::set_var("XDG_CONFIG_HOME", previous);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
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
