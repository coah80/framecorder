//! what a frame's tab is up to, as the app keeps it: what the frame pushes,
//! the words for it, and asking it to record, stop or save a clip.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use framecorder_app_lib::core::api::{About, Recording, Remote};
use framecorder_app_lib::core::engine::{State, Status};
use gpui::{Context, SystemNotification};

use crate::app::{FrameApp, Page};
use crate::format;

/// what a frame has told us since it connected
#[derive(Default)]
pub struct Live {
    pub remote: Option<Remote>,
    /// when `remote` arrived, so a running recording's clock carries on from its elapsed_ms
    pub remote_at: Option<Instant>,
    pub about: Option<About>,
    /// its recording settings, once it's said. kept while it's away, greyed out
    pub recording: Option<Recording>,
    /// it's answered about its settings: None in `recording` then means its
    /// framecorder is too old to have them
    pub asked: bool,
    pub asking: bool,
    /// a command on its way: "record", "stop" or "clip"
    pub sending: Option<&'static str>,
}

impl Live {
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        match (&self.remote, self.remote_at) {
            (Some(r), Some(at)) if r.running => r.elapsed_ms + now.duration_since(at).as_millis() as u64,
            (Some(r), _) => r.elapsed_ms,
            _ => 0,
        }
    }

    pub fn recording(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.recording)
    }

    /// the tab's there and set up, so it takes commands
    pub fn usable(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.available && r.ready)
    }
}

/// the headline on the frame page, and the line under it
pub fn words(s: &Status, remote: Option<&Remote>) -> (&'static str, String) {
    let said = |t: &str| t.to_string();
    match s.state {
        State::Connecting => ("looking for it", said("it takes commands once it's on, on this wi-fi, with framecorder running.")),
        State::Unreachable => ("can't reach it", said("nothing is lost, clips wait on the frame. check it's on and on this wi-fi.")),
        State::Full => ("out of space here", said("the frame can still record. free up some space and its clips sync on their own.")),
        State::Unpaired => ("forgot this device", said("it was removed on the frame. pair again to keep syncing.")),
        State::WrongFingerprint => {
            ("something else answered", said("something else answered where your frame was, so we're not talking to it."))
        }
        State::Connected => match remote {
            None => ("connected", said("record, stop and save clips from here once your frame has the next framecorder update.")),
            Some(r) if !r.available => ("framecorder isn't open", said("framecorder isn't open on the headset. it starts with SteamVR.")),
            Some(r) if !r.ready => ("not set up yet", said("finish setting up framecorder on the headset first.")),
            Some(r) if r.recording && !r.running => ("recording, paused", said("recording, paused while the framecorder tab is open.")),
            Some(r) if r.recording => ("recording", said("recording. stop it here or on the headset.")),
            Some(r) if r.clip_ready => {
                ("ready", format!("keeping the last {}, ready to save.", format::span(u64::from(r.clips.unwrap_or(30)) * 1000)))
            }
            Some(r) if r.clips.is_none() => {
                ("ready", said("clips are off, so there's nothing to save. record instead, or turn them on below."))
            }
            Some(_) => ("ready", said("clips are starting up...")),
        },
    }
}

/// the frame the tray acts on: the first connected one whose tab takes commands
pub fn target<'a>(statuses: &'a [Status], live: &'a HashMap<String, Live>) -> Option<(&'a Status, &'a Remote)> {
    statuses.iter().find_map(|s| {
        let l = live.get(&s.fingerprint).filter(|l| s.state == State::Connected && l.usable())?;
        Some((s, l.remote.as_ref()?))
    })
}

impl FrameApp {
    pub fn live(&self, fingerprint: &str) -> Option<&Live> {
        self.live.get(fingerprint)
    }

    fn live_mut(&mut self, fingerprint: &str) -> &mut Live {
        self.live.entry(fingerprint.to_string()).or_default()
    }

    /// the frame the frame page is about: the one picked, else the first
    pub fn picked(&self) -> Option<&Status> {
        self.frame.as_ref().and_then(|fp| self.statuses.iter().find(|s| &s.fingerprint == fp)).or(self.statuses.first())
    }

    pub fn pick_frame(&mut self, fingerprint: String, cx: &mut Context<Self>) {
        self.frame = Some(fingerprint.clone());
        self.ask_settings(&fingerprint, cx);
        self.go(Page::Frame, cx);
    }

    pub fn on_remote(&mut self, fingerprint: &str, remote: Option<Remote>, cx: &mut Context<Self>) {
        let l = self.live_mut(fingerprint);
        l.remote_at = remote.as_ref().map(|_| Instant::now());
        l.remote = remote;
        self.sync_tray();
        cx.notify();
    }

    pub fn on_about(&mut self, fingerprint: &str, about: Option<About>, cx: &mut Context<Self>) {
        self.live_mut(fingerprint).about = about;
        cx.notify();
    }

    /// a frame that just connected gets asked for its settings, one that left
    /// gets asked again when it's back
    pub fn follow_connections(&mut self, cx: &mut Context<Self>) {
        let statuses = self.statuses.clone();
        for s in statuses {
            if s.state == State::Connected {
                self.ask_settings(&s.fingerprint, cx);
            } else {
                self.live_mut(&s.fingerprint).asked = false;
            }
        }
    }

    pub fn sync_tray(&self) {
        if let Some(t) = &self.tray {
            t.refresh(&self.statuses, target(&self.statuses, &self.live));
        }
    }

    /// any frame recording right now, for the dot in the sidebar
    pub fn any_recording(&self) -> bool {
        self.statuses.iter().any(|s| s.state == State::Connected && self.live.get(&s.fingerprint).is_some_and(Live::recording))
    }

    /// repaints once a second, for a recording's clock. the frame page calls
    /// this while it's showing one, so nothing ticks otherwise
    pub fn arm_tick(&mut self, cx: &mut Context<Self>) {
        if self.tick.is_some() {
            return;
        }
        self.tick = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |app, cx| {
                app.tick = None;
                cx.notify();
            });
        }));
    }

    // settings

    fn ask_settings(&mut self, fingerprint: &str, cx: &mut Context<Self>) {
        if self.demo || !self.statuses.iter().any(|s| s.fingerprint == fingerprint && s.state == State::Connected) {
            return;
        }
        let l = self.live_mut(fingerprint);
        if l.asked || l.asking {
            return;
        }
        l.asking = true;
        let (engine, fp) = (self.core.engine.clone(), fingerprint.to_string());
        let job = self.core.rt.spawn(async move { engine.recording(&fp).await });
        let fp = fingerprint.to_string();
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                let l = app.live_mut(&fp);
                l.asking = false;
                match res {
                    Ok(settings) => {
                        l.asked = true;
                        l.recording = settings;
                    }
                    // asked again when it reconnects
                    Err(e) => log::info!("couldn't read the frame's recording settings: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// changes a frame's recording settings, showing the change right away
    /// and taking it back if the frame says no
    pub fn change_settings(&mut self, fingerprint: &str, settings: Recording, cx: &mut Context<Self>) {
        let old = self.live_mut(fingerprint).recording.replace(settings.clone());
        cx.notify();
        if self.demo {
            return;
        }
        let (engine, fp) = (self.core.engine.clone(), fingerprint.to_string());
        let job = self.core.rt.spawn(async move { engine.set_recording(&fp, &settings).await });
        let fp = fingerprint.to_string();
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(now) => app.live_mut(&fp).recording = Some(now),
                    Err(e) => {
                        app.live_mut(&fp).recording = old;
                        app.toast(format!("couldn't change that: {e}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // commands

    /// asks a frame to `record`, `stop` or `clip`. from the tray the window
    /// may be closed, so how it went is said in a notification
    pub fn command(&mut self, fingerprint: &str, what: &'static str, from_tray: bool, cx: &mut Context<Self>) {
        let l = self.live_mut(fingerprint);
        if l.sending.is_some() {
            return;
        }
        l.sending = Some(what);
        cx.notify();
        if self.demo {
            self.demo_command(fingerprint, what, cx);
            return;
        }
        let (engine, fp) = (self.core.engine.clone(), fingerprint.to_string());
        let job = self.core.rt.spawn(async move { engine.command(&fp, what).await });
        let fp = fingerprint.to_string();
        cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                app.live_mut(&fp).sending = None;
                let said = match res {
                    Ok(remote) => {
                        app.on_remote(&fp, Some(remote), cx);
                        (what == "clip").then_some("saved, it'll be here in a moment".to_string())
                    }
                    Err(e) => Some(e),
                };
                if let Some(said) = said {
                    if from_tray {
                        cx.show_system_notification(SystemNotification {
                            tag: "framecorder-remote".into(),
                            title: "framecorder".into(),
                            body: said.into(),
                            actions: Vec::new(),
                        });
                    } else {
                        app.toast(said, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// record or stop, whichever the tray's frame is up for
    pub fn tray_record(&mut self, cx: &mut Context<Self>) {
        if let Some((s, r)) = target(&self.statuses, &self.live) {
            let fp = s.fingerprint.clone();
            let what = if r.recording { "stop" } else { "record" };
            self.command(&fp, what, true, cx);
        }
    }

    pub fn tray_clip(&mut self, cx: &mut Context<Self>) {
        if let Some((s, _)) = target(&self.statuses, &self.live) {
            let fp = s.fingerprint.clone();
            self.command(&fp, "clip", true, cx);
        }
    }

    /// a demo frame does what it's told, right here
    fn demo_command(&mut self, fingerprint: &str, what: &str, cx: &mut Context<Self>) {
        let l = self.live_mut(fingerprint);
        l.sending = None;
        let Some(r) = l.remote.clone() else { return };
        let now = match what {
            "record" => Remote { recording: true, running: true, elapsed_ms: 0, clip_ready: false, ..r },
            "stop" => Remote { recording: false, running: false, elapsed_ms: 0, clip_ready: r.clips.is_some(), ..r },
            _ => {
                self.toast("saved, it'll be here in a moment", cx);
                r
            }
        };
        self.on_remote(fingerprint, Some(now), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: State) -> Status {
        Status { fingerprint: "a".into(), name: "f".into(), addr: "1:2".into(), state, message: None, update: None }
    }

    fn remote(available: bool, ready: bool, recording: bool, running: bool, clips: Option<u32>, clip_ready: bool) -> Remote {
        Remote { available, ready, recording, running, elapsed_ms: 0, clips, clip_ready }
    }

    #[test]
    fn says_what_the_frame_is_up_to() {
        let c = status(State::Connected);
        assert_eq!(words(&c, None).0, "connected");
        assert_eq!(words(&c, Some(&remote(false, false, false, false, None, false))).0, "framecorder isn't open");
        assert_eq!(words(&c, Some(&remote(true, false, false, false, None, false))).0, "not set up yet");
        assert_eq!(words(&c, Some(&remote(true, true, true, false, Some(30), false))).0, "recording, paused");
        assert_eq!(words(&c, Some(&remote(true, true, true, true, Some(30), false))).0, "recording");
        assert_eq!(
            words(&c, Some(&remote(true, true, false, false, Some(60), true))).1,
            "keeping the last 1:00, ready to save."
        );
        assert!(words(&c, Some(&remote(true, true, false, false, None, false))).1.starts_with("clips are off"));
        assert!(words(&status(State::Unpaired), None).1.contains("pair again"));
    }

    #[test]
    fn a_running_clock_carries_on_from_what_the_frame_said() {
        let at = Instant::now();
        let mut l = Live { remote: Some(remote(true, true, true, true, None, false)), remote_at: Some(at), ..Default::default() };
        l.remote.as_mut().unwrap().elapsed_ms = 5_000;
        assert_eq!(l.elapsed_ms(at + Duration::from_secs(3)), 8_000);
        l.remote.as_mut().unwrap().running = false;
        assert_eq!(l.elapsed_ms(at + Duration::from_secs(3)), 5_000);
    }

    #[test]
    fn the_tray_follows_a_frame_that_takes_commands() {
        let mut live = HashMap::new();
        live.insert("a".to_string(), Live { remote: Some(remote(false, false, false, false, None, false)), ..Default::default() });
        live.insert("b".to_string(), Live { remote: Some(remote(true, true, false, false, Some(30), true)), ..Default::default() });
        let mut b = status(State::Connected);
        b.fingerprint = "b".into();
        let statuses = [status(State::Connected), b];
        assert_eq!(target(&statuses, &live).map(|(s, _)| s.fingerprint.as_str()), Some("b"));
        live.remove("b");
        assert!(target(&statuses, &live).is_none());
    }
}
