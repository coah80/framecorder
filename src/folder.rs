//! Where recordings go: ~/Videos/framecorder, or a framecorder folder on a
//! drive plugged into the headset, a microSD card or a USB drive, which
//! SteamOS mounts under /run/media. A picked drive that's been taken out
//! falls back to ~/Videos/framecorder, never to the empty folder it leaves
//! behind, which is in RAM. Shared with framecorder-sync.

use std::path::{Path, PathBuf};

const MOUNTS: &str = "/proc/self/mountinfo";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// The framecorder folder on it, what `folder=` in ui.conf holds.
    pub folder: PathBuf,
    pub name: String,
}

/// ~/Videos/framecorder.
pub fn home() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Videos/framecorder"))
}

/// The drives plugged in right now.
pub fn drives() -> Vec<Drive> {
    std::fs::read_to_string(MOUNTS).map(|m| drives_in(&mounts(&m))).unwrap_or_default()
}

/// Where recordings go for `picked` (empty for ~/Videos/framecorder).
pub fn current(picked: &str) -> Option<PathBuf> {
    if !picked.is_empty() && valid(picked) {
        let table = std::fs::read_to_string(MOUNTS).unwrap_or_default();
        if on_a_drive(&mounts(&table), Path::new(picked)) {
            return Some(PathBuf::from(picked));
        }
    }
    home()
}

/// What ui.conf may hold: nothing, or an absolute path with no way back up.
pub fn valid(picked: &str) -> bool {
    let path = Path::new(picked);
    picked.is_empty()
        || (path.is_absolute()
            && !picked.contains(['\n', '\r', '\0'])
            && path.components().all(|c| !matches!(c, std::path::Component::ParentDir)))
}

/// A short name for a picked folder whose drive isn't in: its drive's
/// folder name, the way the drive was labeled.
pub fn name_of(picked: &str) -> String {
    let path = Path::new(picked);
    let drive = if path.file_name().is_some_and(|n| n == "framecorder") { path.parent().unwrap_or(path) } else { path };
    drive.file_name().map_or_else(|| picked.to_string(), |n| n.to_string_lossy().into_owned())
}

struct Mount {
    point: PathBuf,
    fstype: String,
    source: String,
}

/// The mount table: where each file system is, what it is and what it's on.
fn mounts(table: &str) -> Vec<Mount> {
    table
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = unescape(left.split(' ').nth(4)?);
            let mut right = right.split(' ');
            Some(Mount { point: PathBuf::from(point), fstype: right.next()?.to_string(), source: unescape(right.next()?) })
        })
        .collect()
}

/// mountinfo writes a space as \040 and so on.
fn unescape(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let octal = b.get(i + 1..i + 4).and_then(|d| std::str::from_utf8(d).ok()).and_then(|d| u8::from_str_radix(d, 8).ok());
        match (b[i], octal) {
            (b'\\', Some(c)) => {
                out.push(c);
                i += 4;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// File systems that live in memory, where a recording must never go.
fn in_memory(fstype: &str) -> bool {
    matches!(fstype, "tmpfs" | "ramfs" | "devtmpfs" | "proc" | "sysfs")
}

fn drives_in(table: &[Mount]) -> Vec<Drive> {
    table
        .iter()
        .filter(|m| m.point.starts_with("/run/media") && m.point.components().count() > 3 && !in_memory(&m.fstype))
        .map(|m| {
            let name = if m.source.starts_with("/dev/mmcblk") {
                "microSD card".to_string()
            } else {
                m.point.file_name().map_or_else(|| m.source.clone(), |n| n.to_string_lossy().into_owned())
            };
            Drive { folder: m.point.join("framecorder"), name }
        })
        .collect()
}

/// Whether `path` is on a real disk: what's mounted deepest above it isn't
/// in memory. An SD card that's been taken out leaves its folder on /run.
fn on_a_drive(table: &[Mount], path: &Path) -> bool {
    table
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count())
        .is_some_and(|m| !in_memory(&m.fstype))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from a Frame with a microSD card and a USB drive in.
    const TABLE: &str = "\
22 1 8:5 / / ro,relatime shared:1 - btrfs /dev/sda5 ro
30 22 0:25 / /run rw,nosuid,nodev shared:5 - tmpfs tmpfs rw,mode=755
40 22 8:8 / /home rw,relatime shared:20 - ext4 /dev/sda8 rw
91 30 179:1 / /run/media/steamos/SD\\040CARD rw,nosuid,nodev,relatime shared:50 - ext4 /dev/mmcblk0p1 rw
92 30 8:17 / /run/media/steamos/BACKUP rw,nosuid,nodev,relatime shared:51 - exfat /dev/sdb1 rw
";

    #[test]
    fn drives_are_whats_mounted_under_run_media() {
        let drives = drives_in(&mounts(TABLE));
        assert_eq!(
            drives,
            [
                Drive { folder: "/run/media/steamos/SD CARD/framecorder".into(), name: "microSD card".into() },
                Drive { folder: "/run/media/steamos/BACKUP/framecorder".into(), name: "BACKUP".into() },
            ]
        );
    }

    #[test]
    fn a_drive_taken_out_isnt_somewhere_to_record() {
        let table = mounts(TABLE);
        assert!(on_a_drive(&table, Path::new("/run/media/steamos/SD CARD/framecorder")));
        assert!(on_a_drive(&table, Path::new("/home/steamos/Videos/framecorder")));
        // gone: what's left above it is /run, in memory
        assert!(!on_a_drive(&table, Path::new("/run/media/steamos/OLD/framecorder")));
        assert_eq!(name_of("/run/media/steamos/OLD/framecorder"), "OLD");
    }

    #[test]
    fn only_plain_absolute_paths_get_saved() {
        assert!(valid(""));
        assert!(valid("/run/media/steamos/SD CARD/framecorder"));
        assert!(!valid("Videos"));
        assert!(!valid("/run/media/../../etc"));
        assert!(!valid("/a\nclip=off"));
    }
}
