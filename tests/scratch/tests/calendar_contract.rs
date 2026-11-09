//! Google Calendar's contract test (connectors spec, Brokered API
//! connectors: "Every brokered connector runs contract tests in CI against
//! provider test accounts before its manifest is published"): the image the
//! catalog names, built from this checkout, in its own Firecracker microVM,
//! against Calendar itself: with no account, that Calendar is reached and
//! an unknown token is said as a sign-in to do again; with the owner's
//! Google test account, every tool, once it is given:
//!
//! - `MAGNUS_GOOGLE_TEST_REFRESH_TOKEN`: a refresh token of the test
//!   account's, signed in with Alpha Vector's Google client
//!   (`catalog/registrations.toml`) for the consent screen's scopes;
//! - `MAGNUS_GOOGLE_CLIENT_SECRET`: that client's secret.
//!
//! Without them it says so. It invites no one but the account itself and
//! tells no one; the one event it adds it deletes.

use magnus::apps::{CallError, CallOutput};
use magnus::brokered::Brokered;
use magnus::config;
use serde_json::{json, Value};
use std::path::PathBuf;

fn root() -> PathBuf {
    magnus_invariants::appliance::workspace_root()
}

/// An access token for the test account, from Google.
async fn access_token(refresh: &str, secret: &str) -> String {
    let r: toml::Table = std::fs::read_to_string(root().join("catalog/registrations.toml")).unwrap().parse().unwrap();
    let client = r["google"]["client_id"].as_str().unwrap();
    let v: Value = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client), ("client_secret", secret)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("Google refused the test account's refresh token (a Testing app's lasts seven days): {v}")).to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn calendar_s_tools_do_what_they_say_against_calendar() {
    let mf: toml::Table = std::fs::read_to_string(root().join("catalog/google-calendar.toml")).unwrap().parse().unwrap();
    let images = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(root().join("brokered/build.sh")).arg("magnus-calendar").arg(images.path()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(mf["endpoint"].as_str(), Some(digest.as_str()), "catalog/google-calendar.toml names another image than the one built from this commit");

    let guest = |var: &str, file: &str| std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| root().join("sandbox/out").join(file));
    let sandbox = config::Sandbox {
        firecracker: std::env::var("MAGNUS_FIRECRACKER").unwrap_or("/usr/local/bin/firecracker".into()).into(),
        kernel_image: guest("MAGNUS_GUEST_KERNEL", "vmlinux"),
        rootfs: guest("MAGNUS_GUEST_ROOTFS", "rootfs.ext4"),
        jailer: None,
        vcpus: 1,
        memory_mib: 256,
        workspace_mib: 64,
    };
    let state = tempfile::tempdir().unwrap();
    let cfg = config::Brokered { images: Some(images.path().into()), ..Default::default() };
    let b = Brokered::new(Some(sandbox), cfg, state.path());
    let mut c: config::Connector =
        toml::from_str(&format!("name = \"google-calendar\"\ntransport = \"brokered\"\nurl = \"{digest}\"\naudience = \"public\"\nlabel = \"private+untrusted\"")).unwrap();
    c.hosts = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    c.scopes = mf["scopes"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();

    // Every tool it lists is classed in the manifest, and the other way round.
    let mut listed: Vec<String> = b.list_tools(&c).await.unwrap().into_iter().map(|t| t.name).collect();
    let mut classed: Vec<String> = mf["tools"].as_table().unwrap().keys().cloned().collect();
    listed.sort();
    classed.sort();
    assert_eq!(listed, classed);

    // Calendar itself, reached from the microVM over TLS it checks: a token
    // Google does not know is said as a sign-in to do again.
    let r = b.call(&c, "contract", Some("ya29.not-a-token"), "list_calendars", &json!({}), &|| Ok(())).await.unwrap();
    assert!(r.is_error && r.text.contains("Google Calendar rejected the sign-in"), "{}", r.text);

    let (Ok(refresh), Ok(secret)) = (std::env::var("MAGNUS_GOOGLE_TEST_REFRESH_TOKEN"), std::env::var("MAGNUS_GOOGLE_CLIENT_SECRET")) else {
        eprintln!("the rest of Calendar's contract test needs the owner's Google test account (MAGNUS_GOOGLE_TEST_REFRESH_TOKEN, MAGNUS_GOOGLE_CLIENT_SECRET); not given, so it was not run");
        b.stop(None, None).await;
        return;
    };

    let token = access_token(&refresh, &secret).await;
    let call = |tool: &'static str, args: Value| {
        let (b, c, token) = (&b, &c, token.clone());
        async move {
            let r: Result<CallOutput, CallError> = b.call(c, "contract", Some(&token), tool, &args, &|| Ok(())).await;
            let r = r.unwrap_or_else(|e| panic!("{tool}: {e:?}"));
            assert!(!r.is_error, "{tool}: {}", r.text);
            r.structured.unwrap_or_else(|| panic!("{tool}: no structured answer"))
        }
    };

    // The account's own calendar, named by its address.
    let cals = call("list_calendars", json!({})).await;
    let me = cals["calendars"].as_array().unwrap().iter().find(|c| c["primary"] == true).unwrap_or_else(|| panic!("{cals}"))["calendar_id"].as_str().unwrap().to_string();

    // An hour, a week on, with the account itself as its one guest, telling
    // no one: added, read, found, changed, answered and deleted.
    let at = chrono::Utc::now() + chrono::Duration::days(7);
    let (start, end) = (at.format("%Y-%m-%dT%H:00:00Z").to_string(), (at + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:00:00Z").to_string());
    let summary = format!("Magnus contract test {}", uuid::Uuid::new_v4());
    let e = call("create_event", json!({ "summary": summary, "start_time": start, "end_time": end, "attendees": [me], "notify": false })).await;
    let id = e["event_id"].as_str().unwrap().to_string();
    let got = call("get_event", json!({ "event_id": id })).await;
    assert_eq!(got["summary"], summary.as_str(), "{got}");
    let day = (at - chrono::Duration::days(1)).format("%Y-%m-%dT00:00:00Z").to_string();
    let listed = call("list_events", json!({ "start_time": day, "max_results": 250 })).await;
    assert!(listed["events"].as_array().unwrap().iter().any(|e| e["event_id"] == id.as_str()), "{listed}");
    call("search_events", json!({ "query": summary, "start_time": day })).await;
    let moved = call("update_event", json!({ "event_id": id, "location": "Contract room", "notify": false })).await;
    assert_eq!((moved["location"].as_str(), moved["summary"].as_str()), (Some("Contract room"), Some(summary.as_str())), "{moved}");
    let answered = call("respond_to_event", json!({ "event_id": id, "response": "tentative", "notify": false })).await;
    assert!(answered["attendees"].as_array().unwrap().iter().any(|p| p["me"] == true && p["response"] == "tentative"), "{answered}");
    let free = call("suggest_time", json!({ "start_time": start, "end_time": end, "duration_minutes": 30 })).await;
    assert!(free["suggestions"].as_array().unwrap().is_empty(), "the hour it holds is not busy: {free}");
    assert_eq!(call("delete_event", json!({ "event_id": id, "notify": false })).await["deleted"], true);
    b.stop(None, None).await;
}
