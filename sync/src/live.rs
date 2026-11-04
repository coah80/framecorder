use std::collections::VecDeque;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::config::{write_atomic, Paths};

const LIMIT: usize = 1024 * 1024;

#[derive(Default)]
struct Queue {
    packets: VecDeque<Arc<[u8]>>,
    bytes: usize,
    waiting: bool,
    closed: bool,
}

#[derive(Default)]
struct Inner {
    viewers: Vec<Arc<(Mutex<Queue>, Condvar)>>,
    socket: Option<Arc<UnixDatagram>>,
    pumping: bool,
}

#[derive(Default)]
pub struct Live(Mutex<Inner>);

pub struct Viewer<'a> {
    live: &'a Live,
    paths: &'a Paths,
    queue: Arc<(Mutex<Queue>, Condvar)>,
}

impl Live {
    fn publish(inner: &Inner, paths: &Paths) -> io::Result<()> {
        let body = serde_json::json!({"pid": std::process::id(), "viewers": inner.viewers.len()});
        write_atomic(&paths.state.join("live.json"), body.to_string().as_bytes())
    }

    pub fn join<'a>(&'a self, paths: &'a Paths) -> io::Result<Viewer<'a>> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if inner.socket.is_none() {
            let path = paths.state.join("live.sock");
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            let socket = Arc::new(UnixDatagram::bind(path)?);
            socket.set_read_timeout(Some(Duration::from_millis(250)))?;
            inner.socket = Some(socket);
        }
        let queue = Arc::new((Mutex::new(Queue { waiting: true, ..Queue::default() }), Condvar::new()));
        inner.viewers.push(queue.clone());
        if let Err(e) = Self::publish(&inner, paths) {
            inner.viewers.pop();
            return Err(e);
        }
        Ok(Viewer { live: self, paths, queue })
    }

    pub fn pump(&self) {
        {
            let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if inner.pumping {
                return;
            }
            inner.pumping = true;
        }
        let mut buf = vec![0; 65536];
        let mut expected = None;
        loop {
            let socket = {
                let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
                if inner.viewers.is_empty() {
                    inner.socket = None;
                    inner.pumping = false;
                    break;
                }
                inner.socket.clone()
            };
            let Some(socket) = socket else { break };
            match socket.recv(&mut buf) {
                Ok(n) if n > 9 => {
                    let mut seq = [0; 8];
                    seq.copy_from_slice(&buf[..8]);
                    let seq = u64::from_le_bytes(seq);
                    let gap = expected.is_some_and(|e| e != seq);
                    expected = Some(seq.wrapping_add(1));
                    self.broadcast(Arc::from(&buf[9..n]), buf[8] == 1, gap);
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(_) => {
                    let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
                    for viewer in &inner.viewers {
                        viewer.0.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
                        viewer.1.notify_all();
                    }
                    inner.pumping = false;
                    break;
                }
                _ => {}
            }
        }
    }

    fn broadcast(&self, data: Arc<[u8]>, key: bool, gap: bool) {
        let inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for viewer in &inner.viewers {
            let mut q = viewer.0.lock().unwrap_or_else(|e| e.into_inner());
            if gap || q.bytes + data.len() > LIMIT {
                q.packets.clear();
                q.bytes = 0;
                q.waiting = true;
            }
            if q.waiting && !key {
                continue;
            }
            q.waiting = false;
            q.bytes += data.len();
            q.packets.push_back(data.clone());
            viewer.1.notify_one();
        }
    }
}

impl Viewer<'_> {
    pub fn next(&self, timeout: Duration) -> io::Result<Option<Arc<[u8]>>> {
        let q = self.queue.0.lock().unwrap_or_else(|e| e.into_inner());
        let (mut q, _) =
            self.queue.1.wait_timeout_while(q, timeout, |q| q.packets.is_empty() && !q.closed).unwrap_or_else(|e| e.into_inner());
        if q.closed {
            return Err(io::Error::other("the live stream stopped"));
        }
        let data = q.packets.pop_front();
        if let Some(data) = &data {
            q.bytes -= data.len();
        }
        Ok(data)
    }
}

impl Drop for Viewer<'_> {
    fn drop(&mut self) {
        let mut inner = self.live.0.lock().unwrap_or_else(|e| e.into_inner());
        inner.viewers.retain(|q| !Arc::ptr_eq(q, &self.queue));
        let _ = Live::publish(&inner, self.paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewers_share_packets_recover_at_keys_and_publish_departures() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path(), dir.path());
        let live = Live::default();
        let first = live.join(&paths).unwrap();
        let second = live.join(&paths).unwrap();
        live.broadcast(Arc::from([1]), false, false);
        assert!(first.next(Duration::ZERO).unwrap().is_none());
        live.broadcast(Arc::from([2]), true, false);
        let a = first.next(Duration::ZERO).unwrap().unwrap();
        let b = second.next(Duration::ZERO).unwrap().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        for _ in 0..20 {
            live.broadcast(vec![3; 65536].into(), false, false);
            assert_eq!(second.next(Duration::ZERO).unwrap().unwrap().len(), 65536);
        }
        assert!(first.next(Duration::ZERO).unwrap().is_none());
        live.broadcast(Arc::from([4]), true, false);
        assert_eq!(&*first.next(Duration::ZERO).unwrap().unwrap(), &[4]);
        live.broadcast(Arc::from([5]), false, true);
        assert!(first.next(Duration::ZERO).unwrap().is_none());
        drop(second);
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.state.join("live.json")).unwrap()).unwrap();
        assert_eq!(value["viewers"], 1);
        drop(first);
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.state.join("live.json")).unwrap()).unwrap();
        assert_eq!(value["viewers"], 0);
        live.pump();
        assert!(live.0.lock().unwrap().socket.is_none());
    }
}
