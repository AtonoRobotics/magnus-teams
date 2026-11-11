//! A person's own password (owner, 2026-11-10): changing it in Settings with
//! the current one, which counts a wrong current password as sign-in does,
//! refuses a short new one, keeps the session that asked and ends the
//! others, and is recorded; recovery links point at `[appliance]
//! public_url`; and `magnus password` on the server's own machine ends every
//! session it replaces the password of.
#![allow(clippy::unwrap_used)] // a test file: its helpers are tests too

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use magnus_scratch::mail;
use serde_json::{json, Value};
use std::io::Write;

const OLD: &str = "carol-old-password";
const NEW: &str = "carol-new-password";

fn set_password(app: &Appliance, person: &str, pw: &str) {
    let mut c = std::process::Command::new(bin("magnus-appliance"))
        .args(["password", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--person", person])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    writeln!(c.stdin.take().unwrap(), "{pw}").unwrap();
    assert!(c.wait().unwrap().success());
}

async fn signed_in(app: &Appliance, person: &str, pw: &str) -> Api {
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": person, "password": pw, "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{person} did not sign in");
    let body: Value = r.json().await.unwrap();
    let mut a = app.api(person);
    a.token = body["token"].as_str().unwrap().to_string();
    a
}

fn change(current: &str, new: &str) -> Value {
    json!({ "current_password": current, "new_password": new })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_is_changed_in_settings_with_the_current_one() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    set_password(&app, "carol", OLD);
    let laptop = signed_in(&app, "carol", OLD).await;
    let phone = signed_in(&app, "carol", OLD).await;

    let (s, rules) = laptop.get("/v1/account/password").await;
    assert_eq!(s, 200, "{rules}");
    assert_eq!(rules["here"], true, "{rules}");
    let least = rules["min_length"].as_u64().unwrap() as usize;

    // Short: refused before the current password is looked at.
    let (s, v) = laptop.post("/v1/account/password", change(OLD, &"x".repeat(least - 1))).await;
    assert_eq!(s, 400, "{v}");
    assert!(v["error"].as_str().unwrap().contains(&format!("at least {least}")), "{v}");

    // Wrong current: refused without ending the session (not a 401), and
    // counted with sign-in's wrong passwords until the lock.
    let (s, v) = laptop.post("/v1/account/password", change("not-carols-password", NEW)).await;
    assert_eq!(s, 403, "{v}");
    assert_eq!(v["error"], "The current password is wrong.");
    let mut locked = false;
    for _ in 0..10 {
        let (s, _) = laptop.post("/v1/account/password", change("not-carols-password", NEW)).await;
        if s == 429 {
            locked = true;
            break;
        }
    }
    assert!(locked, "wrong current passwords were never counted to a lock");
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": "carol", "password": OLD, "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429, "sign-in was not locked by the same wrong passwords");
    assert_eq!(laptop.get("/v1/me").await.0, 200, "a refused change ended the session");
    assert_eq!(phone.get("/v1/me").await.0, 200, "a refused change ended another session");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_keeps_this_device_and_signs_out_the_others() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    set_password(&app, "carol", OLD);
    let laptop = signed_in(&app, "carol", OLD).await;
    let phone = signed_in(&app, "carol", OLD).await;
    let (s, v) = laptop.post("/v1/account/password", change(OLD, NEW)).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["signed_out"], 1, "{v}");
    assert_eq!(laptop.get("/v1/me").await.0, 200, "the device that changed the password was signed out");
    assert_eq!(phone.get("/v1/me").await.0, 401, "another device stayed signed in");
    signed_in(&app, "carol", NEW).await;
    let admin = app.db.admin().await;
    let how: String = admin
        .query_one("SELECT body->>'how' FROM public.events WHERE kind = 'password_changed' AND body->>'person' = 'carol'", &[])
        .await
        .expect("the change is not in the log")
        .get(0);
    assert_eq!(how, "settings");
    let logged = admin.query_one("SELECT count(*) FROM public.events WHERE body::text LIKE $1", &[&format!("%{NEW}%")]).await.unwrap().get::<_, i64>(0);
    assert_eq!(logged, 0, "the new password is in the log");

    // The owner's way back in, on the server's own machine: every session ends.
    set_password(&app, "carol", OLD);
    assert_eq!(laptop.get("/v1/me").await.0, 401, "magnus password left a session signed in");
}

/// With `[appliance] public_url` and no mail base, the recovery link points
/// at the public address; an unknown address answers the same and gets
/// nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_recovery_link_points_at_the_public_address() {
    let sink = mail::Inbox::start_local().await;
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default().mail(json!({ "host": "127.0.0.1", "port": sink.addr.port(), "tls": "none", "from": "magnus@example.com" })), &m).await;
    let path = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    cfg["appliance"].as_table_mut().unwrap().insert("public_url".into(), "https://teams.example".into());
    std::fs::write(&path, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["person-add", "--person", "dave", "--name", "Dave", "--email", "dave@example.com", "--config"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    set_password(&app, "dave", OLD);
    let c = reqwest::Client::new();
    let mut answers = vec![];
    for email in ["dave@example.com", "nobody@example.com"] {
        let r = c.post(format!("{}/v1/password-reset/request", app.base)).json(&json!({ "email": email })).send().await.unwrap();
        answers.push((r.status(), r.text().await.unwrap()));
    }
    assert_eq!(answers[0], answers[1], "a known and an unknown address were answered differently");
    let got = sink.wait_for("dave@example.com").await;
    let link = mail::text_of(&got.data).split_whitespace().find(|w| w.contains("/reset?token=")).expect("the mail holds no link").to_string();
    assert!(link.starts_with("https://teams.example/reset?token="), "the link ignores the public address: {link}");
    assert_eq!(sink.count_for("nobody@example.com"), 0, "mail went to nobody");
}
