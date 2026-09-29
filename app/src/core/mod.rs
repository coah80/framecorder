//! Everything that isn't UI: finding, pairing with and syncing from Frames.
//! Shared by the desktop app, the Android app and the headless mode.

pub mod api;
pub mod discover;
pub mod engine;
pub mod pairlink;
pub mod space;
pub mod store;
pub mod tls;

/// What this device calls itself when pairing.
pub fn device_name() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").ok();
    #[cfg(unix)]
    let name = {
        let mut buf = [0u8; 256];
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        ok.then(|| String::from_utf8_lossy(&buf[..end]).into_owned())
    };
    #[cfg(not(any(windows, unix)))]
    let name: Option<String> = None;
    name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "desktop".into())
}
