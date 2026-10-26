mod support;

use framecorder_app_lib::core::api::{Client, Recording, RecordingPatch, RemoteClip};
use serde_json::json;

const SETTINGS: &str = r#"{"shape":"wide","quality":"standard","fps":"auto","game_audio":true,"mic":false,"clips":true,"clip":30,"eye":"right","panels":false}"#;

#[tokio::test]
async fn identity_partial_settings_and_unpair_use_the_wire_contract() {
    let (addr, fp, rx, task) = support::server(vec![
        (200, "application/json", json!({"name":"frame","version":1,"fingerprint":"unused"}).to_string()),
        (200, "application/json", r#"{"token":"t","device_id":"d"}"#.into()),
        (200, "application/json", SETTINGS.into()),
        (200, "application/json", SETTINGS.into()),
        (200, "text/event-stream", format!("event: settings\ndata: {SETTINGS}\n\n")),
        (200, "video/mp4", "video".into()),
        (204, "application/json", String::new()),
    ]);
    let client = Client::pinned(&addr, &fp, "desktop/9.8.7").unwrap().with_token("token");
    client.hello().await.unwrap();
    client.pair("123456", "test").await.unwrap();
    let recording = client.recording().await.unwrap().unwrap();
    assert_eq!(recording.eye.as_deref(), Some("right"));
    assert_eq!(recording.panels, Some(false));
    client.set_recording(&RecordingPatch { mic: Some(false), ..Default::default() }).await.unwrap();
    let event = client.events().await.unwrap().next().await.unwrap();
    assert_eq!(event.event, "settings");
    assert_eq!(serde_json::from_str::<Recording>(&event.data).unwrap(), recording);
    let dir = tempfile::tempdir().unwrap();
    let clip = RemoteClip { id: "r-test".into(), kind: "recording".into(), name: "test.mp4".into(), size: 5, duration_s: None, created: 0 };
    client.download(&clip, &dir.path().join("test.part"), |_, _| {}).await.unwrap();
    client.unpair().await.unwrap();
    task.join().unwrap();
    let requests: Vec<_> = rx.try_iter().collect();
    assert_eq!(requests.len(), 7);
    for request in &requests {
        assert!(request.headers.iter().any(|h| h == "x-framecorder-app: desktop/9.8.7"));
        assert!(request.headers.iter().any(|h| h == "authorization: bearer token"));
    }
    assert!(requests[3].line.starts_with("PATCH /recording "));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&requests[3].body).unwrap(), json!({"mic":false}));
    assert!(requests[6].line.starts_with("DELETE /pair "));
}

#[test]
fn older_recording_settings_keep_unknown_capabilities_optional() {
    let old = r#"{"shape":"wide","quality":"standard","fps":"auto","game_audio":true,"mic":true,"clips":false,"clip":30}"#;
    let recording: Recording = serde_json::from_str(old).unwrap();
    assert_eq!(recording.eye, None);
    assert_eq!(recording.panels, None);
}

struct Quiet;
impl framecorder_app_lib::core::engine::Listener for Quiet {
    fn status(&self, _: &framecorder_app_lib::core::engine::Status) {}
    fn progress(&self, _: &framecorder_app_lib::core::engine::Progress) {}
    fn synced(&self, _: &framecorder_app_lib::core::store::Entry) {}
}

#[tokio::test]
async fn unpair_forgets_locally_even_when_the_headset_is_older() {
    use framecorder_app_lib::core::{
        engine::{DirSink, Engine, Forgot},
        store::{Host, Hosts},
    };
    use std::sync::Arc;
    for (status, expected) in [(204, Forgot::Both), (404, Forgot::LocalOnly)] {
        let (addr, fp, rx, task) = support::server(vec![(status, "application/json", String::new())]);
        let dir = tempfile::tempdir().unwrap();
        Hosts::load(dir.path())
            .upsert(Host { fingerprint: fp.clone(), name: "frame".into(), addr, token: "t".into(), device_id: "d".into(), paired_at: 0 })
            .unwrap();
        let engine = Engine::new(
            tokio::runtime::Handle::current(),
            dir.path(),
            Arc::new(DirSink { root: dir.path().join("videos") }),
            Arc::new(Quiet),
            "test",
            "desktop/test".into(),
        );
        assert_eq!(engine.unpair(&fp).await.unwrap(), expected);
        assert!(engine.hosts().is_empty());
        assert!(Hosts::load(dir.path()).list().is_empty());
        assert!(rx.recv().unwrap().line.starts_with("DELETE /pair "));
        task.join().unwrap();
    }
}
