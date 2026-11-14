//! The dashboard's lock: one long random password, checked without leaking
//! how close a guess was, five wrong tries per address every fifteen minutes,
//! and a session that's a random token only its hash is kept of.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

use anyhow::{bail, Context, Result};
use ring::digest::{digest, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

/// How long a login lasts.
pub const SESSION_SECS: u64 = 30 * 86400;
const TRIES: usize = 5;
const TRIES_WINDOW: u64 = 15 * 60;

pub enum Login {
    /// The token to hand back as the session cookie.
    In(String),
    Wrong,
    /// Too many wrong tries from this address for now.
    Locked,
}

pub struct Auth {
    /// The password's sha256 signed with `key`, so a guess is checked by
    /// verifying a signature, which takes the same time right or wrong.
    password: hmac::Tag,
    key: hmac::Key,
    sessions: HashMap<[u8; 32], u64>,
    wrong: HashMap<IpAddr, VecDeque<u64>>,
    rng: SystemRandom,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    digest(&SHA256, bytes).as_ref().try_into().expect("sha256 is 32 bytes")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Auth {
    /// `password_sha256` is the hex sha256 of the password; the password
    /// itself is never on this machine's disk next to the service.
    pub fn new(password_sha256: &str) -> Result<Self> {
        let text = password_sha256.trim();
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("STATS_PASSWORD_SHA256 must be 64 hex characters");
        }
        let mut password = [0; 32];
        for (i, byte) in password.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).context("reading STATS_PASSWORD_SHA256")?;
        }
        let rng = SystemRandom::new();
        let key = hmac::Key::generate(hmac::HMAC_SHA256, &rng).map_err(|_| anyhow::anyhow!("no random numbers"))?;
        Ok(Self { password: hmac::sign(&key, &password), key, sessions: HashMap::new(), wrong: HashMap::new(), rng })
    }

    pub fn login(&mut self, ip: IpAddr, password: &str, now: u64) -> Login {
        self.wrong.retain(|_, times| {
            times.retain(|t| now.saturating_sub(*t) < TRIES_WINDOW);
            !times.is_empty()
        });
        if self.wrong.get(&ip).is_some_and(|times| times.len() >= TRIES) {
            return Login::Locked;
        }
        if hmac::verify(&self.key, &sha256(password.as_bytes()), self.password.as_ref()).is_err() {
            self.wrong.entry(ip).or_default().push_back(now);
            return Login::Wrong;
        }
        let mut token = [0; 32];
        if self.rng.fill(&mut token).is_err() {
            return Login::Wrong;
        }
        self.sessions.retain(|_, until| *until > now);
        self.sessions.insert(sha256(&token), now + SESSION_SECS);
        Login::In(hex(&token))
    }

    pub fn valid(&self, token: &str, now: u64) -> bool {
        let Some(bytes) = unhex(token) else { return false };
        self.sessions.get(&sha256(&bytes)).is_some_and(|until| *until > now)
    }

    pub fn logout(&mut self, token: &str) {
        if let Some(bytes) = unhex(token) {
            self.sessions.remove(&sha256(&bytes));
        }
    }
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    (0..32).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Auth {
        Auth::new(&hex(&sha256(b"the right one"))).unwrap()
    }

    #[test]
    fn only_the_password_gets_in_and_guessing_locks_the_address_out() {
        let mut a = auth();
        let (me, them): (IpAddr, IpAddr) = ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        for _ in 0..5 {
            assert!(matches!(a.login(them, "a guess", 100), Login::Wrong));
        }
        // even the right password, once locked
        assert!(matches!(a.login(them, "the right one", 101), Login::Locked));
        let Login::In(token) = a.login(me, "the right one", 101) else { panic!("the password didn't get in") };
        assert!(a.valid(&token, 102));
        assert!(!a.valid(&token, 102 + SESSION_SECS));
        assert!(!a.valid(&"0".repeat(64), 102));
        assert!(!a.valid("", 102));
        // the lock lifts after the window
        assert!(matches!(a.login(them, "the right one", 100 + TRIES_WINDOW), Login::In(_)));
        a.logout(&token);
        assert!(!a.valid(&token, 103));
    }
}
