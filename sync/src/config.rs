//! Where things live, the files we share with the dashboard tab, and our
//! TLS identity.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 38619;
/// How fast transfers may go while a game is running, in MB/s. 0 means no
/// cap: sending a file doesn't touch the GPU or the encoder, so full speed
/// doesn't slow a game down.
pub const DEFAULT_GAME_RATE_MB: f64 = 0.0;

#[derive(Clone, Debug)]
pub struct Paths {
    pub videos: PathBuf,
    pub clips: PathBuf,
    pub state: PathBuf,
    /// Where the kernel lists batteries.
    pub power: PathBuf,
}

impl Paths {
    pub fn from_env() -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("HOME isn't set"))?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let mut paths = Self::new(&home.join("Videos/framecorder"), &config.join("framecorder/sync"));
        // for trying it somewhere without the headset's battery
        if let Some(power) = std::env::var_os("FRAMECORDER_POWER_SUPPLY") {
            paths.power = PathBuf::from(power);
        }
        Ok(paths)
    }

    pub fn new(videos: &Path, state: &Path) -> Self {
        Self {
            videos: videos.to_path_buf(),
            clips: videos.join("clips"),
            state: state.to_path_buf(),
            power: PathBuf::from("/sys/class/power_supply"),
        }
    }

    pub fn info(&self) -> PathBuf {
        self.state.join("info.json")
    }
    pub fn pairing(&self) -> PathBuf {
        self.state.join("pairing.json")
    }
    pub fn devices(&self) -> PathBuf {
        self.state.join("devices.json")
    }
    pub fn settings(&self) -> PathBuf {
        self.state.join("settings.json")
    }
    /// What a phone asked the tab to do.
    pub fn command(&self) -> PathBuf {
        self.state.join("command.json")
    }
    /// What the tab says back: how that went, and whether it's recording.
    pub fn status(&self) -> PathBuf {
        self.state.join("status.json")
    }
    /// The dashboard tab's own settings, next to our folder.
    pub fn recording(&self) -> PathBuf {
        self.state.parent().unwrap_or(&self.state).join("ui.conf")
    }
}

/// `settings.json`. Everything is optional, the tab only ever writes
/// `delete_after_sync`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub delete_after_sync: bool,
    pub port: Option<u16>,
    pub name: Option<String>,
    /// MB/s while a game is running. 0 turns the limit off.
    pub game_rate_mb: Option<f64>,
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        match std::fs::read(paths.settings()) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|e| {
                log::warn!("ignoring settings.json: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}

#[derive(Serialize)]
pub struct Info<'a> {
    pub name: &'a str,
    pub port: u16,
    pub fingerprint: &'a str,
    pub version: u32,
}

/// Writes to a temp file next to `path` and renames it over, so readers
/// never see half a file.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

pub fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(data)?;
    std::fs::rename(&tmp, path)
}

pub fn device_name(settings: &Settings) -> String {
    if let Some(name) = settings.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        return name.to_string();
    }
    let mut buf = [0u8; 256];
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let host = if ok {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).trim().to_string()
    } else {
        String::new()
    };
    if host.is_empty() || host == "localhost" {
        "Steam Frame".into()
    } else {
        host
    }
}

pub struct Identity {
    pub certs: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    pub fingerprint: String,
}

/// Loads cert.pem/key.pem, making a new self-signed pair the first time.
pub fn identity(state: &Path) -> io::Result<Identity> {
    let cert_path = state.join("cert.pem");
    let key_path = state.join("key.pem");
    if !cert_path.exists() || !key_path.exists() {
        log::info!("making a new certificate in {}", state.display());
        let names = vec!["framecorder.local".to_string(), "localhost".to_string()];
        let made = rcgen::generate_simple_self_signed(names).map_err(io::Error::other)?;
        write_private(&key_path, made.signing_key.serialize_pem().as_bytes())?;
        write_atomic(&cert_path, made.cert.pem().as_bytes())?;
    }
    let certs: Vec<_> = CertificateDer::pem_file_iter(&cert_path)
        .and_then(|it| it.collect::<Result<_, _>>())
        .map_err(|e| io::Error::other(format!("bad cert.pem: {e}")))?;
    let key = PrivateKeyDer::from_pem_file(&key_path).map_err(|e| io::Error::other(format!("bad key.pem: {e}")))?;
    let first = certs.first().ok_or_else(|| io::Error::other("cert.pem is empty"))?;
    let fingerprint = sha256_hex(first.as_ref());
    Ok(Identity { certs, key, fingerprint })
}

pub fn tls_config(id: &Identity) -> io::Result<Arc<rustls::ServerConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(id.certs.clone(), id.key.clone_key())
        .map_err(io::Error::other)?;
    Ok(Arc::new(config))
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, data).as_ref())
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
