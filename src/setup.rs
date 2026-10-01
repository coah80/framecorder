//! Installing framecorder from an unpacked release: the programs into
//! ~/.local/bin, the services into systemd, and the one permission the
//! recorder needs if it can be had without asking.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The release's files, packed next to the installer.
const PAYLOAD: &str = "payload.tar";
const PROGRAMS: [&str; 3] = ["framecorder", "framecorder-ui", "framecorder-sync"];
const SERVICES: [&str; 2] = ["framecorder-ui.service", "framecorder-sync.service"];
/// What lets the recorder read what's on the display.
const CAPABILITY: &str = "cap_sys_admin+ep";

pub struct Report {
    pub updated: Vec<String>,
    /// Whether the recorder may read the display.
    pub unlocked: bool,
}

impl Report {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![match self.updated.len() {
            0 => "framecorder is up to date".to_string(),
            _ => format!("installed {}", self.updated.join(", ")),
        }];
        lines.push("ready: open the steamvr dashboard, there's a framecorder tab".to_string());
        if !self.unlocked {
            lines.push(format!("it records steamvr's view for now. for the panels (1:1, 9:16, both eyes), give the recorder {CAPABILITY}"));
        }
        lines
    }
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).context("HOME isn't set")
}

/// Whether a program has been given a capability (any: we only ever set one).
pub fn unlocked(program: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(program.as_os_str().as_bytes()) else { return false };
    let size = unsafe { libc::getxattr(path.as_ptr(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
    size > 0
}

/// Where the recorder gets installed.
pub fn recorder() -> Result<PathBuf> {
    Ok(home()?.join(".local/bin/framecorder"))
}

/// Copies `from` over `to` unless they're the same already. Leaving an
/// unchanged program alone keeps the permission it was given.
fn place(from: &Path, to: &Path, mode: u32) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let new = std::fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    if std::fs::read(to).is_ok_and(|old| old == new) {
        return Ok(false);
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    // Written next to it and moved into place: a running program can't be
    // written over, but it can be replaced.
    let staged = to.with_extension("new");
    std::fs::write(&staged, new).with_context(|| format!("writing {}", staged.display()))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&staged, to).with_context(|| format!("replacing {}", to.display()))?;
    Ok(true)
}

/// Puts an unpacked release in place under `home`. Says what changed.
pub fn install(release: &Path, home: &Path) -> Result<Vec<String>> {
    let mut updated = Vec::new();
    for name in PROGRAMS {
        if place(&release.join("bin").join(name), &home.join(".local/bin").join(name), 0o755)? {
            updated.push(name.to_string());
        }
    }
    for name in SERVICES {
        if place(&release.join("services").join(name), &home.join(".config/systemd/user").join(name), 0o644)? {
            updated.push(name.to_string());
        }
    }
    Ok(updated)
}

fn quiet(program: &str, args: &[&str]) -> bool {
    let done = Command::new(program).args(args).output();
    match done {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            log::debug!("{program} {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
            false
        }
        Err(e) => {
            log::debug!("{program}: {e}");
            false
        }
    }
}

pub fn run() -> Result<Report> {
    let here = std::env::current_exe()?.parent().map(Path::to_path_buf).context("no folder to install from")?;
    let payload = here.join(PAYLOAD);
    if !payload.exists() {
        bail!("{} isn't next to the installer, is the download complete?", payload.display());
    }
    let home = home()?;
    let release = home.join(".local/share/framecorder/release");
    let _ = std::fs::remove_dir_all(&release);
    std::fs::create_dir_all(&release)?;
    let unpacked = Command::new("tar").arg("-xf").arg(&payload).arg("-C").arg(&release).status().context("running tar")?;
    if !unpacked.success() {
        bail!("couldn't unpack {}", payload.display());
    }

    let updated = install(&release, &home)?;
    let recorder = recorder()?;
    if !unlocked(&recorder) {
        // Works where the headset lets this user be root without a password.
        // Where it doesn't, the tab explains the one command that's needed.
        quiet("sudo", &["-n", "setcap", CAPABILITY, &recorder.to_string_lossy()]);
    }

    quiet("systemctl", &["--user", "daemon-reload"]);
    quiet("systemctl", &["--user", "enable", "--now", "framecorder-sync.service"]);
    quiet("systemctl", &["--user", "enable", "framecorder-ui.service"]);
    // Starts with SteamVR from now on; this gets it going right now, on the tab.
    quiet("systemctl", &["--user", "restart", "framecorder-ui.service"]);
    Ok(Report { updated, unlocked: unlocked(&recorder) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(dir: &Path, recorder: &[u8]) {
        for (folder, names) in [("bin", &PROGRAMS[..]), ("services", &SERVICES[..])] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
            for name in names {
                std::fs::write(dir.join(folder).join(name), name.as_bytes()).unwrap();
            }
        }
        std::fs::write(dir.join("bin/framecorder"), recorder).unwrap();
    }

    #[test]
    fn installs_then_only_what_changed() {
        let dir = std::env::temp_dir().join(format!("framecorder-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (from, home) = (dir.join("release"), dir.join("home"));
        release(&from, b"one");

        assert_eq!(install(&from, &home).unwrap().len(), PROGRAMS.len() + SERVICES.len());
        let program = home.join(".local/bin/framecorder");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&program).unwrap().permissions().mode() & 0o777, 0o755);

        // Again with nothing new: nothing gets touched.
        assert!(install(&from, &home).unwrap().is_empty());

        release(&from, b"two");
        assert_eq!(install(&from, &home).unwrap(), ["framecorder"]);
        assert_eq!(std::fs::read(&program).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(dir);
    }
}
