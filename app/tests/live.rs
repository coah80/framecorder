use std::sync::Arc;
use std::time::Duration;

use framecorder_app_lib::core::api::Client;
use framecorder_app_lib::core::live::{LivePhase, LiveRelay};
use framecorder_app_lib::core::tls::sha256_hex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{mpsc, watch};

struct Headset {
    addr: String,
    fp: String,
    feeds: mpsc::Receiver<mpsc::Sender<Vec<u8>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Headset {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn headset(status: u16) -> Headset {
    let cert = rcgen::generate_simple_self_signed(vec!["framecorder.local".into()]).unwrap();
    let fp = sha256_hex(cert.cert.der());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (feeds_tx, feeds) = mpsc::channel(4);
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let mut tls = match acceptor.accept(socket).await {
                Ok(tls) => tls,
                Err(_) => continue,
            };
            let request = header(&mut tls).await;
            assert!(request.starts_with("GET /live HTTP/1.1\r\n"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer synthetic-token\r\n"));
            if status != 200 {
                tls.write_all(
                    format!(
                        "HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
                continue;
            }
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: video/mp2t\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
            let (tx, mut rx) = mpsc::channel::<Vec<u8>>(4);
            feeds_tx.send(tx).await.unwrap();
            while let Some(chunk) = rx.recv().await {
                if tls
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                if tls.write_all(&chunk).await.is_err() {
                    break;
                }
                if tls.write_all(b"\r\n").await.is_err() {
                    break;
                }
                if tls.flush().await.is_err() {
                    break;
                }
            }
        }
    });
    Headset {
        addr,
        fp,
        feeds,
        task,
    }
}

async fn header(socket: &mut (impl tokio::io::AsyncRead + Unpin)) -> String {
    let mut data = Vec::new();
    while !data.ends_with(b"\r\n\r\n") {
        data.push(socket.read_u8().await.unwrap());
    }
    String::from_utf8(data).unwrap()
}

fn client(server: &Headset) -> Client {
    Client::pinned(&server.addr, &server.fp, "desktop/test")
        .unwrap()
        .with_token("synthetic-token")
}

async fn player(url: &str, stalled: bool) -> TcpStream {
    let addr = url
        .trim_start_matches("http://")
        .trim_end_matches("/live.ts");
    let socket = TcpSocket::new_v4().unwrap();
    if stalled {
        socket.set_recv_buffer_size(1024).unwrap();
    }
    let mut socket = socket.connect(addr.parse().unwrap()).await.unwrap();
    socket
        .write_all(b"GET /live.ts HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let h = header(&mut socket).await;
    assert!(h.starts_with("HTTP/1.1 200"));
    assert!(h.contains("Content-Type: video/mp2t"));
    socket
}

async fn until(
    state: &mut watch::Receiver<framecorder_app_lib::core::live::LiveStatus>,
    condition: impl Fn(&framecorder_app_lib::core::live::LiveStatus) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if condition(&state.borrow()) {
                return;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shares_real_transport_stream_drops_slow_players_and_reconnects() {
    tokio::time::timeout(Duration::from_secs(40), async {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("stream.ts");
        let generated = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "0.3",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-f",
                "mpegts",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg generates the MPEG-TS fixture");
        assert!(generated.success());
        let fixture = std::fs::read(path).unwrap();
        assert_eq!(fixture[0], 0x47);
        assert_eq!(fixture.len() % 188, 0);
        let mut headset = headset(200).await;
        let mut relay = LiveRelay::start(client(&headset)).await.unwrap();
        let mut state = relay.subscribe();
        let feed = headset.feeds.recv().await.unwrap();
        let mut a = player(&relay.status().url, false).await;
        let mut b = player(&relay.status().url, false).await;
        until(&mut state, |s| s.viewers == 2).await;
        feed.send(fixture.clone()).await.unwrap();
        let mut received = vec![0; fixture.len()];
        a.read_exact(&mut received).await.unwrap();
        assert_eq!(received, fixture);
        b.read_exact(&mut received).await.unwrap();
        assert_eq!(received, fixture);
        assert!(
            headset.feeds.try_recv().is_err(),
            "local players share one upstream"
        );
        drop(b);
        until(&mut state, |s| s.viewers == 1).await;
        let slow = player(&relay.status().url, true).await;
        until(&mut state, |s| s.viewers == 2).await;
        let chunk = fixture.repeat(32);
        let mut received = vec![0; chunk.len()];
        for _ in 0..512 {
            feed.send(chunk.clone()).await.unwrap();
            a.read_exact(&mut received).await.unwrap();
            assert_eq!(received, chunk);
            if relay.status().viewers == 1 {
                break;
            }
        }
        until(&mut state, |s| s.viewers == 1).await;
        drop(slow);
        drop(feed);
        let second_feed = headset.feeds.recv().await.unwrap();
        let mut c = player(&relay.status().url, false).await;
        second_feed.send(fixture.clone()).await.unwrap();
        let mut received = vec![0; fixture.len()];
        c.read_exact(&mut received).await.unwrap();
        assert_eq!(received, fixture);
        relay.stop().await;
        assert_eq!(relay.status().phase, LivePhase::Stopped);
        assert_eq!(relay.status().viewers, 0);
        assert!(TcpStream::connect(
            relay
                .status()
                .url
                .trim_start_matches("http://")
                .trim_end_matches("/live.ts")
        )
        .await
        .is_err());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn old_headsets_explain_the_update_and_bad_pins_never_send_a_token() {
    let headset = headset(404).await;
    let mut relay = LiveRelay::start(client(&headset)).await.unwrap();
    let mut state = relay.subscribe();
    until(&mut state, |s| s.phase == LivePhase::Error).await;
    assert!(relay.status().error.unwrap().contains("needs an update"));
    relay.stop().await;
    assert_eq!(relay.status().phase, LivePhase::Stopped);
    let wrong = Client::pinned(&headset.addr, &"00".repeat(32), "desktop/test")
        .unwrap()
        .with_token("synthetic-token");
    let mut relay = LiveRelay::start(wrong).await.unwrap();
    let mut state = relay.subscribe();
    until(&mut state, |s| s.phase == LivePhase::Error).await;
    assert!(relay
        .status()
        .error
        .unwrap()
        .contains("isn't the frame you paired with"));
    relay.stop().await;
}
