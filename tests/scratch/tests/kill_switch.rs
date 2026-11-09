//! The kill switch (B11, K53), one control for the owner: every action is
//! refused, every run is suspended, and the vault is sealed; each OAuth
//! sign-in is revoked at its provider and taken out of the vault, while API
//! keys and app secrets stay sealed (only their provider's console can revoke
//! them). Releasing it resumes nothing; the owner signs in again and resumes
//! each task. The same, as the owner sees it on Home (`desktop/tests/killswitch.mjs`).

use axum::{
    extract::State,
    routing::{get, post},
    Form, Json, Router,
};
use magnus_invariants::appliance::{bin, workspace_root};
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Seen = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// A provider that publishes its revocation endpoint in its discovery
/// document, as OpenAI's and xAI's do, and records what it is asked to revoke.
async fn oauth_provider() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let u = url.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || async move { Json(json!({ "issuer": u, "token_endpoint": format!("{u}/token"), "revocation_endpoint": format!("{u}/revoke") })) }),
        )
        .route("/token", post(|| async { Json(json!({ "access_token": "acc-2", "refresh_token": "ref-2", "expires_in": 3600 })) }))
        .route("/models", get(|| async { Json(json!({ "data": [{ "id": "oa-1" }] })) }))
        .route(
            "/revoke",
            post(|State(s): State<Seen>, Form(f): Form<HashMap<String, String>>| async move {
                s.lock().unwrap().push(f);
                ""
            }),
        )
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_kill_switch_revokes_sign_ins_seals_the_vault_and_releasing_resumes_nothing() {
    let (provider, revoked) = oauth_provider().await;
    let mail = mcp::mail().start().await;
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g) = (in_turn.clone(), go.clone());
    let m = FakeModel::start(script(move |c| {
        let (t, g) = (t.clone(), g.clone());
        async move {
            let send = tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" }));
            match c.turn() {
                0 => contract_unchecked("send a note", "note sent"),
                // Hold the model call open until the kill switch is engaged.
                1 => {
                    if !t.swap(true, Ordering::SeqCst) {
                        g.notified().await;
                    }
                    send
                }
                2 if c.last_results().iter().any(|r| r.is_error) => send,
                _ => complete("Sent."),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &m).await;
    let alice = app.api("alice");
    let signed_in = json!({ "access_token": "acc-1", "refresh_token": "ref-1", "expires_at": 4_102_444_800i64 }).to_string();
    assert!(alice.put_secret("oa_credential", &signed_in).await.is_success());
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let oa = json!({
        "id": "oa",
        "command": [bin("magnus-adapter-openai")],
        "settings": { "api": "chat", "base_url": provider, "credential": "oa_credential",
            "auth": { "kind": "oauth_device", "token_url": format!("{provider}/token"), "client_id": "magnus-test" } }
    });
    cfg["adapters"].as_array_mut().unwrap().push(toml::Value::try_from(oa).unwrap());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    let alice = app.api("alice");
    let carol = app.api("carol");
    let before: Vec<String> = serde_json::from_value(alice.get("/v1/secrets").await.1["names"].clone()).unwrap();
    assert!(before.contains(&"oa_credential".to_string()) && before.contains(&"mail_token".to_string()), "{before:?}");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());

    // One control, the owner's only.
    assert!(matches!(carol.post("/v1/kill-switch", json!({ "engaged": true })).await.0.as_u16(), 401 | 403));
    let id = alice.submit("Email bob a hello.").await;
    eventually("the task is in a model call", PATIENCE, || {
        let t = in_turn.clone();
        async move { t.load(Ordering::SeqCst).then_some(()) }
    })
    .await;
    let (st, v) = alice.post("/v1/kill-switch", json!({ "engaged": true })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(v["revoking"], json!(["oa_credential"]), "only the OAuth sign-in is revoked: {v}");
    assert!(v["sealed"].as_array().unwrap().contains(&json!("mail_token")), "{v}");
    go.notify_one();

    // Suspended, and the action it then asked for was refused.
    let t = alice.wait_status(&id, &["suspended"]).await;
    assert_eq!(t["waiting_on"], "kill_switch", "{t}");
    assert!(mail.calls_to("send").is_empty(), "an action ran under the kill switch");

    // The sign-in was revoked at the provider: the refresh token (which ends
    // the grant) and the access token, and it left the vault.
    let asked = eventually("the provider was asked to revoke the sign-in", PATIENCE, || {
        let r = revoked.clone();
        async move {
            let r = r.lock().unwrap().clone();
            (r.len() == 2).then_some(r)
        }
    })
    .await;
    let pairs: Vec<(String, String)> = asked.iter().map(|f| (f["token_type_hint"].clone(), f["token"].clone())).collect();
    assert_eq!(pairs, [("refresh_token".into(), "ref-1".into()), ("access_token".into(), "acc-1".into())]);
    assert!(asked.iter().all(|f| f["client_id"] == "magnus-test"));
    let ks = eventually("the revocation is recorded", PATIENCE, || {
        let alice = alice.clone();
        async move {
            let v = alice.get("/v1/kill-switch").await.1;
            v["revoked"]["oa_credential"]["at_provider"].is_boolean().then_some(v)
        }
    })
    .await;
    assert_eq!(ks["engaged"], true);
    assert_eq!(ks["revoked"]["oa_credential"]["at_provider"], true, "{ks}");
    assert_eq!(ks["revoked"]["oa_credential"]["adapter"], "oa", "{ks}");
    assert!(ks["sealed"].as_array().unwrap().contains(&json!("mail_token")), "{ks}");
    assert!(matches!(carol.get("/v1/kill-switch").await.0.as_u16(), 401 | 403), "someone else saw the vault's names");
    let names: Vec<String> = serde_json::from_value(alice.get("/v1/secrets").await.1["names"].clone()).unwrap();
    assert!(!names.contains(&"oa_credential".to_string()), "the revoked sign-in is still in the vault: {names:?}");
    // Nothing goes into the vault while it is sealed, even across a restart.
    assert_eq!(alice.put_secret("oa_credential", &signed_in).await.as_u16(), 409);
    app.restart().await;
    let alice = app.api("alice");
    assert_eq!(alice.get("/v1/kill-switch").await.1["engaged"], true);
    assert_eq!(alice.put_secret("oa_credential", &signed_in).await.as_u16(), 409, "a restart unsealed the vault");

    // Released: nothing resumes by itself (K53).
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
    never("a run resumed by itself after the kill switch was released", Duration::from_secs(4), || async {
        alice.task(&id).await["status"] != "suspended" || !mail.calls_to("send").is_empty()
    })
    .await;
    let ks = alice.get("/v1/kill-switch").await.1;
    assert_eq!(ks["revoked"]["oa_credential"]["at_provider"], true, "the owner is not told what to sign in to again: {ks}");
    // The owner signs in again, and resumes the task; the app secret was kept.
    assert!(alice.put_secret("oa_credential", &signed_in).await.is_success());
    assert!(alice.get("/v1/kill-switch").await.1["revoked"].as_object().unwrap().is_empty());
    assert!(alice.post(&format!("/v1/tasks/{id}/resume"), json!({})).await.0.is_success());
    eventually("the resumed task sends with the app's kept secret", PATIENCE, || async { (mail.effects("send") == 1).then_some(()) }).await;

    // The same control on Home, as the owner sees it.
    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/killswitch.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the kill switch on Home failed:\n{log}");
    let ks = alice.get("/v1/kill-switch").await.1;
    assert_eq!(ks["engaged"], false, "{ks}");
    assert_eq!(ks["revoked"]["oa_credential"]["at_provider"], true, "{ks}");
}

/// Releasing the switch and telling each person which of their tasks it
/// stopped are one write: no release is left untold by a crash between them.
#[tokio::test(flavor = "multi_thread")]
async fn releasing_the_switch_and_its_notices_are_one_write() {
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g) = (in_turn.clone(), go.clone());
    let m = FakeModel::start(script(move |c| {
        let (t, g) = (t.clone(), g.clone());
        async move {
            match c.turn() {
                0 => contract_unchecked("write a note", "note written"),
                // Hold the model call open until the kill switch is engaged.
                _ => {
                    if !t.swap(true, Ordering::SeqCst) {
                        g.notified().await;
                    }
                    complete("Written.")
                }
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let id = alice.submit("Write a note.").await;
    eventually("the task is in a model call", PATIENCE, || {
        let t = in_turn.clone();
        async move { t.load(Ordering::SeqCst).then_some(()) }
    })
    .await;
    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    go.notify_one();
    assert_eq!(alice.wait_status(&id, &["suspended"]).await["waiting_on"], "kill_switch");
    let (st, v) = alice.post("/v1/kill-switch", json!({ "engaged": false })).await;
    assert!(st.is_success() && v["told"] == 1, "{v}");

    let rows = app.db.admin().await.query("SELECT kind, at::text, body->>'item_kind' FROM public.events ORDER BY seq", &[]).await.unwrap();
    let rows: Vec<(String, String, Option<String>)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect();
    let release = rows.iter().rposition(|r| r.0 == "kill_switch").expect("the release is in the log");
    let next = rows.get(release + 1).expect("a notice after the release");
    assert_eq!((next.0.as_str(), next.2.as_deref()), ("item", Some("notice")), "{rows:?}");
    assert_eq!(next.1, rows[release].1, "the release and its notice were written apart: {rows:?}");
}

/// While the kill switch is on, new work is refused, not queued: a task, a
/// chat message, or a task made from a chat message is answered "the kill
/// switch is on" and nothing is created. Released, work starts again
/// (owner's ruling 11, 2026-11-01).
#[tokio::test(flavor = "multi_thread")]
async fn new_work_is_refused_while_the_kill_switch_is_on() {
    let m = FakeModel::start(script(|c| async move {
        if c.body["system"].to_string().contains("talking with the person you work for in a chat") {
            return Reply::Text("Noted.".into());
        }
        fail("nothing to do")
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api("alice");
    let (st, v) = alice.post("/v1/chat", json!({ "text": "Remember the agenda." })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    let entry = eventually("the message in the thread", PATIENCE, || async {
        let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
        c["entries"].as_array()?.iter().find(|e| e["kind"] == "message").and_then(|e| e["id"].as_str().map(String::from))
    })
    .await;
    let tasks = || async { alice.tasks().await["tasks"].as_array().unwrap().len() };
    let before = tasks().await;

    assert!(alice.post("/v1/kill-switch", json!({ "engaged": true })).await.0.is_success());
    for (what, (st, v)) in [
        ("a task", alice.post("/v1/tasks", json!({ "text": "Email bob a hello." })).await),
        ("a chat message", alice.post("/v1/chat", json!({ "text": "And another thing.", "thread": thread })).await),
        ("a task from a chat message", alice.post(&format!("/v1/chat/{thread}/entries/{entry}/task"), json!({})).await),
        ("a message sent again", alice.post(&format!("/v1/chat/{thread}/entries/{entry}/resend"), json!({})).await),
    ] {
        assert_eq!(st.as_u16(), 409, "{what} was accepted under the kill switch: {v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("kill switch is on"), "{what}: {v}");
    }
    assert_eq!(tasks().await, before, "work was created under the kill switch");
    // The message sent again under the switch is still in the thread.
    let (_, c) = alice.get(&format!("/v1/chat/{thread}")).await;
    assert!(c["entries"].as_array().unwrap().iter().any(|e| e["id"] == json!(entry)), "a refused resend hid the message: {c}");

    assert!(alice.post("/v1/kill-switch", json!({ "engaged": false })).await.0.is_success());
    let (st, v) = alice.post("/v1/tasks", json!({ "text": "Email bob a hello." })).await;
    assert_eq!(st.as_u16(), 201, "released, new work is still refused: {v}");
}

/// A page that keeps calling home while it is open, and how often each did.
async fn calling_pages() -> (String, Arc<Mutex<HashMap<String, usize>>>) {
    use axum::{extract::Path, response::Html};
    let calls: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/page/{name}",
            get(|Path(n): Path<String>| async move { Html(format!("<html><body><h1>Page {n}</h1><script>setInterval(() => fetch('/call/{n}'), 100)</script></body></html>")) }),
        )
        .route(
            "/call/{name}",
            get(|State(s): State<Arc<Mutex<HashMap<String, usize>>>>, Path(n): Path<String>| async move {
                *s.lock().unwrap().entry(n).or_default() += 1;
                ""
            }),
        )
        .with_state(calls.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, calls)
}

/// The kill switch stops what runs in the browser too: every run's pages
/// close, the running run's and the one already waiting on a question, so
/// no page's scripts go on after it (found reviewing the shared computer
/// spec, 2026-11-04: a suspended run kept its browser context open).
#[tokio::test(flavor = "multi_thread")]
async fn the_kill_switch_closes_every_runs_pages() {
    let (site, calls) = calling_pages().await;
    let in_turn = Arc::new(AtomicBool::new(false));
    let go = Arc::new(tokio::sync::Notify::new());
    let (t, g, s) = (in_turn.clone(), go.clone(), site.clone());
    let m = FakeModel::start(script(move |c| {
        let (t, g, s) = (t.clone(), g.clone(), s.clone());
        async move {
            let page = if c.body.to_string().contains("Watch page a.") { "a" } else { "b" };
            match c.turn() {
                0 => contract_unchecked("watch the page", "the page was watched"),
                1 => tool("browser_navigate", json!({ "url": format!("{s}/page/{page}") })),
                2 if c.last_results().iter().any(|r| r.is_error) => fail(&format!("the page did not open: {}", c.last_results()[0].text)),
                2 if page == "b" => tool("ask_principal", json!({ "question": "Keep watching?" })),
                // Held in a model call until the kill switch is engaged.
                2 => {
                    t.store(true, Ordering::SeqCst);
                    g.notified().await;
                    complete("Watched.")
                }
                _ => complete("Watched."),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    reconfigure(&mut app, |cfg| {
        // The test's pages are on this machine's loopback: an internal site the
        // owner allows (#185).
        let browser = json!({ "command": ["node", workspace_root().join("browser/sidecar.js")], "chromium": chromium(), "allow": ["127.0.0.1"] });
        cfg.insert("browser".into(), toml::Value::try_from(browser).unwrap());
    })
    .await;
    let alice = app.api("alice");
    let calling = |n: &'static str| {
        let calls = calls.clone();
        move || {
            let calls = calls.clone();
            async move { (calls.lock().unwrap().get(n).copied().unwrap_or(0) > 3).then_some(()) }
        }
    };

    // One run waits on a question with its page open; another is running.
    let waiting = alice.submit("Watch page b.").await;
    alice.wait_item(&waiting, "question").await;
    eventually("the waiting run's page calls home", PATIENCE, calling("b")).await;
    let running = alice.submit("Watch page a.").await;
    eventually("the running run is in a model call", PATIENCE, || {
        let t = in_turn.clone();
        async move { t.load(Ordering::SeqCst).then_some(()) }
    })
    .await;
    eventually("the running run's page calls home", PATIENCE, calling("a")).await;

    let (st, v) = alice.post("/v1/kill-switch", json!({ "engaged": true })).await;
    assert!(st.is_success(), "{v}");
    go.notify_one();
    let t = alice.wait_status(&running, &["suspended"]).await;
    assert_eq!(t["waiting_on"], "kill_switch", "{t}");
    // A call already on its way when the page closed may still arrive.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let at = calls.lock().unwrap().clone();
    never("a page went on running under the kill switch", Duration::from_secs(3), || {
        let (calls, at) = (calls.clone(), at.clone());
        async move { *calls.lock().unwrap() != at }
    })
    .await;
}
