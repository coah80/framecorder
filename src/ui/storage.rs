//! How much room recordings have left, from the file system they go to.

use std::path::PathBuf;

/// Free bytes where recordings are saved, or where they would be once the
/// folder exists.
pub fn free_for_recordings() -> Option<u64> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let videos = home.join("Videos/framecorder");
    let dir = if videos.is_dir() { videos } else { home };
    free_at(&dir)
}

fn free_at(dir: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    Some(stat.f_bavail * stat.f_frsize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_real_directory_and_not_a_missing_one() {
        assert!(free_at(std::path::Path::new("/")).is_some());
        assert_eq!(free_at(std::path::Path::new("/definitely/not/here")), None);
    }
}
