//! the desktop app keeping itself up to date, from the github releases.
//!
//! it only offers an update when the latest release has a build of this app
//! for this platform (framecorder-x86_64.AppImage, framecorder-setup.exe). on
//! windows the update is the installer, run silently: it replaces the
//! installed app and starts it again. on linux the new AppImage takes the old
//! one's place. a mac app lives in a bundle, so there it opens the release
//! page instead.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Deserialize;

const LATEST: &str = "https://api.github.com/repos/coah80/framecorder/releases/latest";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    /// where this platform's build is, when there is one
    pub asset: Option<(String, u64)>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
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
    let asset = asset_name()
        .and_then(|want| rel.assets.iter().find(|a| a.name == want).map(|a| (a.browser_download_url.clone(), a.size)));
    // a release without a build of this app for here: nothing to offer, except on mac
    if asset.is_none() && !cfg!(target_os = "macos") {
        return Ok(None);
    }
    Ok(Some(Release { version: rel.tag_name.trim_start_matches('v').to_string(), page: rel.html_url, asset }))
}

/// where an update downloads to: the temp folder for the windows installer,
/// next to the AppImage on linux, so putting it in place is a rename
fn update_path() -> Result<PathBuf, String> {
    if cfg!(windows) {
        return Ok(std::env::temp_dir().join("framecorder-setup.exe"));
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
#[cfg(not(windows))]
pub fn apply_and_restart(new: &PathBuf) -> Result<(), String> {
    let file = crate::appimage::file().ok_or("this isn't running from an AppImage")?;
    std::fs::rename(new, &file).map_err(|e| format!("couldn't put the update in place: {e}"))?;
    std::process::Command::new(&file).arg("--after-update").spawn().map_err(|e| e.to_string())?;
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
