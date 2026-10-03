//! Pairing phones and computers with the sync service (framecorder-sync):
//! the tab writes a one-time code for it to accept, and shows a QR code with
//! everything the app needs to find and trust this headset.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// How long a code works for.
const VALID_FOR: Duration = Duration::from_secs(600);

pub struct Pairing {
    pub code: String,
    expires: u64,
    pub name: String,
    /// QR modules, row by row, `size` wide.
    pub qr: Vec<bool>,
    pub size: usize,
}

pub struct Device {
    pub id: String,
    pub name: String,
}

pub(super) fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("framecorder/sync"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read_json(name: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir()?.join(name)).ok()?).ok()
}

fn random_code() -> Result<String> {
    let mut bytes = [0u8; 4];
    let n = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    if n != bytes.len() as isize {
        bail!("no randomness for a pairing code");
    }
    Ok(format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000))
}

/// This headset's address on the local network, for the QR code.
fn local_ip() -> Option<String> {
    let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
        return None;
    }
    let mut found = None;
    let mut cur = addrs;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || unsafe { (*ifa.ifa_addr).sa_family } as i32 != libc::AF_INET {
            continue;
        }
        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }.to_string_lossy();
        if ip.is_loopback() || name.starts_with("docker") || name.starts_with("veth") {
            continue;
        }
        // Wi-Fi first, that's where phones are.
        if name.starts_with("wl") || found.is_none() {
            found = Some(ip.to_string());
        }
    }
    unsafe { libc::freeifaddrs(addrs) };
    found
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Pairing {
    /// Writes a fresh code for the sync service and builds the QR code.
    pub fn begin() -> Result<Self> {
        let info = read_json("info.json").context("the sync service isn't running on this headset")?;
        let port = info["port"].as_u64().context("the sync service's info.json has no port")?;
        let fingerprint = info["fingerprint"].as_str().context("the sync service's info.json has no fingerprint")?;
        let name = info["name"].as_str().unwrap_or("Steam Frame").to_string();
        let code = random_code()?;
        let expires = now() + VALID_FOR.as_secs();

        let path = dir().context("HOME isn't set")?.join("pairing.json");
        let text = json!({ "code": code, "expires": expires }).to_string();
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .with_context(|| format!("writing {}", path.display()))?;
            f.write_all(text.as_bytes())?;
        }

        // The address only goes in the QR code, as a head start for phones;
        // the app finds the Frame by name and certificate on the network.
        let host = local_ip();
        let url = format!(
            "framecorder://pair?host={}&port={port}&fp={fingerprint}&code={code}&name={}",
            host.as_deref().unwrap_or(""),
            url_encode(&name)
        );
        let qr = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M)?;
        let size = qr.width();
        let modules = qr.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
        Ok(Self { code, expires, name, qr: modules, size })
    }

    /// A made up one, for previewing the screen.
    pub fn sample() -> Result<Self> {
        let url = "framecorder://pair?host=192.168.1.20&port=38619&fp=00&code=482913&name=Steam%20Frame";
        let qr = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M)?;
        Ok(Self {
            code: "482913".into(),
            expires: now() + VALID_FOR.as_secs(),
            name: "Steam Frame".into(),
            size: qr.width(),
            qr: qr.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect(),
        })
    }

    pub fn expires_in(&self) -> Duration {
        Duration::from_secs(self.expires.saturating_sub(now()))
    }

    /// Whether the code is still waiting to be used (the service deletes it
    /// after a pairing).
    pub fn waiting(&self) -> bool {
        read_json("pairing.json").is_some_and(|v| v["code"].as_str() == Some(self.code.as_str())) && self.expires_in() > Duration::ZERO
    }
}

/// Whether the sync service has been set up and started on this headset.
pub fn available() -> bool {
    read_json("info.json").is_some()
}

/// Takes the code back so nobody can use it after pairing's been closed.
pub fn end() {
    if let Some(d) = dir() {
        let _ = std::fs::remove_file(d.join("pairing.json"));
    }
}

pub fn devices() -> Vec<Device> {
    let Some(Value::Array(list)) = read_json("devices.json") else { return Vec::new() };
    list.iter()
        .filter_map(|d| Some(Device { id: d["id"].as_str()?.to_string(), name: d["name"].as_str().unwrap_or("a device").to_string() }))
        .collect()
}

/// Forgets a device; the sync service locks it out right away.
pub fn remove(id: &str) -> Result<()> {
    let path = dir().context("HOME isn't set")?.join("devices.json");
    let Some(Value::Array(list)) = read_json("devices.json") else { return Ok(()) };
    let kept: Vec<Value> = list.into_iter().filter(|d| d["id"].as_str() != Some(id)).collect();
    // Write then rename, so the service never reads half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, Value::Array(kept).to_string())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_names_for_the_url() {
        assert_eq!(url_encode("Sam's Frame"), "Sam%27s%20Frame");
        assert_eq!(url_encode("frame-1"), "frame-1");
    }

    #[test]
    fn codes_are_six_digits() {
        let c = random_code().unwrap();
        assert_eq!(c.len(), 6);
        assert!(c.bytes().all(|b| b.is_ascii_digit()));
    }
}
