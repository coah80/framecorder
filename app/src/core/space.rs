//! Whether a download fits on this device, checked before starting it
//! rather than finding out the hard way a few gigabytes in.

use std::path::Path;

use super::api::ApiError;

/// Kept free on top of the file itself, so syncing never takes the last
/// of the disk.
const MARGIN: u64 = 200_000_000;

/// Bytes free for us where `path` is (or will be), if it can be told.
pub fn free(path: &Path) -> Option<u64> {
    // the folder may not exist yet: ask about the closest one that does
    let existing = path.ancestors().find(|p| p.exists())?;
    free_at(existing)
}

#[cfg(unix)]
fn free_at(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

#[cfg(windows)]
fn free_at(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(any(unix, windows)))]
fn free_at(_: &Path) -> Option<u64> {
    None
}

/// Checks a file of `size` fits, given `have` bytes of it are here already
/// and `copies` of it exist at once while it's being put in place.
pub fn room_for(size: u64, have: u64, copies: u64, free: Option<u64>) -> Result<(), ApiError> {
    // can't tell: try, the write will say if it doesn't fit
    let Some(free) = free else { return Ok(()) };
    let need = size.saturating_sub(have) + size * copies.saturating_sub(1) + MARGIN;
    if free < need {
        return Err(ApiError::Full { need, free });
    }
    Ok(())
}

/// Whether a failed write was the disk filling up.
pub fn is_full(e: &std::io::Error) -> bool {
    // ENOSPC and EDQUOT; ERROR_HANDLE_DISK_FULL and ERROR_DISK_FULL
    let codes: &[i32] = if cfg!(windows) { &[39, 112] } else { &[28, 122] };
    e.kind() == std::io::ErrorKind::StorageFull || e.raw_os_error().is_some_and(|c| codes.contains(&c))
}

pub fn human(bytes: u64) -> String {
    let (mut n, mut unit) = (bytes as f64, 0);
    while n >= 1000.0 && unit < 4 {
        n /= 1000.0;
        unit += 1;
    }
    let digits = if n < 10.0 && unit > 0 { 1 } else { 0 };
    format!("{n:.digits$} {}", ["B", "KB", "MB", "GB", "TB"][unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn knows_how_much_is_free() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free(dir.path()).unwrap() > 0);
        // a folder that isn't there yet counts as where it'll be
        assert!(free(&dir.path().join("clips/not/yet")).unwrap() > 0);
    }

    #[test]
    fn needs_the_file_plus_a_margin() {
        assert!(room_for(GB, 0, 1, Some(2 * GB)).is_ok());
        assert_eq!(room_for(GB, 0, 1, Some(GB)), Err(ApiError::Full { need: GB + MARGIN, free: GB }));
        // what's already downloaded doesn't need space again
        assert!(room_for(4 * GB, 3 * GB + GB / 2, 1, Some(GB)).is_ok());
        // android holds the download and the gallery's copy for a moment
        assert!(room_for(GB, 0, 2, Some(2 * GB)).is_err());
        assert!(room_for(GB, 0, 2, Some(3 * GB)).is_ok());
        assert!(room_for(GB, 0, 1, None).is_ok());
    }

    #[test]
    fn says_sizes_like_people_do() {
        assert_eq!(human(3_771_000_000), "3.8 GB");
        assert_eq!(human(151_000_000), "151 MB");
    }
}
