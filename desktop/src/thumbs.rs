//! a frame from each video for the grid. gpui can't decode video, so this
//! asks ffmpeg for one, the small one the app ships with, and the grid shows
//! a plain tile if there's none.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

pub fn dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("com.framecorder.app").join("thumbs")
}

pub fn path_for(dir: &Path, key: &str) -> PathBuf {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    dir.join(format!("{safe}.jpg"))
}

/// the one the app ships next to itself (desktop/ffmpeg.sh), then an installed
/// one. a mac app doesn't get the shell's PATH, so brew's is looked for by place
fn program() -> &'static Path {
    static FOUND: OnceLock<PathBuf> = OnceLock::new();
    FOUND.get_or_init(|| {
        let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        let beside = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(name)));
        let brew = ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"].map(PathBuf::from);
        let brew = if cfg!(target_os = "macos") { &brew[..] } else { &[] };
        beside.into_iter().chain(brew.iter().cloned()).find(|p| p.is_file()).unwrap_or_else(|| name.into())
    })
}

pub(crate) fn ffmpeg() -> Command {
    let mut cmd = Command::new(program());
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashing up for each thumbnail
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

pub fn have_ffmpeg() -> bool {
    ffmpeg().arg("-version").status().is_ok_and(|s| s.success())
}

/// one frame a second in (the very first is often black), 480 wide
pub fn make(src: &Path, dst: &Path) -> bool {
    if dst.exists() {
        return true;
    }
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = dst.with_extension("part.jpg");
    let ok = ffmpeg()
        .args(["-loglevel", "error", "-ss", "1", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-q:v", "4", "-y"])
        .arg(&tmp)
        .status()
        .is_ok_and(|s| s.success());
    ok && std::fs::rename(&tmp, dst).is_ok()
}
