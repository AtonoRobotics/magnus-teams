//! The patterns every screen of the admin console uses (admin console spec,
//! Web UI, Shell), in a real Chromium against the local store: the admin's
//! menu sets the theme; live data arrives over server-sent events (the kill
//! switch's banner appears with no reload); a step-up opens the console's
//! own sign-in in its own window and the action goes on after it; a list's row
//! opens its detail panel; a confirm sheet cancelled changes nothing; a
//! reversible change offers Undo. The stream itself: refused without a
//! session, a `status` event first, and over once the session is.
//! Overview names each certificate the appliance serves and when it ends.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread")]
async fn the_consoles_screens_share_a_live_stream_a_step_up_sheet_panels_sheets_and_undo() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    let lapse = 20;
    let p = prepare(&app, &[&mine], &[("step_up_seconds", lapse.into())]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let api = |path: &str| format!("{url}/api/v1{path}");

    // The app's own fonts, as its stylesheet asks for them.
    let r = b.get(format!("{url}/assets/fonts/inter-latin-wght-normal.woff2")).send().await.unwrap();
    assert_eq!((r.status(), r.headers()["content-type"].to_str().unwrap()), (StatusCode::OK, "font/woff2"));

    // The stream: refused without a session.
    let r = b.get(api("/events")).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // In the browser, every pattern.
    let more = [("MAGNUS_SHELL", "1".to_string()), ("MAGNUS_OTHER", "bob".to_string()), ("MAGNUS_STEP_UP_SECONDS", lapse.to_string())];
    let sid = sign_in_with(&url, c.addr, "Alice", PASSWORD, "admin", false, Some("#/overview"), &more).await.unwrap_or_else(|e| panic!("{e}"));

    // What the browser did is what happened: the switch is off again, Bob
    // is turned on, and each step is in the audit log.
    assert_eq!(app.api("alice").get("/v1/kill-switch").await.1["engaged"], false, "Release did not release the switch");
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    let csrf = me["csrf"].as_str().unwrap().to_string();
    let users: Value = with_cookie(b.get(api("/users")), &sid).send().await.unwrap().json().await.unwrap();
    let bob = users["users"].as_array().unwrap().iter().find(|u| u["id"] == "bob").cloned().unwrap_or_else(|| panic!("{users}"));
    assert_eq!(bob["status"], "active", "Undo did not turn Bob back on: {bob}");
    let audit: Value = with_cookie(b.get(api("/audit")), &sid).send().await.unwrap().json().await.unwrap();
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["kill_switch_engaged", "step_up", "kill_switch_released", "user_disabled", "user_enabled"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }
    assert!(!actions.contains(&"user_sessions_ended"), "the cancelled sheet ended Bob's sessions: {actions:?}");

    // The stream opens with the status: the kill switch, the cards.
    let mut r = with_cookie(b.get(api("/events")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
    let mut got = String::new();
    while !got.contains("\n\n") {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(10), r.chunk()).await.expect("no event in 10 seconds").unwrap().expect("the stream ended");
        got.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(got.starts_with("event: status\n"), "{got}");
    let data: Value = serde_json::from_str(got.lines().find_map(|l| l.strip_prefix("data: ")).unwrap()).unwrap();
    assert_eq!(data["kill_switch"]["engaged"], false, "{data}");
    assert!(data["running_tasks"].is_number() && data["kernel_up"] == true, "{data}");

    // Signed out, the open stream ends.
    let r2 = with_cookie(b.delete(api("/session")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r2.status(), StatusCode::NO_CONTENT);
    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match r.chunk().await {
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the stream outlived its session");

    // Overview names the console's certificate and when it ends.
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let o: Value = with_cookie(b.get(api("/overview")), &sid).send().await.unwrap().json().await.unwrap();
    let cert = o["certificates"].as_array().unwrap().iter().find(|x| x["what"] == "The admin console's certificate").cloned().unwrap_or_else(|| panic!("{o}"));
    assert!(cert["days_left"].as_i64().is_some_and(|d| d > 14), "{cert}");
}
