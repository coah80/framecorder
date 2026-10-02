//! inotify on the video folders and our state folder. Blocks in read() the
//! whole time, so it costs nothing while nothing happens.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::Arc;

use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};

use crate::config::Paths;
use crate::devices::Devices;
use crate::events::Hub;
use crate::library::{Change, Library};

pub struct Watcher {
    inotify: Inotify,
    paths: Paths,
    library: Arc<Library>,
    hub: Arc<Hub>,
    devices: Arc<Devices>,
    parent: Option<WatchDescriptor>,
    videos: Option<WatchDescriptor>,
    clips: Option<WatchDescriptor>,
    state: Option<WatchDescriptor>,
}

fn files_mask() -> WatchMask {
    WatchMask::MOVED_TO | WatchMask::MOVED_FROM | WatchMask::CLOSE_WRITE | WatchMask::DELETE
}

impl Watcher {
    pub fn new(paths: &Paths, library: Arc<Library>, hub: Arc<Hub>, devices: Arc<Devices>) -> io::Result<Self> {
        let inotify = Inotify::init()?;
        let mut w = Self {
            inotify,
            paths: paths.clone(),
            library,
            hub,
            devices,
            parent: None,
            videos: None,
            clips: None,
            state: None,
        };
        if let Some(parent) = paths.videos.parent() {
            w.parent = w.inotify.watches().add(parent, WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::ONLYDIR).ok();
        }
        w.state = Some(w.inotify.watches().add(&paths.state, files_mask() | WatchMask::ONLYDIR)?);
        w.watch_videos();
        // scan after the watches are up so nothing slips between the two
        w.library.rescan();
        Ok(w)
    }

    fn watch_videos(&mut self) {
        let mut watches = self.inotify.watches();
        if self.videos.is_none() {
            self.videos = watches.add(&self.paths.videos, files_mask() | WatchMask::CREATE | WatchMask::ONLYDIR).ok();
        }
        if self.clips.is_none() {
            self.clips = watches.add(&self.paths.clips, files_mask() | WatchMask::ONLYDIR).ok();
        }
    }

    pub fn run(mut self) {
        let mut buf = [0u8; 4096];
        loop {
            let events: Vec<(WatchDescriptor, EventMask, Option<std::ffi::OsString>)> =
                match self.inotify.read_events_blocking(&mut buf) {
                    Ok(events) => events.map(|e| (e.wd, e.mask, e.name.map(OsStr::to_os_string))).collect(),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        log::error!("inotify stopped working: {e}");
                        return;
                    }
                };
            for (wd, mask, name) in events {
                self.handle(&wd, mask, name.as_deref());
            }
        }
    }

    fn handle(&mut self, wd: &WatchDescriptor, mask: EventMask, name: Option<&OsStr>) {
        if mask.contains(EventMask::Q_OVERFLOW) {
            log::warn!("missed some file events, rescanning");
            self.publish(self.library.rescan());
            return;
        }
        let is = |w: &Option<WatchDescriptor>| w.as_ref() == Some(wd);

        if mask.contains(EventMask::IGNORED) {
            // the folder itself went away
            let dir = if is(&self.videos) {
                self.videos = None;
                self.paths.videos.clone()
            } else if is(&self.clips) {
                self.clips = None;
                self.paths.clips.clone()
            } else {
                return;
            };
            log::info!("{} is gone", dir.display());
            let gone = self.library.drop_dir(&dir);
            self.publish(gone.into_iter().map(Change::Removed).collect());
            return;
        }

        let Some(name) = name.and_then(OsStr::to_str) else { return };

        if is(&self.state) {
            if name == "devices.json" {
                self.devices.reload();
            }
            // the tab writes it whole and renames it into place
            if name == "status.json" && mask.contains(EventMask::MOVED_TO) {
                self.hub.broadcast("remote", &crate::remote::status(&self.paths).to_string());
            }
            return;
        }

        if mask.contains(EventMask::ISDIR) {
            let appeared = mask.intersects(EventMask::CREATE | EventMask::MOVED_TO);
            let ours = (is(&self.parent) && Some(OsStr::new(name)) == self.paths.videos.file_name())
                || (is(&self.videos) && Some(OsStr::new(name)) == self.paths.clips.file_name());
            if appeared && ours {
                self.watch_videos();
                self.publish(self.library.rescan());
            }
            return;
        }

        let dir = if is(&self.videos) {
            &self.paths.videos
        } else if is(&self.clips) {
            &self.paths.clips
        } else {
            return;
        };
        let path = dir.join(name);
        if mask.intersects(EventMask::MOVED_TO | EventMask::CLOSE_WRITE) {
            if let Some(clip) = self.library.add(&path) {
                log::info!("new: {}", clip.name);
                self.hub.send(&Change::New(clip));
            }
        } else if mask.intersects(EventMask::MOVED_FROM | EventMask::DELETE) {
            if let Some(id) = self.library.remove(&path) {
                log::info!("removed: {name}");
                self.hub.send(&Change::Removed(id));
            }
        }
    }

    fn publish(&self, changes: Vec<Change>) {
        for change in &changes {
            self.hub.send(change);
        }
    }
}

pub fn ensure_dirs(paths: &Paths) -> io::Result<()> {
    std::fs::create_dir_all(&paths.clips)?;
    create_private_dir(&paths.state)
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::fake_mp4;
    use std::time::Duration;

    #[test]
    fn rename_to_mp4_sends_new_and_delete_sends_removed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(&dir.path().join("Videos/framecorder"), &dir.path().join("state"));
        ensure_dirs(&paths).unwrap();
        fake_mp4(&paths.videos.join("old.mp4"), 1);

        let library = Arc::new(Library::new(&paths));
        let hub = Arc::new(Hub::default());
        let devices = Arc::new(Devices::load(&paths.devices()));
        let watcher = Watcher::new(&paths, library.clone(), hub.clone(), devices).unwrap();
        assert_eq!(library.list().len(), 1, "existing files count as finished");
        let rx = hub.subscribe("test");
        std::thread::spawn(move || watcher.run());

        // the writer's .part shouldn't make a sound
        let part = paths.clips.join("x.mp4.part");
        fake_mp4(&part, 5);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());

        std::fs::rename(&part, paths.clips.join("x.mp4")).unwrap();
        let msg = rx.recv_timeout(Duration::from_secs(1)).expect("no event within a second");
        assert!(msg.starts_with("event: new\ndata: {"), "{msg}");
        assert!(msg.contains(r#""id":"c-x""#) && msg.contains(r#""kind":"clip""#) && msg.contains(r#""duration_s":5.0"#));

        std::fs::remove_file(paths.clips.join("x.mp4")).unwrap();
        let msg = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(&*msg, "event: removed\ndata: {\"id\":\"c-x\"}\n\n");
    }

    #[test]
    fn picks_up_revoked_devices() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(&dir.path().join("v"), &dir.path().join("state"));
        ensure_dirs(&paths).unwrap();
        std::fs::write(
            paths.devices(),
            r#"[{"id":"a","name":"a","token_sha256":"2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae","paired_at":0,"last_seen":0}]"#,
        )
        .unwrap();
        let devices = Arc::new(Devices::load(&paths.devices()));
        assert!(devices.check("foo").is_some());
        let library = Arc::new(Library::new(&paths));
        let watcher = Watcher::new(&paths, library, Arc::new(Hub::default()), devices.clone()).unwrap();
        std::thread::spawn(move || watcher.run());

        crate::config::write_atomic(&paths.devices(), b"[]").unwrap();
        for _ in 0..50 {
            if devices.check("foo").is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("devices.json change wasn't picked up");
    }
}
