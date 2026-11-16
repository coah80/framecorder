use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::devices::Devices;

const CHECK_FOR: Duration = Duration::from_secs(10 * 60);
const SERVICE: &str = "framecorder-update.service";
const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/framecorder_2dupdate_2eservice";

struct Checked {
    at: Instant,
    checksum: Option<Vec<u8>>,
    status: Value,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    #[serde(skip)]
    checked: Option<Checked>,
    requested: bool,
    failed: Option<String>,
}

impl State {
    fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }
}

pub struct Updates {
    path: PathBuf,
    state: Mutex<State>,
    home: PathBuf,
    systemctl: PathBuf,
    devices: Arc<Devices>,
}

impl Updates {
    pub fn new(paths: &crate::config::Paths, devices: Arc<Devices>) -> Self {
        let path = paths.state.join("update.json");
        Self {
            state: Mutex::new(State::load(&path)),
            path,
            home: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            systemctl: "systemctl".into(),
            devices,
        }
    }

    pub fn status(&self) -> Value {
        let mut state = self.state.lock().unwrap();
        let updating = self.service_status(&mut state);
        let checksum = std::fs::read(self.home.join(".local/share/framecorder/installed.sha256")).ok();
        let fresh = state.checked.as_ref().filter(|c| c.at.elapsed() < CHECK_FOR && c.checksum == checksum);
        let mut status = match fresh {
            Some(c) if !updating => c.status.clone(),
            _ => {
                state.checked = None;
                match self.check() {
                    Ok(status) => {
                        if !updating {
                            state.checked = Some(Checked { at: Instant::now(), checksum, status: status.clone() });
                        }
                        status
                    }
                    Err(error) => json!({
                        "installed": self.installed_version(), "latest": null, "available": false, "error": error,
                    }),
                }
            }
        };
        status["updating"] = updating.into();
        status
    }

    pub fn start(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        state.checked = None;
        state.requested = true;
        state.failed = None;
        let result = self.save(&state).and_then(|()| Command::new(&self.systemctl)
            .args(["--user", "--no-block", "start", SERVICE])
            .output()
            .map_err(|e| format!("couldn't run systemctl: {e}"))
            .and_then(|out| {
                if out.status.success() {
                    Ok(())
                } else {
                    Err(format!("couldn't start the update: {}", String::from_utf8_lossy(&out.stderr).trim()))
                }
            }));
        match &result {
            Ok(()) => {
                self.devices.update_status("updating", None);
                log::info!("starting a framecorder update, an app asked");
            }
            Err(error) => {
                state.requested = false;
                state.failed = Some(error.clone());
                if let Err(e) = self.save(&state) {
                    log::error!("{e}");
                }
                self.devices.update_status("failed", Some(error));
            }
        }
        result
    }

    fn save(&self, state: &State) -> Result<(), String> {
        let data = serde_json::to_vec(state).map_err(|e| format!("couldn't save the update state: {e}"))?;
        crate::config::write_atomic(&self.path, &data).map_err(|e| format!("couldn't save the update state: {e}"))
    }

    fn check(&self) -> Result<Value, String> {
        let out = Command::new(self.home.join(".local/bin/framecorder-setup"))
            .arg("--check")
            .output()
            .map_err(|e| format!("couldn't check for an update: {e}"))?;
        if !out.status.success() {
            return Err(format!("couldn't check for an update: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let status: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("odd answer from framecorder-setup: {e}"))?;
        if !status.is_object() || !status["installed"].is_string() || !status["available"].is_boolean() {
            return Err("odd answer from framecorder-setup".into());
        }
        Ok(status)
    }

    fn installed_version(&self) -> String {
        Command::new(self.home.join(".local/bin/framecorder"))
            .arg("--version")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|text| text.split_whitespace().nth(1).map(str::to_owned))
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into())
    }

    fn service_status(&self, state: &mut State) -> bool {
        let out = Command::new(&self.systemctl)
            .args(["--user", "show", SERVICE, "--property=ActiveState", "--property=Result", "--property=Job"])
            .output();
        let Some(out) = out.ok().filter(|out| out.status.success()) else {
            log::warn!("couldn't read the update service's state");
            return state.requested;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let property = |key| text.lines().find_map(|line| line.strip_prefix(key));
        let job = property("Job=").is_some_and(|job| job.split_whitespace().next().is_some_and(|id| id != "0"));
        let updating = job || matches!(property("ActiveState="), Some("active" | "activating" | "reloading"));
        if updating {
            state.checked = None;
        } else if state.requested {
            state.requested = false;
            if property("Result=").is_some_and(|result| result != "success") || property("ActiveState=") == Some("failed") {
                state.failed = Some("the update failed. try again from framecorder's setup on the headset".into());
            } else {
                state.failed = None;
            }
            if let Err(e) = self.save(state) {
                log::error!("{e}");
            }
        }
        self.devices.update_status(
            if updating {
                "updating"
            } else if state.failed.is_some() {
                "failed"
            } else {
                "idle"
            },
            if updating { None } else { state.failed.as_deref() },
        );
        updating
    }

    fn refresh_service(&self) {
        self.service_status(&mut self.state.lock().unwrap());
    }

    pub fn watch(self: Arc<Self>) -> Result<(), dbus::Error> {
        self.listen(dbus::blocking::Connection::new_session()?)
    }

    fn listen(self: Arc<Self>, connection: dbus::blocking::Connection) -> Result<(), dbus::Error> {
        use dbus::message::MatchRule;
        let rule = MatchRule::new_signal("org.freedesktop.DBus.Properties", "PropertiesChanged")
            .with_sender("org.freedesktop.systemd1")
            .with_path(UNIT_PATH);
        let updates = self.clone();
        connection.add_match(rule, move |_: (String, dbus::arg::PropMap, Vec<String>), _, _| {
            updates.refresh_service();
            true
        })?;
        let updates = self.clone();
        let rule = MatchRule::new_signal("org.freedesktop.systemd1.Manager", "JobRemoved").with_sender("org.freedesktop.systemd1");
        connection.add_match(rule, move |(_, _, unit, _): (u32, dbus::Path<'static>, String, String), _, _| {
            if unit == SERVICE {
                updates.refresh_service();
            }
            true
        })?;
        let proxy = connection.with_proxy("org.freedesktop.systemd1", "/org/freedesktop/systemd1", Duration::from_secs(3));
        let _: () = proxy.method_call("org.freedesktop.systemd1.Manager", "Subscribe", ())?;
        self.refresh_service();
        loop {
            connection.process(Duration::from_millis(i32::MAX as u64))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn script(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn setup() -> (tempfile::TempDir, Updates) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".local/share/framecorder")).unwrap();
        std::fs::write(home.join("service"), "ActiveState=inactive\nResult=success\nJob=0\n").unwrap();
        std::fs::write(home.join(".local/share/framecorder/installed.sha256"), "old").unwrap();
        std::fs::write(bin.join("check.json"), r#"{"installed":"0.2.0","latest":"0.3.0","available":true}"#).unwrap();
        script(
            &bin.join("framecorder-setup"),
            "#!/bin/sh\ndir=$(dirname \"$0\")\nif [ -f \"$dir/fail\" ]; then echo offline >&2; exit 1; fi\ncat \"$dir/check.json\"\n",
        );
        script(&bin.join("framecorder"), "#!/bin/sh\necho 'framecorder 0.2.0'\n");
        let systemctl = home.join("systemctl");
        script(&systemctl, "#!/bin/sh\ndir=$(dirname \"$0\")\ncase \"$2\" in\nshow) cat \"$dir/service\";;\n--no-block)\nif [ -p \"$dir/ready\" ]; then echo ready > \"$dir/ready\"; read -r answer < \"$dir/release\"; fi\nif [ -f \"$dir/launch-fail\" ]; then echo denied >&2; exit 1; fi\nprintf 'ActiveState=activating\\nResult=success\\nJob=1\\n' > \"$dir/service\";;\nesac\n");
        let updates = Updates {
            path: home.join("update.json"),
            state: Mutex::new(State::default()),
            home: home.into(),
            systemctl,
            devices: Arc::new(Devices::load(&home.join("devices.json"))),
        };
        (dir, updates)
    }

    fn clients(dir: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(dir.join("clients.json")).unwrap()).unwrap()
    }

    #[test]
    fn systemd_signals_publish_timer_updates_without_an_app_request() {
        use dbus::blocking::Connection;
        use dbus::channel::{MatchingReceiver, Sender};
        use dbus::message::MatchRule;
        use std::io::BufRead;
        use std::os::fd::AsRawFd;
        use std::process::{Child, Stdio};

        struct Bus(Child);
        impl Drop for Bus {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut bus = Bus(Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap());
        let mut address = String::new();
        std::io::BufReader::new(bus.0.stdout.take().unwrap()).read_line(&mut address).unwrap();
        let connect = || {
            let mut channel = dbus::channel::Channel::open_private(address.trim()).unwrap();
            channel.register().unwrap();
            Connection::from(channel)
        };
        let manager = connect();
        manager.request_name("org.freedesktop.systemd1", false, true, false).unwrap();
        let (ready, rx) = std::sync::mpsc::channel();
        manager.start_receive(
            MatchRule::new_method_call().with_interface("org.freedesktop.systemd1.Manager"),
            Box::new(move |msg, conn| {
                assert_eq!(&*msg.member().unwrap(), "Subscribe");
                conn.send(msg.method_return()).unwrap();
                ready.send(()).unwrap();
                true
            }),
        );
        let (dir, updates) = setup();
        let mut notify = inotify::Inotify::init().unwrap();
        notify.watches().add(dir.path(), inotify::WatchMask::MOVED_TO).unwrap();
        let updates = Arc::new(updates);
        let connection = connect();
        let worker = std::thread::spawn(move || updates.listen(connection));
        while rx.try_recv().is_err() {
            assert!(manager.process(Duration::from_secs(1)).unwrap(), "no systemd subscription");
        }
        let mut wait_for = |state: &str| {
            while clients(dir.path())["update"]["state"] != state {
                let mut fd = libc::pollfd { fd: notify.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                assert_eq!(unsafe { libc::poll(&mut fd, 1, 1000) }, 1, "no update state change");
                let mut buf = [0; 4096];
                notify.read_events(&mut buf).unwrap().for_each(drop);
            }
        };
        std::fs::write(dir.path().join("service"), "ActiveState=activating\nResult=success\nJob=0\n").unwrap();
        let signal = dbus::Message::new_signal(UNIT_PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged").unwrap().append3(
            "org.freedesktop.systemd1.Unit",
            dbus::arg::PropMap::new(),
            Vec::<String>::new(),
        );
        manager.send(signal).unwrap();
        manager.channel().flush();
        wait_for("updating");
        std::fs::write(dir.path().join("service"), "ActiveState=inactive\nResult=success\nJob=0\n").unwrap();
        let signal = dbus::Message::new_signal("/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "JobRemoved")
            .unwrap()
            .append3(1u32, dbus::Path::new("/org/freedesktop/systemd1/job/1").unwrap(), SERVICE)
            .append1("done");
        manager.send(signal).unwrap();
        manager.channel().flush();
        wait_for("idle");
        bus.0.kill().unwrap();
        assert!(worker.join().unwrap().is_err());
    }

    #[test]
    fn offline_checks_keep_the_installed_version_and_actual_service_state() {
        let (dir, updates) = setup();
        std::fs::write(dir.path().join(".local/bin/fail"), "").unwrap();
        for active in [true, false] {
            std::fs::write(
                dir.path().join("service"),
                if active { "ActiveState=activating\nResult=success\nJob=0\n" } else { "ActiveState=inactive\nResult=success\nJob=0\n" },
            )
            .unwrap();
            let status = updates.status();
            assert_eq!(status["installed"], "0.2.0");
            assert_eq!(status["updating"], active);
            assert_eq!(status["available"], false);
            assert!(status["latest"].is_null());
            assert!(status["error"].as_str().unwrap().contains("offline"));
            assert_eq!(clients(dir.path())["update"]["state"], if active { "updating" } else { "idle" });
        }
    }

    #[test]
    fn a_changed_installed_checksum_invalidates_a_cached_check() {
        let (dir, updates) = setup();
        assert_eq!(updates.status()["installed"], "0.2.0");
        std::fs::write(dir.path().join(".local/bin/check.json"), r#"{"installed":"0.3.0","latest":"0.3.0","available":false}"#).unwrap();
        assert_eq!(updates.status()["installed"], "0.2.0");
        std::fs::write(dir.path().join(".local/share/framecorder/installed.sha256"), "new").unwrap();
        let status = updates.status();
        assert_eq!(status["installed"], "0.3.0");
        assert_eq!(status["available"], false);
    }

    #[test]
    fn launch_holds_cache_lock_until_the_service_has_started() {
        use std::ffi::CString;
        use std::io::Write;
        let (dir, updates) = setup();
        assert_eq!(updates.status()["available"], true);
        for name in ["ready", "release"] {
            let path = CString::new(dir.path().join(name).as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        }
        std::thread::scope(|scope| {
            let start = scope.spawn(|| updates.start());
            assert_eq!(std::fs::read_to_string(dir.path().join("ready")).unwrap().trim(), "ready");
            assert!(updates.state.try_lock().is_err(), "status must wait until launch finishes");
            let mut release = std::fs::OpenOptions::new().write(true).open(dir.path().join("release")).unwrap();
            writeln!(release, "go").unwrap();
            start.join().unwrap().unwrap();
        });
        assert!(updates.state.lock().unwrap().checked.is_none());
        assert_eq!(clients(dir.path())["update"]["state"], "updating");
    }

    #[test]
    fn app_update_failure_survives_a_sync_restart() {
        let (dir, updates) = setup();
        updates.start().unwrap();
        let restarted = Updates {
            path: updates.path.clone(), state: Mutex::new(State::load(&updates.path)),
            home: updates.home.clone(), systemctl: updates.systemctl.clone(), devices: updates.devices.clone(),
        };
        std::fs::write(dir.path().join("service"), "ActiveState=failed\nResult=exit-code\nJob=0\n").unwrap();
        restarted.refresh_service();
        assert_eq!(clients(dir.path())["update"]["state"], "failed");
        assert!(State::load(&updates.path).failed.is_some());
        assert!(!State::load(&updates.path).requested);
    }

    #[test]
    fn app_failures_stay_visible_and_timer_updates_return_to_idle() {
        let (dir, updates) = setup();
        updates.start().unwrap();
        std::fs::write(dir.path().join("service"), "ActiveState=failed\nResult=exit-code\nJob=0\n").unwrap();
        updates.refresh_service();
        assert_eq!(clients(dir.path())["update"]["state"], "failed");
        assert!(clients(dir.path())["update"]["error"].as_str().unwrap().contains("update failed"));
        updates.start().unwrap();
        std::fs::write(dir.path().join("service"), "ActiveState=inactive\nResult=success\nJob=0\n").unwrap();
        updates.refresh_service();
        assert_eq!(clients(dir.path())["update"], json!({"state": "idle", "error": null}));
        std::fs::write(dir.path().join("service"), "ActiveState=activating\nResult=success\nJob=0\n").unwrap();
        updates.refresh_service();
        assert_eq!(clients(dir.path())["update"]["state"], "updating");
        std::fs::write(dir.path().join("service"), "ActiveState=failed\nResult=exit-code\nJob=0\n").unwrap();
        updates.refresh_service();
        assert_eq!(clients(dir.path())["update"]["state"], "idle");
        std::fs::write(dir.path().join("launch-fail"), "").unwrap();
        assert!(updates.start().unwrap_err().contains("denied"));
        assert_eq!(clients(dir.path())["update"]["state"], "failed");
    }
}
