//! TLS that trusts exactly one certificate: the one whose fingerprint we
//! got when pairing (from the QR code or the Frame's mDNS record). No CA
//! store is involved at all.

use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};

pub fn sha256_hex(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Lowercase hex, 64 chars. Anything else can't be a fingerprint we made.
pub fn normalize_fingerprint(fp: &str) -> Option<String> {
    let fp: String = fp.trim().chars().filter(|c| *c != ':').collect::<String>().to_ascii_lowercase();
    (fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit())).then_some(fp)
}

#[derive(Debug)]
pub struct PinnedVerifier {
    /// None means "accept anything, but remember what we saw", used once to
    /// learn the fingerprint of a Frame typed in by address.
    expected: Option<String>,
    seen: Mutex<Option<String>>,
    algs: WebPkiSupportedAlgorithms,
}

impl PinnedVerifier {
    pub fn pinned(fingerprint: &str) -> Arc<Self> {
        Arc::new(Self::new(Some(fingerprint.to_ascii_lowercase())))
    }

    pub fn capture() -> Arc<Self> {
        Arc::new(Self::new(None))
    }

    fn new(expected: Option<String>) -> Self {
        let algs = rustls::crypto::ring::default_provider().signature_verification_algorithms;
        Self { expected, seen: Mutex::new(None), algs }
    }

    pub fn seen(&self) -> Option<String> {
        self.seen.lock().unwrap().clone()
    }

    pub fn check(&self, cert: &[u8]) -> Result<(), Error> {
        let got = sha256_hex(cert);
        *self.seen.lock().unwrap() = Some(got.clone());
        match &self.expected {
            None => Ok(()),
            Some(want) if same(want.as_bytes(), got.as_bytes()) => Ok(()),
            Some(_) => Err(Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)),
        }
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check(end_entity.as_ref()).map(|_| ServerCertVerified::assertion())
    }

    // the server still has to prove it holds the key for that certificate
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

pub fn client_config(verifier: Arc<PinnedVerifier>) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_accepts_only_its_certificate() {
        let cert = b"pretend this is DER";
        let fp = sha256_hex(cert);
        let v = PinnedVerifier::pinned(&fp);
        assert!(v.check(cert).is_ok());
        assert!(v.check(b"some other cert").is_err());
        assert!(PinnedVerifier::pinned(&fp.to_ascii_uppercase()).check(cert).is_ok());
        assert!(PinnedVerifier::pinned("").check(cert).is_err());
    }

    #[test]
    fn fingerprints_get_normalized() {
        let fp = "AB".repeat(32);
        assert_eq!(normalize_fingerprint(&fp).unwrap(), "ab".repeat(32));
        let colons: Vec<String> = (0..32).map(|_| "AB".to_string()).collect();
        assert_eq!(normalize_fingerprint(&colons.join(":")).unwrap(), "ab".repeat(32));
        assert!(normalize_fingerprint("abc").is_none());
        assert!(normalize_fingerprint(&"zz".repeat(32)).is_none());
    }
}
