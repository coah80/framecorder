use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::hkdf::{Salt, HKDF_SHA256};
use ring::rand::SystemRandom;

pub const SERVICE: [u8; 32] = hex(b"4052517de9595727bf0a4b9a349a786a8269d501bb9a7362d5d277414be8de2c");
const INFO: &[u8] = b"framecorder ping v1";

pub fn seal(to: &[u8; 32], plain: &[u8]) -> Option<Vec<u8>> {
    let mine = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).ok()?;
    let ours = mine.compute_public_key().ok()?;
    agree_ephemeral(mine, &UnparsedPublicKey::new(&X25519, to), |shared| {
        let salt = [ours.as_ref(), to.as_slice()].concat();
        let prk = Salt::new(HKDF_SHA256, &salt).extract(shared);
        let key = LessSafeKey::new(UnboundKey::from(prk.expand(&[INFO], &CHACHA20_POLY1305).ok()?));
        let mut boxed = plain.to_vec();
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
        assert!(!a.windows(8).any(|w| plain.windows(8).any(|p| p == w)));
        assert_ne!(a, b);
    }
}
