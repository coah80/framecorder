//! Fans library changes out to every open /events stream.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use crate::library::{Change, Clip};

/// How many events a slow listener may fall behind before we drop it. It
/// reconnects and diffs /clips, so nothing's lost.
const BACKLOG: usize = 64;

#[derive(Default)]
pub struct Hub {
    subs: Mutex<Vec<(String, SyncSender<Arc<str>>)>>,
}

impl Hub {
    /// One stream per device. A new one replaces the old, which then ends,
    /// so a phone that roamed off and back doesn't leave a dead one behind.
    pub fn subscribe(&self, device: &str) -> Receiver<Arc<str>> {
        let (tx, rx) = sync_channel(BACKLOG);
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|(d, _)| d != device);
        subs.push((device.to_string(), tx));
        rx
    }

    #[cfg(test)]
    pub fn listeners(&self) -> usize {
        self.subs.lock().unwrap().len()
    }

    pub fn send(&self, change: &Change) {
        let msg: Arc<str> = sse_message(change).into();
        self.subs.lock().unwrap().retain(|(_, tx)| match tx.try_send(msg.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        });
    }
}

pub fn sse_message(change: &Change) -> String {
    match change {
        Change::New(clip) => format!("event: new\ndata: {}\n\n", clip_json(clip)),
        Change::Removed(id) => format!("event: removed\ndata: {}\n\n", serde_json::json!({ "id": id })),
    }
}

pub fn clip_json(clip: &Clip) -> String {
    serde_json::to_string(clip).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn one_stream_per_device() {
        let hub = Hub::default();
        let old = hub.subscribe("a");
        let other = hub.subscribe("b");
        let new = hub.subscribe("a");
        assert!(matches!(old.recv_timeout(Duration::from_millis(10)), Err(std::sync::mpsc::RecvTimeoutError::Disconnected)));
        hub.send(&Change::Removed("r-x".into()));
        assert!(new.try_recv().is_ok());
        assert!(other.try_recv().is_ok());
        assert_eq!(hub.listeners(), 2);
    }

    #[test]
    fn slow_listeners_get_dropped() {
        let hub = Hub::default();
        let _rx = hub.subscribe("a");
        for _ in 0..=BACKLOG {
            hub.send(&Change::Removed("r-x".into()));
        }
        assert_eq!(hub.listeners(), 0);
    }
}
