//! Tells a release that's ours from one that isn't, before it's installed as
//! an update. A release's `.sha256` file (its checksum, its name and its
//! version) is signed with a key that's kept off GitHub (`packaging/sign.sh`),
//! and the signature goes up next to it as `.sig`. So whoever gets into the
//! repo, its releases or the site still can't hand installs an update: they'd
//! need the key too. The headset's updater checks it here, and the desktop
//! app `#[path]`-includes this file for its own.

use ring::signature::{UnparsedPublicKey, ED25519};

/// The public halves of the release keys. The second is a spare, kept apart
/// from the first, for the day the first one's lost.
pub const KEYS: [[u8; 32]; 2] = [
    hex(b"0b52dea5e2f09b3904660ba548b7b2404009267c2d6b77b071ed9a079b502584"),
    hex(b"443b627faeee6a8c013a53c3ff71c2e39009039b6b8317d572dbad289ec5742c"),
];

/// What a signature covers begins with this, so it can't be passed off as
/// anything else the key might sign one day.
const PURPOSE: &[u8] = b"framecorder release v1\n";

const fn hex(text: &[u8; 64]) -> [u8; 32] {
    const fn digit(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("a key is lowercase hex"),
        }
    }
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = digit(text[2 * i]) << 4 | digit(text[2 * i + 1]);
        i += 1;
    }
    out
}

/// What a signed `.sha256` file says about its release.
#[derive(Debug, PartialEq)]
pub struct Listing {
    /// The file's SHA-256, lowercase hex.
    pub sum: String,
    pub name: String,
    pub version: String,
}

/// `listing` is a release's `.sha256` file exactly as downloaded, `signature`
/// its `.sig`. What the listing says, if one of `keys` signed it.
pub fn verify(listing: &str, signature: &str, keys: &[[u8; 32]]) -> Option<Listing> {
    let signature = signature.trim();
    if signature.len() != 128 || !signature.is_ascii() {
        return None;
    }
    let signature: Vec<u8> = (0..64).map(|i| u8::from_str_radix(&signature[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>().ok()?;
    let message = [PURPOSE, listing.as_bytes()].concat();
    if !keys.iter().any(|key| UnparsedPublicKey::new(&ED25519, key).verify(&message, &signature).is_ok()) {
        return None;
    }
    let mut lines = listing.lines();
    let (sum, name) = lines.next()?.split_once("  ")?;
    let version = lines.find_map(|l| l.strip_prefix("version "))?.trim();
    let hex = sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (hex && !name.is_empty() && !version.is_empty()).then(|| Listing { sum: sum.into(), name: name.into(), version: version.into() })
}

/// Whether `version` is `current` or newer, so an old release, signed in its
/// day, can't be served again to take an install backwards.
pub fn at_least(version: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split(['.', '-']).map_while(|p| p.parse().ok()).collect() };
    parts(version) >= parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn a_release_counts_only_as_it_was_signed() {
        let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let ours = [pair.public_key().as_ref().try_into().unwrap()];
        let sum = "ab".repeat(32);
        let listing = format!("{sum}  framecorder-arm64.tar.gz\nversion 0.2.1\n");
        let sign = |text: &str| pair.sign(&[PURPOSE, text.as_bytes()].concat()).as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>();
        let signature = sign(&listing);

        let read = verify(&listing, &format!("{signature}\n"), &ours).unwrap();
        assert_eq!(read, Listing { sum: sum.clone(), name: "framecorder-arm64.tar.gz".into(), version: "0.2.1".into() });
        // another file, another name or another version under the same signature
        for changed in [listing.replace("ab", "cd"), listing.replace("arm64", "evil"), listing.replace("0.2.1", "9.9.9")] {
            assert_eq!(verify(&changed, &signature, &ours), None, "{changed}");
        }
        // someone else's key, however well formed the rest is
        assert_eq!(verify(&listing, &signature, &KEYS), None);
        // signed, but not a listing; and not a signature at all
        assert_eq!(verify("hello\n", &sign("hello\n"), &ours), None);
        for junk in ["", "zz", &signature[2..], &format!("{}é", &signature[..126])] {
            assert_eq!(verify(&listing, junk, &ours), None, "{junk}");
        }
    }

    #[test]
    fn what_sign_sh_signs_is_what_installs_accept() {
        // packaging/sign.sh on a file holding "not a release\n", with the real key
        let listing = "e1b4ae11edeff39246d89d887c09d87b72980afb4f01302a14fb7ed24fcf4f6b  interop-test\nversion 0.0.0\n";
        let signature = "2f6e956640fce2668bf2154dda794587954cc89afb0b0d9aeb76d480760e65bec6c167bf35b122b577fd875d28df516b5e9c48119d16a338cdc9586a5538e90a\n";
        assert_eq!(verify(listing, signature, &KEYS).map(|l| l.name), Some("interop-test".into()));
    }

    #[test]
    fn a_release_cant_take_an_install_backwards() {
        assert!(at_least("0.2.1", "0.2.0") && at_least("0.2.0", "0.2.0") && at_least("v0.10.0", "0.9.9") && at_least("1.0.0", "0.99.99"));
        assert!(!at_least("0.1.4", "0.2.0") && !at_least("0.2", "0.2.0") && !at_least("", "0.2.0"));
    }
}
