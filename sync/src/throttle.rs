//! Keeps transfers from getting in the way of a game. While a VR game is
//! running every transfer shares one slow lane, otherwise they go flat out.
//! Whether a game is running only gets checked while something's actually
//! being sent, so an idle daemon never wakes up for it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const CHECK_EVERY: Duration = Duration::from_secs(5);
/// How far ahead of schedule a sender may get, so pacing isn't choppy.
const BURST: Duration = Duration::from_millis(250);

pub struct Throttle {
    /// Bytes per second while a game runs, 0 for no limit at all.
    game_rate: f64,
    limited: AtomicBool,
    active: AtomicUsize,
    next: Mutex<Instant>,
    wake: Condvar,
    /// When the checker last looked, if its answer is still good.
    checked: Mutex<Option<Instant>>,
}

pub struct Transfer<'a>(&'a Throttle);

impl Drop for Transfer<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Throttle {
    pub fn new(game_rate_mb: f64) -> Arc<Self> {
        Arc::new(Self {
            game_rate: (game_rate_mb.max(0.0) * 1_000_000.0).round(),
            limited: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            next: Mutex::new(Instant::now()),
            wake: Condvar::new(),
            checked: Mutex::new(None),
        })
    }

    pub fn start(&self) -> Transfer<'_> {
        if self.active.fetch_add(1, Ordering::SeqCst) == 0 && self.game_rate > 0.0 {
            let mut checked = self.checked.lock().unwrap();
            if checked.is_none_or(|t| t.elapsed() > CHECK_EVERY) {
                // assume the worst until the checker has had a fresh look
                self.limited.store(true, Ordering::SeqCst);
                *checked = None;
            }
            self.wake.notify_all();
        }
        Transfer(self)
    }

    pub fn limited(&self) -> bool {
        self.limited.load(Ordering::Relaxed)
    }

    /// Call before sending `bytes`. Sleeps as long as needed to keep all
    /// transfers together under the game rate.
    pub fn pace(&self, bytes: usize) {
        if !self.limited() || self.game_rate <= 0.0 {
            return;
        }
        let wait = {
            let mut next = self.next.lock().unwrap();
            let now = Instant::now();
            if *next + BURST < now {
                *next = now;
            }
            let wait = next.saturating_duration_since(now + BURST);
            *next += Duration::from_secs_f64(bytes as f64 / self.game_rate);
            wait
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    /// Runs forever. Sleeps on a condvar while nothing is being sent.
    pub fn run_checker(&self, mut game_running: impl FnMut() -> bool) {
        if self.game_rate <= 0.0 {
            return;
        }
        let mut was = None;
        loop {
            {
                let mut checked = self.checked.lock().unwrap();
                while self.active.load(Ordering::SeqCst) == 0 {
                    checked = self.wake.wait(checked).unwrap();
                }
            }
            let game = game_running();
            if was != Some(game) {
                log::info!("{}", if game { "game running, slowing transfers down" } else { "no game, full speed" });
                was = Some(game);
            }
            let mut checked = self.checked.lock().unwrap();
            self.limited.store(game, Ordering::SeqCst);
            *checked = Some(Instant::now());
            // until it's time to look again, or a transfer starts after a quiet spell
            let _ = self.wake.wait_timeout(checked, CHECK_EVERY).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paces_to_the_game_rate_only_when_limited() {
        let t = Throttle::new(1.0);
        // not limited: no waiting
        let start = Instant::now();
        t.pace(10_000_000);
        assert!(start.elapsed() < Duration::from_millis(50));

        let _transfer = t.start();
        assert!(t.limited());
        let start = Instant::now();
        // 1 MB/s: the burst allowance covers the first quarter second
        // and the tenth send is scheduled for 0.9 s in
        for _ in 0..10 {
            t.pace(100_000);
        }
        let took = start.elapsed();
        assert!(took >= Duration::from_millis(600) && took < Duration::from_millis(900), "{took:?}");
    }

    #[test]
    fn zero_rate_never_limits() {
        let t = Throttle::new(0.0);
        let _transfer = t.start();
        assert!(!t.limited());
    }

    #[test]
    fn checker_only_runs_during_transfers() {
        let t = Throttle::new(8.0);
        let checks = Arc::new(AtomicUsize::new(0));
        let (t2, c2) = (t.clone(), checks.clone());
        std::thread::spawn(move || {
            t2.run_checker(|| {
                c2.fetch_add(1, Ordering::SeqCst);
                false
            })
        });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(checks.load(Ordering::SeqCst), 0);
        let transfer = t.start();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(checks.load(Ordering::SeqCst), 1);
        assert!(!t.limited(), "checker said no game");
        drop(transfer);
    }
}
