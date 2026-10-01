//! Installing framecorder from an unpacked release: the programs into
//! ~/.local/bin, the services into systemd, and the one permission the
//! recorder needs if it can be had without asking.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The release's files, packed next to the installer.
const PAYLOAD: &str = "payload.tar";
const PROGRAMS: [&str; 4] = ["framecorder", "framecorder-ui", "framecorder-sync", "framecorder-setup"];
const SERVICES: [&str; 4] =
    ["framecorder-ui.service", "framecorder-sync.service", "framecorder-update.service", "framecorder-update.timer"];
/// The latest release, and its checksum next to it at `.sha256`.
const RELEASE_URL: &str = "https://framecorder.coah80.com/dl/framecorder-arm64.tar.gz";
/// Where it was installed from, when that's not the releases (the dev
/// builds): the folder the installer downloaded from. Updates come from
/// there too. Installing from the normal place takes it away again.
const SOURCE: &str = ".local/share/framecorder/source";
/// The checksum of the release that's installed, under the home folder.
const INSTALLED: &str = ".local/share/framecorder/installed.sha256";
/// Left by an update that replaced an unlocked recorder, which drops its
/// permission, so the tab can say how to get it back.
pub const RELOCK: &str = ".local/share/framecorder/relock";
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
            lines.push("the panels aren't unlocked: run the installer, it asks for your password".to_string());
        }
        lines
    }
}

/// Points systemctl at the user's own systemd. Konsole in the Frame's
/// desktop mode runs in a nested plasma session with its own runtime folder
/// and session bus, where `systemctl --user` can't find it.
fn use_user_manager() {
    let runtime = format!("/run/user/{}", unsafe { libc::getuid() });
    if Path::new(&runtime).join("bus").exists() {
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={runtime}/bus"));
        std::env::set_var("XDG_RUNTIME_DIR", runtime);
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

/// Whether an update took the recorder's permission away (see `RELOCK`).
pub fn relocked() -> bool {
    home().is_ok_and(|h| h.join(RELOCK).exists())
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

/// Where framecorder shows up as an app: the "launch program" list on the
/// Frame's app bar, and desktop mode's app menu. Both read these.
const DESKTOP_ENTRY: &str = ".local/share/applications/framecorder.desktop";
const ICON: &str = ".local/share/icons/hicolor/128x128/apps/framecorder.png";

/// Writes framecorder's app menu entry and icon. Launching it opens the tab,
/// or starts framecorder again if it was closed.
fn desktop_entry(home: &Path) -> Result<()> {
    let ui = home.join(".local/bin/framecorder-ui");
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=framecorder\n\
         Comment=Records what the Frame's panels show\n\
         Exec={} --show\n\
         Icon=framecorder\n\
         Categories=AudioVideo;Recorder;\n\
         Terminal=false\n",
        ui.display()
    );
    let files: [(&str, &[u8]); 2] = [(DESKTOP_ENTRY, entry.as_bytes()), (ICON, include_bytes!("../app/icons/128x128.png"))];
    for (path, bytes) in files {
        let path = home.join(path);
        if std::fs::read(&path).is_ok_and(|old| old == bytes) {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
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

/// Installs the release next to this program. `restart_ui` gets the tab
/// going on the new version right away; updates leave that to the tab, which
/// restarts itself when nothing's being recorded.
pub fn run(restart_ui: bool) -> Result<Report> {
    use_user_manager();
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

    // Installed from the normal place (the installer passes FRAMECORDER_DL
    // through when it's not): back on releases, see SOURCE. Not for updates,
    // they come from wherever it was installed from.
    if restart_ui && std::env::var_os("FRAMECORDER_DL").is_none() {
        let _ = std::fs::remove_file(home.join(SOURCE));
    }
    let was_unlocked = panels_unlocked();
    let updated = install(&release, &home)?;
    desktop_entry(&home)?;
    // Run by hand, right after the installer asked for the password (sudo
    // remembers it for a bit): puts the panel helper in place, or a newer
    // one. The update timer has no password, so it doesn't try.
    if restart_ui && (!panels_unlocked() || helper_outdated(&release)) {
        if let Err(e) = unlock() {
            log::debug!("no panel helper: {e:#}");
        }
    }

    let relock = home.join(RELOCK);
    if panels_unlocked() {
        let _ = std::fs::remove_file(&relock);
    } else if was_unlocked {
        let _ = std::fs::write(&relock, "");
    }

    // These have to work, or nothing runs, so they're not quiet about it.
    output("systemctl", &["--user", "daemon-reload"])?;
    output("systemctl", &["--user", "enable", "--now", "framecorder-sync.service", "framecorder-update.timer"])?;
    if updated.iter().any(|u| u == "framecorder-sync") {
        quiet("systemctl", &["--user", "try-restart", "framecorder-sync.service"]);
    }
    output("systemctl", &["--user", "enable", "framecorder-ui.service"])?;
    // It starts with SteamVR from now on. Starting it while SteamVR is off
    // would start SteamVR too (it's bound to it), so only when it's on.
    if restart_ui && quiet("systemctl", &["--user", "is-active", "--quiet", "steamvr.service"]) {
        quiet("systemctl", &["--user", "restart", "framecorder-ui.service"]);
    }
    Ok(Report { updated, unlocked: panels_unlocked() })
}

/// Whether the recorder can read the panels: through the panel helper, or
/// (installs from before it) a recorder that has the permission itself.
pub fn panels_unlocked() -> bool {
    crate::grab::helper_ready() || recorder().is_ok_and(|r| unlocked(&r))
}

/// Whether the release has a different panel helper than the one in place.
fn helper_outdated(release: &Path) -> bool {
    let Some(installed) = crate::grab::helper_path() else { return false };
    match (std::fs::read(release.join("bin/framecorder-grab")), std::fs::read(installed)) {
        (Ok(new), Ok(old)) => new != old,
        _ => false,
    }
}

/// Puts the panel helper in place: owned by root, so nothing else can change
/// it, and with the permission, which updates never take away since they
/// don't touch it. Needs sudo without asking, so the password's been asked
/// for just before (the installer does that).
pub fn unlock() -> Result<()> {
    use_user_manager();
    let home = home()?;
    let from = home.join(".local/share/framecorder/release/bin/framecorder-grab");
    if !from.exists() {
        bail!("install framecorder first, the panel helper comes with it");
    }
    let to = crate::grab::helper_path().context("HOME isn't set")?;
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (from, to) = (from.to_string_lossy(), to.to_string_lossy());
    output("sudo", &["-n", "install", "-o", "root", "-g", "root", "-m", "0755", &from, &to])?;
    output("sudo", &["-n", "setcap", CAPABILITY, &to])?;
    let _ = std::fs::remove_file(home.join(RELOCK));
    // the tab picks the panels when it starts the recorder
    quiet("systemctl", &["--user", "try-restart", "framecorder-ui.service"]);
    Ok(())
}

/// What the update timer runs: installs the latest release, unless it's the
/// one that's installed already.
/// The newest release where this one came from: its url, checksum, and
/// version (releases list it under the checksum, older ones didn't).
struct Latest {
    url: String,
    sum: String,
    version: Option<String>,
}

fn latest(home: &Path) -> Result<Latest> {
    // FRAMECORDER_URL points it at another release, for testing one.
    let source = std::fs::read_to_string(home.join(SOURCE)).ok().map(|s| format!("{}/framecorder-arm64.tar.gz", s.trim()));
    let url = std::env::var("FRAMECORDER_URL").ok().or(source).unwrap_or_else(|| RELEASE_URL.to_string());
    let listed = output("curl", &["-fsSL", &format!("{url}.sha256")]).context("checking for an update")?;
    let sum = listed
        .split_whitespace()
        .next()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("the release's checksum doesn't look like one")?
        .to_ascii_lowercase();
    let version = listed.lines().find_map(|l| l.strip_prefix("version ")).map(|v| v.trim().to_string());
    Ok(Latest { url, sum, version })
}

/// Whether there's an update, as JSON for framecorder-sync to hand the apps:
/// the version that's installed, the newest one, and whether that's not
/// what's installed.
pub fn check() -> Result<String> {
    let home = home()?;
    let latest = latest(&home)?;
    let installed = std::fs::read_to_string(home.join(INSTALLED)).ok().map(|s| s.trim().to_string());
    Ok(serde_json::json!({
        "installed": env!("CARGO_PKG_VERSION"),
        "latest": latest.version,
        "available": installed.as_deref() != Some(latest.sum.as_str()),
    })
    .to_string())
}

pub fn update() -> Result<()> {
    use_user_manager();
    let home = home()?;
    let marker = home.join(INSTALLED);
    let Latest { url, sum: latest, .. } = self::latest(&home)?;
    if std::fs::read_to_string(&marker).is_ok_and(|s| s.trim() == latest) {
        log::info!("framecorder is up to date");
        return Ok(());
    }

    let work = home.join(".cache/framecorder-update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let tarball = work.join("release.tar.gz");
    let tarball_path = tarball.to_string_lossy();
    output("curl", &["-fsSL", "-o", &tarball_path, &url]).context("downloading the update")?;
    let got = output("sha256sum", &[&tarball_path])?;
    if got.split_whitespace().next() != Some(latest.as_str()) {
        bail!("the download doesn't match the release's checksum, not installing it");
    }
    output("tar", &["-xzf", &tarball_path, "-C", &work.to_string_lossy()])?;
    // The new release's own installer, so whatever it changed about
    // installing applies too.
    let installed = Command::new(work.join("framecorder-setup")).arg("--update-install").status()?;
    if !installed.success() {
        bail!("the new release's installer failed");
    }
    std::fs::write(&marker, format!("{latest}\n"))?;
    let _ = std::fs::remove_dir_all(&work);
    log::info!("updated framecorder");
    Ok(())
}

/// Runs a program and hands back what it printed, or why it failed.
fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output().with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Takes framecorder off the headset: its services, programs, settings and
/// pairings. Recordings and clips in ~/Videos/framecorder stay. Stopping the
/// tab's service ends the tab, so the tab runs this outside of it.
pub fn uninstall() -> Result<()> {
    use_user_manager();
    let home = home()?;
    for service in SERVICES {
        quiet("systemctl", &["--user", "disable", "--now", service]);
    }
    let files = PROGRAMS.iter().map(|p| home.join(".local/bin").join(p));
    let units = SERVICES.iter().map(|s| home.join(".config/systemd/user").join(s));
    let app = [DESKTOP_ENTRY, ICON].map(|f| home.join(f));
    for file in files.chain(units).chain(app) {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e).with_context(|| format!("removing {}", file.display())),
            _ => {}
        }
    }
    quiet("systemctl", &["--user", "daemon-reload"]);
    // the panel helper's root's, but the folder it's in is ours, so it can go
    let dirs = [".config/framecorder", ".local/share/framecorder", ".local/state/framecorder", ".local/lib/framecorder"];
    for dir in dirs.into_iter().chain([".cache/framecorder-update"]) {
        let dir = home.join(dir);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e).with_context(|| format!("removing {}", dir.display())),
            _ => {}
        }
    }
    log::info!("framecorder is gone. your videos are still in ~/Videos/framecorder");
    Ok(())
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
