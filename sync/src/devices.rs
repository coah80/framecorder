//! Paired devices and pairing itself. We only ever keep hashes of the
//! tokens we hand out, so devices.json leaking doesn't let anyone in.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::config::{hex, now_unix, sha256_hex, write_private};

/// Wrong codes allowed per window before /pair stops listening for a bit.
const MAX_FAILURES: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_NAME_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_sha256: String,
    pub paired_at: i64,
    pub last_seen: i64,
}

pub struct Devices {
    path: PathBuf,
    list: Mutex<Vec<Device>>,
}

fn read_list(path: &Path) -> Option<Vec<Device>> {
    let data = std::fs::read(path).ok()?;
    match serde_json::from_slice(&data) {
        Ok(list) => Some(list),
        Err(e) => {
            log::warn!("can't read {}: {e}", path.display());
            None
        }
    }
}

impl Devices {
    pub fn load(path: &Path) -> Self {
        let list = read_list(path).unwrap_or_default();
        Self { path: path.to_path_buf(), list: Mutex::new(list) }
    }

    /// Picks up changes made by the tab, like a revoked device.
    pub fn reload(&self) {
        let list = read_list(&self.path).unwrap_or_default();
        let mut cur = self.list.lock().unwrap();
        if *cur != list {
            log::info!("devices.json changed, {} paired", list.len());
            *cur = list;
        }
    }

    pub fn count(&self) -> usize {
        self.list.lock().unwrap().len()
    }

    /// Which device a bearer token belongs to, if any.
    pub fn check(&self, token: &str) -> Option<String> {
        if token.is_empty() {
            return None;
        }
        let hash = sha256_hex(token.as_bytes());
        let list = self.list.lock().unwrap();
        list.iter().find(|d| same(d.token_sha256.as_bytes(), hash.as_bytes())).map(|d| d.id.clone())
    }

    pub fn touch(&self, id: &str) {
        let now = now_unix();
        self.update(|list| {
            if let Some(d) = list.iter_mut().find(|d| d.id == id) {
                d.last_seen = now;
            }
        });
    }

    fn add(&self, device: Device) {
        self.update(move |list| list.push(device));
    }

    /// Changes the list on top of what's on disk right now, so we don't undo
    /// a revoke the tab made since our last reload.
    fn update(&self, change: impl FnOnce(&mut Vec<Device>)) {
        let mut cur = self.list.lock().unwrap();
        let mut list = read_list(&self.path).unwrap_or_else(|| cur.clone());
        change(&mut list);
        match serde_json::to_vec_pretty(&list) {
            Ok(data) => {
                if let Err(e) = write_private(&self.path, &data) {
                    log::error!("couldn't save {}: {e}", self.path.display());
                }
            }
            Err(e) => log::error!("couldn't encode devices: {e}"),
        }
        *cur = list;
    }
}

#[derive(Deserialize)]
struct Code {
    code: String,
    expires: i64,
}

#[derive(Debug, PartialEq)]
pub enum PairError {
    /// Too many wrong codes lately, try again in this many seconds.
    Limited(u64),
    NoCode,
    Expired,
    Wrong,
}

pub struct Paired {
    pub token: String,
    pub device_id: String,
}

pub struct Pairing {
    path: PathBuf,
    failures: Mutex<VecDeque<Instant>>,
    rng: SystemRandom,
}

impl Pairing {
    pub fn new(path: &Path) -> Self {
        Self { path: path.to_path_buf(), failures: Mutex::new(VecDeque::new()), rng: SystemRandom::new() }
    }

    pub fn pair(&self, code: &str, device_name: &str, devices: &Devices) -> Result<Paired, PairError> {
        self.pair_at(code, device_name, devices, now_unix(), Instant::now())
    }

    pub fn pair_at(
        &self,
        code: &str,
        device_name: &str,
        devices: &Devices,
        unix: i64,
        now: Instant,
    ) -> Result<Paired, PairError> {
        let mut failures = self.failures.lock().unwrap();
        while failures.front().is_some_and(|t| now.duration_since(*t) >= FAILURE_WINDOW) {
            failures.pop_front();
        }
        if failures.len() >= MAX_FAILURES {
            let wait = FAILURE_WINDOW.saturating_sub(now.duration_since(failures[0]));
            return Err(PairError::Limited(wait.as_secs().max(1)));
        }

        let result = self.verify(code, unix);
        if let Err(e) = result {
            failures.push_back(now);
            return Err(e);
        }
        drop(failures);

        let token = self.random_hex(32);
        let device_id = self.random_hex(8);
        devices.add(Device {
            id: device_id.clone(),
            name: clean_name(device_name),
            token_sha256: sha256_hex(token.as_bytes()),
            paired_at: unix,
            last_seen: unix,
        });
        // one code, one device
        if let Err(e) = std::fs::remove_file(&self.path) {
            log::warn!("couldn't remove {}: {e}", self.path.display());
        }
        Ok(Paired { token, device_id })
    }

    fn verify(&self, code: &str, unix: i64) -> Result<(), PairError> {
        let data = std::fs::read(&self.path).map_err(|_| PairError::NoCode)?;
        let want: Code = serde_json::from_slice(&data).map_err(|_| PairError::NoCode)?;
        if want.code.is_empty() {
            return Err(PairError::NoCode);
        }
        if unix >= want.expires {
            return Err(PairError::Expired);
        }
        if !same(code.trim().as_bytes(), want.code.as_bytes()) {
            return Err(PairError::Wrong);
        }
        Ok(())
    }

    fn random_hex(&self, len: usize) -> String {
        let mut buf = vec![0u8; len];
        self.rng.fill(&mut buf).expect("system rng failed");
        hex(&buf)
    }
}

/// Compares without bailing at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    let name = name.trim();
    if name.is_empty() {
        "device".into()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Devices, Pairing) {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::load(&dir.path().join("devices.json"));
        let pairing = Pairing::new(&dir.path().join("pairing.json"));
        (dir, devices, pairing)
    }

    fn write_code(dir: &Path, code: &str, expires: i64) {
        std::fs::write(dir.join("pairing.json"), format!(r#"{{"code":"{code}","expires":{expires}}}"#)).unwrap();
    }

    #[test]
    fn pairs_with_the_right_code_once() {
        let (dir, devices, pairing) = setup();
        write_code(dir.path(), "123456", 2000);
        let now = Instant::now();
        let paired = pairing.pair_at("123456", "desk", &devices, 1000, now).unwrap();
        assert_eq!(paired.token.len(), 64);
        assert_eq!(devices.check(&paired.token), Some(paired.device_id.clone()));
        assert!(!dir.path().join("pairing.json").exists());

        // the code is gone now
        assert_eq!(pairing.pair_at("123456", "other", &devices, 1001, now).err(), Some(PairError::NoCode));

        // only the hash is on disk
        let saved = std::fs::read_to_string(dir.path().join("devices.json")).unwrap();
        assert!(!saved.contains(&paired.token));
        assert!(saved.contains(&sha256_hex(paired.token.as_bytes())));
    }

    #[test]
    fn rejects_wrong_and_expired_codes() {
        let (dir, devices, pairing) = setup();
        let now = Instant::now();
        assert_eq!(pairing.pair_at("123456", "x", &devices, 1000, now).err(), Some(PairError::NoCode));
        write_code(dir.path(), "123456", 2000);
        assert_eq!(pairing.pair_at("654321", "x", &devices, 1000, now).err(), Some(PairError::Wrong));
        assert_eq!(pairing.pair_at("123456", "x", &devices, 2000, now).err(), Some(PairError::Expired));
        assert_eq!(devices.count(), 0);
        assert!(dir.path().join("pairing.json").exists());
    }

    #[test]
    fn rate_limits_guessing() {
        let (dir, devices, pairing) = setup();
        write_code(dir.path(), "123456", 2000);
        let start = Instant::now();
        for i in 0..MAX_FAILURES {
            let guess = format!("{:06}", i);
            assert_eq!(pairing.pair_at(&guess, "x", &devices, 1000, start).err(), Some(PairError::Wrong));
        }
        // even the right code is refused while limited
        let limited = pairing.pair_at("123456", "x", &devices, 1000, start + Duration::from_secs(10));
        assert_eq!(limited.err(), Some(PairError::Limited(50)));
        // and it lets up after the window
        let later = start + FAILURE_WINDOW + Duration::from_secs(1);
        assert!(pairing.pair_at("123456", "x", &devices, 1000, later).is_ok());
    }

    #[test]
    fn token_check() {
        let (dir, devices, pairing) = setup();
        write_code(dir.path(), "000111", 2000);
        let paired = pairing.pair_at("000111", "phone", &devices, 1000, Instant::now()).unwrap();
        assert!(devices.check(&paired.token).is_some());
        assert!(devices.check("").is_none());
        assert!(devices.check("nope").is_none());
        assert!(devices.check(&paired.token[..63]).is_none());
        assert!(devices.check(&sha256_hex(paired.token.as_bytes())).is_none());
    }

    #[test]
    fn revoking_on_disk_takes_effect_and_sticks() {
        let (dir, devices, pairing) = setup();
        write_code(dir.path(), "111111", 2000);
        let a = pairing.pair_at("111111", "a", &devices, 1000, Instant::now()).unwrap();
        write_code(dir.path(), "222222", 2000);
        let b = pairing.pair_at("222222", "b", &devices, 1000, Instant::now()).unwrap();
        assert_eq!(devices.count(), 2);

        // the tab removes "a"
        let path = dir.path().join("devices.json");
        let list: Vec<Device> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let kept: Vec<_> = list.into_iter().filter(|d| d.id != a.device_id).collect();
        std::fs::write(&path, serde_json::to_vec(&kept).unwrap()).unwrap();

        // a write of ours before the reload must not bring it back
        devices.touch(&b.device_id);
        assert!(devices.check(&a.token).is_none());
        devices.reload();
        assert!(devices.check(&a.token).is_none());
        assert!(devices.check(&b.token).is_some());
    }

    #[test]
    fn cleans_device_names() {
        assert_eq!(clean_name("  "), "device");
        assert_eq!(clean_name("pixel\n8"), "pixel8");
        assert_eq!(clean_name(&"x".repeat(200)).len(), MAX_NAME_LEN);
    }
}
