//! The client against a real TLS server: the pinned certificate gets
//! through, anything else is refused before a single byte of HTTP.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use framecorder_app_lib::core::api::{ApiError, Client};
use framecorder_app_lib::core::discover::identify;
use framecorder_app_lib::core::tls::sha256_hex;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A one-trick HTTPS server: answers every request with a fixed /hello.
/// Returns its address, its fingerprint and how many requests it's seen.
fn server() -> (String, String, Arc<AtomicUsize>) {
    server_claiming(None)
}

/// Same, but its /hello claims `claim` as its fingerprint instead of the real one.
fn server_claiming(claim: Option<String>) -> (String, String, Arc<AtomicUsize>) {
    let made = rcgen::generate_simple_self_signed(vec!["framecorder.local".into()]).unwrap();
    let cert = CertificateDer::from(made.cert.der().to_vec());
    let fp = sha256_hex(cert.as_ref());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let hits = Arc::new(AtomicUsize::new(0));
    let (hits2, fp2) = (hits.clone(), claim.unwrap_or_else(|| fp.clone()));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let conn = rustls::ServerConnection::new(config.clone()).unwrap();
            let mut tls = BufReader::new(rustls::StreamOwned::new(conn, stream));
            let mut line = String::new();
            loop {
                line.clear();
                if tls.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" {
                    hits2.fetch_add(1, Ordering::SeqCst);
                    let body = format!(r#"{{"name":"test","version":1,"fingerprint":"{fp2}"}}"#);
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tls.get_mut().write_all(resp.as_bytes());
                    let _ = tls.get_mut().flush();
                }
            }
        }
    });
    (addr, fp, hits)
}

#[tokio::test]
async fn right_fingerprint_connects() {
    let (addr, fp, hits) = server();
    let hello = Client::pinned(&addr, &fp).unwrap().hello().await.unwrap();
    assert_eq!(hello.fingerprint, fp);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wrong_fingerprint_is_refused_before_any_request() {
    let (addr, fp, hits) = server();
    let mut wrong = fp.into_bytes();
    wrong[0] = if wrong[0] == b'a' { b'b' } else { b'a' };
    let wrong = String::from_utf8(wrong).unwrap();
    let client = Client::pinned(&addr, &wrong).unwrap().with_token("secret");
    assert_eq!(client.hello().await.unwrap_err(), ApiError::WrongFingerprint);
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the token must never be sent");
}

#[tokio::test]
async fn sweep_identifies_a_frame_by_its_certificate() {
    let (addr, fp, _) = server();
    let found = identify(&addr).await.expect("a framecorder-sync answered");
    assert_eq!(found.fingerprint, fp);
    assert_eq!(found.name, "test");
    assert_eq!(found.addrs, vec![addr]);
}

#[tokio::test]
async fn sweep_ignores_a_server_claiming_someone_elses_fingerprint() {
    let (addr, _, _) = server_claiming(Some("ab".repeat(32)));
    assert!(identify(&addr).await.is_none());
}

#[tokio::test]
async fn capture_learns_the_fingerprint() {
    let (addr, fp, _) = server();
    let client = Client::capture(&addr).unwrap();
    client.hello().await.unwrap();
    assert_eq!(client.seen_fingerprint().unwrap(), fp);
}
