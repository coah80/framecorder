//! How much room recordings have left, from the file system they go to.

use std::path::Path;

/// Free bytes where recordings are saved, or where they would be once the
/// folder exists.
pub fn free_for_recordings(folder: &Path) -> Option<u64> {
    free_at(folder.ancestors().find(|d| d.is_dir())?)
}

fn free_at(dir: &Path) -> Option<u64> {
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
        assert!(free_at(Path::new("/")).is_some());
        assert_eq!(free_at(Path::new("/definitely/not/here")), None);
    }
}
