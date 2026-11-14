//! The dashboard's lock, after OWASP's authentication, session and password
//! storage cheat sheets:
//! - two things to get in, both checked on every try: a password kept only as
//!   an Argon2id hash, and a six digit code from an authenticator app (TOTP,
//!   RFC 6238) that can't be used twice
//! - the same answer whichever of them was wrong
//! - five wrong tries from an address lock it out for fifteen minutes
//! - a session is 256 random bits whose hash is all that's kept, ends after
//!   two idle hours or a day, belongs to the browser that made it, and is
//!   gone on lock or restart

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

use anyhow::{anyhow, bail, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use ring::digest::{digest, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

/// A session ends this long after it began, whatever happens.
pub const SESSION_SECS: u64 = 24 * 3600;
/// And this long after it was last used.
const IDLE_SECS: u64 = 2 * 3600;
const TRIES: usize = 5;
const TRIES_WINDOW: u64 = 15 * 60;
/// How long a code lasts, and how many digits it has.
const STEP: u64 = 30;
const DIGITS: u32 = 6;

pub enum Login {
    /// The token to hand back as the session cookie.
    In(String),
    Wrong,
    /// Too many wrong tries from this address for now.
    Locked,
}

struct Session {
    until: u64,
    seen: u64,
    /// The browser it was made in, hashed.
    agent: [u8; 32],
}

pub struct Auth {
    /// The password's Argon2id hash, as a PHC string.
    password: String,
    /// The authenticator's shared secret.
    totp: Vec<u8>,
    /// The last code step that got in: a code works once.
    used_step: u64,
    sessions: HashMap<[u8; 32], Session>,
    wrong: HashMap<IpAddr, VecDeque<u64>>,
    rng: SystemRandom,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    digest(&SHA256, bytes).as_ref().try_into().expect("sha256 is 32 bytes")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    (0..32).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

/// OWASP's first choice for Argon2id: 19 MiB, two passes, one lane.
fn argon() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(19_456, 2, 1, None).expect("fixed argon2 parameters"))
}

/// A password's hash, to keep in place of it.
pub fn hash_password(password: &str) -> Result<String> {
    let mut salt = [0; 16];
    SystemRandom::new().fill(&mut salt).map_err(|_| anyhow!("no random numbers"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| anyhow!("{e}"))?;
    Ok(argon().hash_password(password.as_bytes(), &salt).map_err(|e| anyhow!("{e}"))?.to_string())
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32(bytes: &[u8]) -> String {
    let (mut out, mut bits, mut have) = (String::new(), 0u32, 0);
    for &b in bytes {
        bits = bits << 8 | u32::from(b);
        have += 8;
        while have >= 5 {
            have -= 5;
            out.push(BASE32[(bits >> have & 31) as usize] as char);
        }
    }
    if have > 0 {
        out.push(BASE32[(bits << (5 - have) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> Option<Vec<u8>> {
    let (mut out, mut bits, mut have) = (Vec::new(), 0u32, 0);
    for c in text.trim().bytes().filter(|c| *c != b'=') {
        bits = bits << 5 | BASE32.iter().position(|b| *b == c.to_ascii_uppercase())? as u32;
        have += 5;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
        }
    }
    Some(out)
}

/// The code an authenticator shows for `step` (RFC 4226 over HMAC-SHA1,
/// which is what every authenticator app speaks).
fn code(secret: &[u8], step: u64) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let mac = hmac::sign(&key, &step.to_be_bytes());
    let mac = mac.as_ref();
    let at = (mac[19] & 15) as usize;
    let word = u32::from_be_bytes([mac[at] & 0x7f, mac[at + 1], mac[at + 2], mac[at + 3]]);
    word % 10u32.pow(DIGITS)
}

impl Auth {
    /// `password_hash` is the Argon2id hash `hash_password` makes, `totp` the
    /// authenticator's secret in base32.
    pub fn new(password_hash: &str, totp: &str) -> Result<Self> {
        let password = password_hash.trim().to_owned();
        let parsed = PasswordHash::new(&password).map_err(|_| anyhow!("STATS_PASSWORD_HASH isn't a password hash"))?;
        if parsed.algorithm.as_str() != "argon2id" {
            bail!("STATS_PASSWORD_HASH has to be argon2id");
        }
        let totp = unbase32(totp).filter(|s| s.len() >= 20).ok_or_else(|| anyhow!("STATS_TOTP_SECRET has to be at least 160 bits of base32"))?;
        Ok(Self { password, totp, used_step: 0, sessions: HashMap::new(), wrong: HashMap::new(), rng: SystemRandom::new() })
    }

    /// Whether `given` is the code for now, or for the step either side of it
    /// (a phone's clock is never quite ours), and hasn't got anyone in yet.
    fn code_step(&self, given: &str, now: u64) -> Option<u64> {
        let given = given.trim();
        if given.len() != DIGITS as usize || !given.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let step = now / STEP;
        (step.saturating_sub(1)..=step + 1).filter(|s| *s > self.used_step).find(|s| {
            // compared as signatures, so right and wrong take the same time
            let key = hmac::Key::new(hmac::HMAC_SHA256, &sha256(&self.totp));
            hmac::verify(&key, format!("{:06}", code(&self.totp, *s)).as_bytes(), hmac::sign(&key, given.as_bytes()).as_ref()).is_ok()
        })
    }

    /// `agent` is the browser's User-Agent: the session it gets only works there.
    pub fn login(&mut self, ip: IpAddr, password: &str, totp: &str, agent: &str, now: u64) -> Login {
        self.wrong.retain(|_, times| {
            times.retain(|t| now.saturating_sub(*t) < TRIES_WINDOW);
            !times.is_empty()
        });
        if self.wrong.get(&ip).is_some_and(|times| times.len() >= TRIES) {
            return Login::Locked;
        }
        // both are always worked out, so the time taken doesn't say which was wrong
        let parsed = PasswordHash::new(&self.password).expect("checked when it was loaded");
        let password_ok = argon().verify_password(password.as_bytes(), &parsed).is_ok();
        let step = self.code_step(totp, now);
        let mut token = [0; 32];
        let (true, Some(step), Ok(())) = (password_ok, step, self.rng.fill(&mut token)) else {
            self.wrong.entry(ip).or_default().push_back(now);
            return Login::Wrong;
        };
        self.used_step = step;
        self.sessions.retain(|_, s| s.until > now && s.seen + IDLE_SECS > now);
        self.sessions.insert(sha256(&token), Session { until: now + SESSION_SECS, seen: now, agent: sha256(agent.as_bytes()) });
        Login::In(hex(&token))
    }

    /// Whether `token` is a live session of this browser. Using it keeps it from going idle.
    pub fn valid(&mut self, token: &str, agent: &str, now: u64) -> bool {
        let Some(bytes) = unhex(token) else { return false };
        let key = sha256(&bytes);
        let Some(session) = self.sessions.get_mut(&key) else { return false };
        if session.until <= now || session.seen + IDLE_SECS <= now || session.agent != sha256(agent.as_bytes()) {
            // expired, or turned up in another browser: it's over either way
            self.sessions.remove(&key);
            return false;
        }
        session.seen = now;
        true
    }

    pub fn logout(&mut self, token: &str) {
        if let Some(bytes) = unhex(token) {
            self.sessions.remove(&sha256(&bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"; // rfc 6238's "12345678901234567890"

    fn auth() -> Auth {
        Auth::new(&hash_password("the right one").unwrap(), SECRET).unwrap()
    }

    fn now_code(now: u64) -> String {
        format!("{:06}", code(&unbase32(SECRET).unwrap(), now / STEP))
    }

    #[test]
    fn codes_match_the_rfc_and_base32_round_trips() {
        let secret = unbase32(SECRET).unwrap();
        assert_eq!(secret, b"12345678901234567890");
        assert_eq!(base32(&secret), SECRET);
        // rfc 6238 appendix b, sha1, truncated to six digits
        assert_eq!(code(&secret, 59 / 30), 287_082);
        assert_eq!(code(&secret, 1_111_111_109 / 30), 81_804);
        assert_eq!(code(&secret, 2_000_000_000 / 30), 279_037);
    }

    #[test]
    fn it_takes_the_password_and_a_fresh_code_and_guessing_locks_the_address_out() {
        let mut a = auth();
        let (me, them): (IpAddr, IpAddr) = ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        let t = 1_800_000_000;
        // either one alone doesn't get in
        assert!(matches!(a.login(them, "the right one", "000000", "firefox", t), Login::Wrong));
        assert!(matches!(a.login(them, "a guess", &now_code(t), "firefox", t), Login::Wrong));
        for _ in 0..3 {
            assert!(matches!(a.login(them, "a guess", "123456", "firefox", t), Login::Wrong));
        }
        // five wrong: even the right pair, once locked
        assert!(matches!(a.login(them, "the right one", &now_code(t), "firefox", t + 1), Login::Locked));
        let Login::In(token) = a.login(me, "the right one", &now_code(t), "firefox", t + 1) else { panic!("the right pair didn't get in") };
        // the same code again doesn't, from anywhere
        assert!(matches!(a.login(me, "the right one", &now_code(t), "firefox", t + 2), Login::Wrong));
        assert!(a.valid(&token, "firefox", t + 3));
        // the lock lifts after the window, with the next code
        let later = t + TRIES_WINDOW + 60;
        assert!(matches!(a.login(them, "the right one", &now_code(later), "firefox", later), Login::In(_)));
        a.logout(&token);
        assert!(!a.valid(&token, "firefox", t + 4));
    }

    #[test]
    fn sessions_end_idle_or_old_or_in_another_browser() {
        let t = 1_800_000_000;
        let token = |a: &mut Auth| match a.login("10.0.0.1".parse().unwrap(), "the right one", &now_code(t), "firefox", t) {
            Login::In(token) => token,
            _ => panic!("didn't get in"),
        };
        let mut a = auth();
        let s = token(&mut a);
        assert!(a.valid(&s, "firefox", t + IDLE_SECS - 1));
        // used, so it's good for another stretch, but not past a day
        assert!(a.valid(&s, "firefox", t + 2 * IDLE_SECS - 2));
        let mut a = auth();
        let s = token(&mut a);
        assert!(!a.valid(&s, "firefox", t + IDLE_SECS));
        let mut a = auth();
        let s = token(&mut a);
        for hour in 1..24 {
            assert!(a.valid(&s, "firefox", t + hour * 3600));
        }
        assert!(!a.valid(&s, "firefox", t + SESSION_SECS));
        // a stolen cookie in another browser ends the session for both
        let mut a = auth();
        let s = token(&mut a);
        assert!(!a.valid(&s, "curl", t + 1));
        assert!(!a.valid(&s, "firefox", t + 2));
        assert!(!a.valid(&"0".repeat(64), "firefox", t) && !a.valid("", "firefox", t));
    }
}
