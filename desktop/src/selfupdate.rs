//! the desktop app keeping itself up to date, from the github releases.
//!
//! it only offers an update when the latest release has a build of this app
//! for this platform (framecorder-x86_64.AppImage, framecorder-setup.exe,
//! framecorder-desktop-macos.dmg). on windows the update is the installer,
//! run silently: it replaces the installed app and starts it again. on linux
//! the new AppImage takes the old one's place. on a mac the new
//! framecorder.app comes out of the dmg and takes the old one's place.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Deserialize;

const LATEST: &str = "https://api.github.com/repos/coah80/framecorder/releases/latest";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// where this platform's build is, and its size
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("framecorder-x86_64.AppImage")
    } else if cfg!(windows) {
        Some("framecorder-setup.exe")
    } else if cfg!(target_os = "macos") {
        Some("framecorder-desktop-macos.dmg")
    } else {
        None
    }
}

fn parse(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(['.', '-']).map_while(|p| p.parse().ok()).collect()
}

pub fn newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

/// the system's own root certificates, so this works behind a proxy that
/// inspects tls, like a lot of school and work networks have
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().tls_config(tls).build().into()
}

/// whether there's a newer desktop app than this one. blocking
pub fn check() -> Result<Option<Release>, String> {
    // a linux build that isn't an AppImage is someone's own, it has nothing to swap
    if cfg!(target_os = "linux") && crate::appimage::file().is_none() {
        return Ok(None);
    }
    // the same on a mac outside a framecorder.app
    #[cfg(target_os = "macos")]
    if bundle().is_none() {
        return Ok(None);
    }
    let rel: GhRelease = agent()
        .get(LATEST)
        .header("User-Agent", "framecorder-desktop")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    if !newer(&rel.tag_name, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    // a release without a build of this app for here has nothing to offer
    let Some(asset) = asset_name().and_then(|want| rel.assets.into_iter().find(|a| a.name == want)) else {
        return Ok(None);
    };
    Ok(Some(Release {
        version: rel.tag_name.trim_start_matches('v').to_string(),
        url: asset.browser_download_url,
        size: asset.size,
    }))
}

/// where an update downloads to: the temp folder for the windows installer
/// and the mac dmg, next to the AppImage on linux, so putting it in place is
/// a rename
fn update_path() -> Result<PathBuf, String> {
    if cfg!(windows) {
        return Ok(std::env::temp_dir().join("framecorder-setup.exe"));
    }
    if cfg!(target_os = "macos") {
        return Ok(std::env::temp_dir().join("framecorder-update.dmg"));
    }
    let file = crate::appimage::file().ok_or("this isn't running from an AppImage")?;
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".update");
    Ok(file.with_file_name(name))
}

/// downloads the new build, calling `progress` with 0 to 1. blocking
pub fn download(url: &str, size: u64, progress: impl Fn(f32)) -> Result<PathBuf, String> {
    let dst = update_path()?;
    let mut res = agent().get(url).header("User-Agent", "framecorder-desktop").call().map_err(|e| e.to_string())?;
    let total = res.body().content_length().unwrap_or(size).max(1);
    let mut reader = res.body_mut().as_reader();
    let mut out = std::fs::File::create(&dst).map_err(|e| format!("can't write {}: {e}", dst.display()))?;
    let (mut buf, mut done) = (vec![0u8; 64 * 1024], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress((done as f32 / total as f32).min(1.0));
    }
    out.sync_all().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
    }
    Ok(dst)
}

/// starts the installer, which waits for us to quit (it closes us if we
/// don't), installs over us and starts the new version
#[cfg(windows)]
pub fn apply_and_restart(setup: &PathBuf) -> Result<(), String> {
    std::process::Command::new(setup)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't start the installer: {e}"))
}

/// puts the new AppImage where the old one is and starts it. the running one
/// keeps its own open copy until it quits
#[cfg(target_os = "linux")]
pub fn apply_and_restart(new: &PathBuf) -> Result<(), String> {
    use std::os::unix::process::CommandExt;

    let file = crate::appimage::file().ok_or("this isn't running from an AppImage")?;
    std::fs::rename(new, &file).map_err(|e| format!("couldn't put the update in place: {e}"))?;
    let mut cmd = std::process::Command::new(&file);
    cmd.arg("--after-update");
    // the AppImage runtime keeps the old version mounted until every copy of
    // a pipe it hands us is closed, and we'd pass ours on to the new version.
    // so the new one starts with nothing open but stdin, stdout and stderr.
    // close_range as a raw syscall, the glibc wrapper is too new for some distros
    unsafe {
        cmd.pre_exec(|| {
            #[cfg(target_os = "linux")]
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) == 0 {
                return Ok(());
            }
            for fd in 3..1024 {
                libc::close(fd);
            }
            Ok(())
        });
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// the framecorder.app we're running from, when we are
#[cfg(target_os = "macos")]
fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(PathBuf::from)
}

/// copies the new framecorder.app out of the dmg next to ours, swaps the two,
/// and leaves a shell to open it once we've quit. a second copy started
/// before then would find us still running and only bring our window up
#[cfg(target_os = "macos")]
pub fn apply_and_restart(dmg: &PathBuf) -> Result<(), String> {
    use std::process::Command;

    let app = bundle().ok_or("this isn't running from framecorder.app")?;
    let run = |cmd: &mut Command, what: &str| match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{what} failed ({s})")),
        Err(e) => Err(format!("{what} failed: {e}")),
    };
    let mount = std::env::temp_dir().join("framecorder-update");
    let staged = app.with_file_name(".framecorder-update.app");
    let old = app.with_file_name(".framecorder-old.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    run(Command::new("hdiutil").arg("attach").arg("-nobrowse").arg("-readonly").arg("-mountpoint").arg(&mount).arg(dmg), "opening the update")?;
    let copied = run(Command::new("ditto").arg(mount.join("framecorder.app")).arg(&staged), "copying the update");
    let _ = Command::new("hdiutil").arg("detach").arg("-quiet").arg(&mount).status();
    copied?;
    std::fs::rename(&app, &old).map_err(|e| format!("couldn't put the update in place: {e}"))?;
    if let Err(e) = std::fs::rename(&staged, &app) {
        let _ = std::fs::rename(&old, &app);
        return Err(format!("couldn't put the update in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_file(dmg);
    Command::new("sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; open "$2" --args --after-update"#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(&app)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions() {
        assert!(newer("v0.2.0", "0.1.1"));
        assert!(newer("0.1.10", "0.1.9"));
        assert!(!newer("v0.1.1", "0.1.1"));
        assert!(!newer("v0.1.0", "0.1.1"));
    }
}
