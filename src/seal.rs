//! Seals a stats ping to the stats service's own key before it leaves, so
//! only the service can read it: not the network, not Cloudflare's edge
//! where TLS ends. The other half is `stats/src/seal.rs`; the same code is in
//! `app/src/core/seal.rs` for the apps.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::hkdf::{Salt, HKDF_SHA256};
use ring::rand::SystemRandom;

/// The stats service's public key (`framecorder-stats seal-key`).
pub const SERVICE: [u8; 32] = hex(b"4052517de9595727bf0a4b9a349a786a8269d501bb9a7362d5d277414be8de2c");
const INFO: &[u8] = b"framecorder ping v1";

/// `1`, a one-off X25519 public key, then `plain` under ChaCha20-Poly1305
/// keyed from that key and `to`'s shared secret.
pub fn seal(to: &[u8; 32], plain: &[u8]) -> Option<Vec<u8>> {
    let mine = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).ok()?;
    let ours = mine.compute_public_key().ok()?;
    agree_ephemeral(mine, &UnparsedPublicKey::new(&X25519, to), |shared| {
        let salt = [ours.as_ref(), to.as_slice()].concat();
        let prk = Salt::new(HKDF_SHA256, &salt).extract(shared);
        let key = LessSafeKey::new(UnboundKey::from(prk.expand(&[INFO], &CHACHA20_POLY1305).ok()?));
        let mut boxed = plain.to_vec();
        // the key is new with every ping, so one nonce never meets it twice
        key.seal_in_place_append_tag(Nonce::assume_unique_for_key([0; 12]), Aad::from(INFO), &mut boxed).ok()?;
        Some([&[1u8], ours.as_ref(), &boxed].concat())
    })
    .ok()
    .flatten()
}

const fn hex(s: &[u8; 64]) -> [u8; 32] {
    const fn digit(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("not hex"),
        }
    }
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = digit(s[2 * i]) << 4 | digit(s[2 * i + 1]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_leaves_sealed() {
        let plain = br#"{"app":"headset","version":"0.2.0"}"#;
        let (a, b) = (seal(&SERVICE, plain).unwrap(), seal(&SERVICE, plain).unwrap());
        assert_eq!((a[0], a.len()), (1, 1 + 32 + plain.len() + 16));
        // nothing of it readable, and never the same twice
        assert!(!a.windows(8).any(|w| plain.windows(8).any(|p| p == w)));
        assert_ne!(a, b);
    }
}
