use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Arc};

use framecorder_app_lib::core::tls::sha256_hex;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

pub struct Request {
    pub line: String,
    pub headers: Vec<String>,
    pub body: String,
}

pub fn server(responses: Vec<(u16, &'static str, String)>) -> (String, String, mpsc::Receiver<Request>, std::thread::JoinHandle<()>) {
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
    let (tx, rx) = mpsc::channel();
    let task = std::thread::spawn(move || {
        let mut streams = Vec::new();
        for (status, content_type, body) in responses {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let conn = rustls::ServerConnection::new(config.clone()).unwrap();
            let mut tls = BufReader::new(rustls::StreamOwned::new(conn, stream));
            let mut line = String::new();
            tls.read_line(&mut line).unwrap();
            let mut headers = Vec::new();
            let mut length = 0;
            loop {
                let mut header = String::new();
                assert!(tls.read_line(&mut header).unwrap() > 0);
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                headers.push(header.trim().to_ascii_lowercase());
            }
            let mut data = vec![0; length];
            tls.read_exact(&mut data).unwrap();
            tx.send(Request { line, headers, body: String::from_utf8(data).unwrap() }).unwrap();
            write!(tls.get_mut(), "HTTP/1.1 {status} OK\r\nConnection: close\r\nContent-Type: {content_type}\r\n").unwrap();
            if content_type != "text/event-stream" {
                write!(tls.get_mut(), "Content-Length: {}\r\n", body.len()).unwrap();
            }
            write!(tls.get_mut(), "\r\n{body}").unwrap();
            tls.get_mut().flush().unwrap();
            if content_type == "text/event-stream" {
                streams.push(tls);
            }
        }
    });
    (addr, fp, rx, task)
}
