//! the tauri app this one replaces. it syncs into the same folder from the
//! same state, lives in the tray and starts with the computer, so after
//! someone downloads this app the old one is usually still running, and the
//! two would download everything twice. it can't update itself, so this is
//! where it stops.
//!
//! it's found by the marker its single-instance plugin
//! (tauri-plugin-single-instance 2.4, no semver feature) leaves for
//! `com.framecorder.app`, never by process name.

pub fn close() -> Result<bool, String> {
    close_process(find())
}

fn close_process(pid: Option<u32>) -> Result<bool, String> {
    let Some(pid) = pid else {
        return Ok(false);
    };
    if pid == std::process::id() {
        return Ok(false);
    }
    log::info!("the old framecorder app is running (pid {pid}), closing it");
    end(pid).map_err(|e| {
        log::warn!("couldn't close the old framecorder app: {e}");
        e
    })?;
    Ok(true)
}

/// its hidden window, named after the identifier
#[cfg(windows)]
fn find() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (class, title) = (wide("com.framecorder.app-sic"), wide("com.framecorder.app-siw"));
    let hwnd = unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

#[cfg(windows)]
fn end(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // its state is written atomically, and a half-done download is picked
        // up from its .part, so ending it outright loses nothing
        let ended = TerminateProcess(handle, 0) != 0 && WaitForSingleObject(handle, 3000) == WAIT_OBJECT_0;
        let err = std::io::Error::last_os_error();
        CloseHandle(handle);
        if ended {
            Ok(())
        } else {
            Err(err.to_string())
        }
    }
}

/// the name it holds on the session bus
#[cfg(target_os = "linux")]
fn find() -> Option<u32> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&conn).ok()?;
    let name = zbus::names::BusName::try_from("com.framecorder.app.SingleInstance").ok()?;
    bus.get_connection_unix_process_id(name).ok()
}

/// the socket it listens on
#[cfg(target_os = "macos")]
fn find() -> Option<u32> {
    let out = std::process::Command::new("lsof").args(["-t", "/tmp/com_framecorder_app_si.sock"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .find(|&p| p != std::process::id())
}

#[cfg(unix)]
fn end(pid: u32) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let pid = pid as libc::pid_t;
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if unsafe { libc::kill(pid, 0) } != 0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("it didn't close within 3 s".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_old_app_to_close() {
        assert!(!close_process(None).unwrap());
        assert!(!close_process(Some(std::process::id())).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn old_app_refusing_to_close_is_an_error() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let mut old = Command::new("sh")
            .args(["-c", "trap '' TERM; printf 'ready\n'; read ignored"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(old.stdout.take().unwrap()).read_line(&mut ready).unwrap();
        let result = close_process(Some(old.id()));
        old.kill().unwrap();
        old.wait().unwrap();
        assert_eq!(ready, "ready\n");
        assert!(result.unwrap_err().contains("didn't close"));
    }
}
