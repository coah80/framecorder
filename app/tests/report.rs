mod support;

use framecorder_app_lib::core::{
    api::Client,
    report::{self, AppInfo},
};
use serde_json::json;

fn app() -> AppInfo {
    AppInfo {
        name: "framecorder desktop".into(),
        version: "test".into(),
        os: "test".into(),
        device: "test".into(),
        arch: "test".into(),
    }
}

#[tokio::test]
async fn diagnostics_preview_uses_authenticated_pinned_http_and_handles_old_headsets() {
    for old in [false, true] {
        let responses = if old {
            vec![
                (404, "application/json", "{}".into()),
                (
                    200,
                    "application/json",
                    json!({"name":"frame", "version":1, "fingerprint":"test"}).to_string(),
                ),
                (200, "application/json", json!({"battery":{"percent":50}}).to_string()),
            ]
        } else {
            vec![(200, "application/json", json!({"steam":{"name":"Persona"}, "logs":[{"name":"recorder.log", "text":"failed at /home/somebody/file via 8.8.8.8:443"}], "token":"never include", "recent_errors":["failure"]}).to_string())]
        };
        let (addr, fp, requests, thread) = support::server(responses);
        let client = Client::pinned(&addr, &fp, "desktop/test").unwrap().with_token("fake-pairing-token");
        let preview = report::preview_with_client("it failed", app(), Some(&client)).await;
        thread.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.line.starts_with("GET /diagnostics "));
        assert!(request.headers.contains(&"authorization: bearer fake-pairing-token".into()));
        assert!(request.body.is_empty());
        let body = serde_json::to_string(&preview).unwrap();
        assert!(!body.contains("never include"));
        assert!(!body.contains("8.8.8.8"));
        assert!(!body.contains("somebody"));
        if old {
            assert_eq!(
                preview.headset_unavailable.as_deref(),
                Some("the headset runs an older framecorder")
            );
            assert_eq!(preview.headset.unwrap()["frame"]["battery"]["percent"], 50);
        } else {
            assert!(preview.headset_unavailable.is_none());
            assert_eq!(preview.headset.unwrap()["steam"]["name"], "Persona");
        }
    }
}

#[tokio::test]
async fn disconnected_and_unpaired_apps_can_still_preview_a_report() {
    let unpaired = report::preview("broken", app()).await;
    assert_eq!(unpaired.headset_unavailable.as_deref(), Some("no headset is paired"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = Client::pinned(&addr.to_string(), &"ab".repeat(32), "desktop/test").unwrap();
    let unreachable = report::preview_with_client("broken", app(), Some(&client)).await;
    assert!(unreachable.headset.is_none());
    assert!(unreachable.headset_unavailable.unwrap().contains("could not be reached"));
}

#[test]
fn private_data_is_removed_without_losing_lan_addresses_or_persona() {
    for text in [
        "/home/someone/test",
        r"C:\Users\someone\test",
        "/Users/someone/test",
        "8.8.8.8:443",
        "2001:4860:4860::8888",
        "aa:bb:cc:dd:ee:ff",
        "token=secret-value",
        "pairing code 123456",
        "serial LHR-1234",
    ] {
        let redacted = report::redact(text);
        assert!(!redacted.contains("someone"), "{redacted}");
        assert!(!redacted.contains("secret-value"), "{redacted}");
        assert!(!redacted.contains("123456"), "{redacted}");
        assert_ne!(redacted, text, "{text}");
    }
    let lan = "192.168.1.2:38619 10.0.0.1 172.16.0.2 127.0.0.1 ::1 fd00::1 Persona";
    assert_eq!(report::redact(lan), lan);
}
