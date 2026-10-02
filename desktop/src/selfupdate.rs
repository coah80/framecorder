//! the desktop app keeping itself up to date, from the github releases.
//!
//! it only offers an update when the latest release has a build of this app
//! for this platform (framecorder-desktop-linux, framecorder-desktop-windows.exe).
//! a mac app lives in a bundle, so there it opens the release page instead.

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
        Some("framecorder-desktop-linux")
    } else if cfg!(windows) {
        Some("framecorder-desktop-windows.exe")
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

/// downloads the new build next to us, calling `progress` with 0 to 1. blocking
pub fn download(url: &str, size: u64, progress: impl Fn(f32)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dst = exe.with_extension("update");
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

/// swaps the running binary for the new one and starts it
pub fn apply_and_restart(new: &PathBuf) -> Result<(), String> {
    self_replace::self_replace(new).map_err(|e| format!("couldn't put the update in place: {e}"))?;
    let _ = std::fs::remove_file(new);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).arg("--after-update").spawn().map_err(|e| e.to_string())?;
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
