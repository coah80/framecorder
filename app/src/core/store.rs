//! What the app remembers: paired Frames and every clip it already has, so
//! nothing downloads twice.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::api::RemoteClip;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub fingerprint: String,
    pub name: String,
    /// Last address that worked, `ip:port`.
    pub addr: String,
    pub token: String,
    pub device_id: String,
    pub paired_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub host: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub created: i64,
    #[serde(default)]
    pub duration_s: Option<f64>,
    /// A file path on desktop, a content:// uri on Android.
    pub location: String,
    pub synced_at: i64,
}

impl Entry {
    pub fn key(&self) -> String {
        format!("{}/{}", &self.host[..self.host.len().min(16)], self.id)
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

fn load<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    match std::fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|e| {
            log::warn!("{} is unreadable, starting over: {e}", path.display());
            T::default()
        }),
        Err(_) => T::default(),
    }
}

pub struct Hosts {
    path: PathBuf,
    list: Vec<Host>,
}

impl Hosts {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("hosts.json");
        let list = load(&path);
        Self { path, list }
    }

    pub fn list(&self) -> &[Host] {
        &self.list
    }

    pub fn get(&self, fingerprint: &str) -> Option<&Host> {
        self.list.iter().find(|h| h.fingerprint == fingerprint)
    }

    pub fn upsert(&mut self, host: Host) -> io::Result<()> {
        self.list.retain(|h| h.fingerprint != host.fingerprint);
        self.list.push(host);
        self.save()
    }

    pub fn remove(&mut self, fingerprint: &str) -> io::Result<()> {
        self.list.retain(|h| h.fingerprint != fingerprint);
        self.save()
    }

    pub fn set_addr(&mut self, fingerprint: &str, addr: &str) -> io::Result<()> {
        match self.list.iter_mut().find(|h| h.fingerprint == fingerprint) {
            Some(h) if h.addr != addr => {
                h.addr = addr.to_string();
                self.save()
            }
            _ => Ok(()),
        }
    }

    fn save(&self) -> io::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec_pretty(&self.list)?)
    }
}

pub struct Index {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Index {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("index.json");
        let entries = load(&path);
        Self { path, entries }
    }

    pub fn contains(&self, host: &str, id: &str) -> bool {
        self.entries.iter().any(|e| e.host == host && e.id == id)
    }

    /// What the Frame has that we don't: clips before recordings (they're
    /// small and usually what you want first), newest first within each.
    pub fn missing(&self, host: &str, remote: &[RemoteClip]) -> Vec<RemoteClip> {
        let mut out: Vec<RemoteClip> = remote.iter().filter(|c| !self.contains(host, &c.id)).cloned().collect();
        out.sort_by(|a, b| (a.kind != "clip").cmp(&(b.kind != "clip")).then(b.created.cmp(&a.created)));
        out
    }

    pub fn insert(&mut self, entry: Entry) -> io::Result<()> {
        self.entries.retain(|e| !(e.host == entry.host && e.id == entry.id));
        self.entries.push(entry);
        self.save()
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key() == key)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Entry> {
        let mut list = self.entries.clone();
        list.sort_by(|a, b| b.created.cmp(&a.created).then(b.synced_at.cmp(&a.synced_at)));
        list
    }

    fn save(&self) -> io::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec(&self.entries)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(id: &str, kind: &str, created: i64) -> RemoteClip {
        RemoteClip { id: id.into(), kind: kind.into(), name: format!("{id}.mp4"), size: 10, duration_s: None, created }
    }

    fn entry(host: &str, id: &str) -> Entry {
        Entry {
            host: host.into(),
            id: id.into(),
            name: format!("{id}.mp4"),
            kind: "clip".into(),
            size: 10,
            created: 1,
            duration_s: None,
            location: "/x".into(),
            synced_at: 2,
        }
    }

    #[test]
    fn diff_skips_what_we_have_and_orders_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index::load(dir.path());
        index.insert(entry("fp1", "c-a")).unwrap();
        index.insert(entry("fp2", "c-b")).unwrap();

        let remote = vec![remote("c-a", "clip", 5), remote("c-b", "clip", 1), remote("r-c", "recording", 9), remote("c-d", "clip", 7)];
        let ids: Vec<String> = index.missing("fp1", &remote).into_iter().map(|c| c.id).collect();
        // c-b is only synced from another Frame, so it's still missing here
        assert_eq!(ids, ["c-d", "c-b", "r-c"]);

        // survives a restart, no duplicates on re-insert
        index.insert(entry("fp1", "c-a")).unwrap();
        let again = Index::load(dir.path());
        assert_eq!(again.list().len(), 2);
        assert!(again.contains("fp1", "c-a"));
        assert!(!again.contains("fp1", "c-b"));
        assert!(again.get(&entry("fp2", "c-b").key()).is_some());
    }

    #[test]
    fn hosts_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut hosts = Hosts::load(dir.path());
        let host = Host {
            fingerprint: "ab".repeat(32),
            name: "Frame".into(),
            addr: "10.0.0.2:38619".into(),
            token: "t".into(),
            device_id: "d".into(),
            paired_at: 1,
        };
        hosts.upsert(host.clone()).unwrap();
        hosts.set_addr(&host.fingerprint, "10.0.0.3:38619").unwrap();
        let again = Hosts::load(dir.path());
        assert_eq!(again.get(&host.fingerprint).unwrap().addr, "10.0.0.3:38619");
        let mut again = again;
        again.remove(&host.fingerprint).unwrap();
        assert!(Hosts::load(dir.path()).list().is_empty());
    }

    #[test]
    fn corrupt_files_start_fresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.json"), "{nope").unwrap();
        assert!(Index::load(dir.path()).list().is_empty());
    }
}
