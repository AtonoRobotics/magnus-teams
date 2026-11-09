//! The appliance's own directory (#429): with Keycloak live, a person
//! added with `magnus person-add` signs in to the API with a local
//! password, accepts an invite, and stops signing in once removed; people
//! the config knows still go through Keycloak.

use magnus_invariants::appliance::{bin, Appliance, Spec};
use magnus_invariants::*;
use magnus_scratch::Keycloak;
use serde_json::{json, Value};
use std::io::Write;

fn cli(app: &Appliance, args: &[&str], stdin: Option<&str>) -> std::process::Output {
    let mut c = std::process::Command::new(bin("magnus-appliance"));
    c.args(args).arg("--config").arg(app.dir.path().join("magnus.toml"));
    if stdin.is_some() {
        c.stdin(std::process::Stdio::piped());
    }
    let mut c = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    if let Some(text) = stdin {
        writeln!(c.stdin.take().unwrap(), "{text}").unwrap();
    }
    c.wait_with_output().unwrap()
}

fn person_add(app: &Appliance, args: &[&str]) -> std::process::Output {
    let mut full = vec!["person-add"];
    full.extend(args.iter());
    cli(app, &full, None)
}

fn password(app: &Appliance, person: &str, pw: &str) {
    let out = cli(app, &["password", "--person", person], Some(pw));
    assert!(out.status.success(), "magnus password: {}", String::from_utf8_lossy(&out.stderr));
}

fn invite(app: &Appliance, person: &str) -> String {
    let out = cli(app, &["invite", "--person", person], None);
    assert!(out.status.success(), "magnus invite: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

async fn sign_in(app: &Appliance, person: &str, pw: &str) -> (reqwest::StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": person, "password": pw, "client": "app" }))
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_local_person_signs_in_while_keycloak_is_live() {
    let model = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &model).await;
    let kc = Keycloak::start().await;
    kc.add_person("Carol").await;
    kc.configure(&mut app).await;

    // An id the config has is refused: it would never resolve to them.
    let out = person_add(&app, &["--person", "carol"]);
    assert!(!out.status.success(), "a local namesake of a config person was added");
    // In any casing: ids canonicalize to lowercase, so CAROL would land on
    // carol and shadow her (#470).
    let out = person_add(&app, &["--person", "CAROL"]);
    assert!(!out.status.success(), "a differently-cased namesake of a config person was added");

    // Ada is unknown to Keycloak; the appliance itself keeps her.
    let out = person_add(&app, &["--person", "ada", "--name", "Ada"]);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    password(&app, "ada", "ada-long-password");

    // Her password signs her in straight from the local store, past the
    // live provider; the session resolves her as a plain member.
    let (st, v) = sign_in(&app, "ada", "ada-long-password").await;
    assert_eq!(st, 200, "{v}");
    let ada_session = v["token"].as_str().expect("an app sign-in returns a token").to_string();
    let mut me = app.api("carol");
    me.token = ada_session.clone();
    let (st, who) = me.get("/v1/me").await;
    assert_eq!((st.as_u16(), who["id"].as_str()), (200, Some("ada")), "the local session did not resolve: {who}");

    // A wrong password refuses like an unknown name.
    let (st, v) = sign_in(&app, "ada", "wrong-password").await;
    assert_eq!((st.as_u16(), v["error"].as_str()), (401, Some("The name or password is wrong.")), "{v}");

    // Carol still goes through Keycloak: her name never reaches the
    // local store, and Keycloak's own flow signs her in.
    let (st, v) = sign_in(&app, "carol", "ada-long-password").await;
    assert_eq!(st, 401, "a config person reached the local store: {v}");
    let (st, v) = kc.app_sign_in(&app, "Carol").await;
    assert_eq!(st, 200, "Keycloak's flow broke: {v}");

    // An invite for a local person accepts at the API, and she signs in.
    let out = person_add(&app, &["--person", "bea"]);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    let bea_invite = invite(&app, "bea");
    let r = reqwest::Client::new().post(format!("{}/v1/invites/accept", app.base)).json(&json!({ "token": bea_invite, "password": "bea-long-password" })).send().await.unwrap();
    assert_eq!(r.status(), 200, "a local invite did not accept: {}", r.text().await.unwrap());
    let (st, v) = sign_in(&app, "bea", "bea-long-password").await;
    assert_eq!(st, 200, "{v}");

    // Removing Ada ends her session and shuts her out.
    let out = cli(&app, &["person-remove", "--person", "ada"], None);
    assert!(out.status.success(), "magnus person-remove: {}", String::from_utf8_lossy(&out.stderr));
    let (st, _) = sign_in(&app, "ada", "ada-long-password").await;
    assert_eq!(st, 401, "a removed person still signs in");
    me.token = ada_session.clone();
    let (st, _) = me.get("/v1/me").await;
    assert_eq!(st.as_u16(), 401, "a removed person's session still resolves");
}
