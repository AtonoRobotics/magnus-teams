//! Signing in to the admin console against the local store (#429): no
//! provider involved. An active admin with a password (and code where
//! enrolled) signs in at `/auth/local`; a member, a wrong password and an
//! unknown name refuse alike with no session; changes still need the CSRF
//! token; step-up confirms with the password; disabling ends the session
//! at the next check.

use magnus_invariants::appliance::{bin, Appliance, Spec};
use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::Value;
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

fn password(app: &Appliance, person: &str, pw: &str) {
    let out = cli(app, &["password", "--person", person], Some(pw));
    assert!(out.status.success(), "magnus password: {}", String::from_utf8_lossy(&out.stderr));
}

fn enroll(app: &Appliance, person: &str) -> String {
    let out = cli(app, &["totp-enroll", "--person", person], None);
    assert!(out.status.success(), "magnus totp-enroll: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn role(app: &Appliance, person: &str, role: &str) {
    let out = cli(app, &["person-role", "--person", person, "--role", role], None);
    assert!(out.status.success(), "magnus person-role: {}", String::from_utf8_lossy(&out.stderr));
}

fn status(app: &Appliance, person: &str, status: &str) {
    let out = cli(app, &["person-status", "--person", person, "--status", status], None);
    assert!(out.status.success(), "magnus person-status: {}", String::from_utf8_lossy(&out.stderr));
}

fn code(uri: &str) -> String {
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).unwrap();
    let raw = magnus::totp::base32_decode(secret).unwrap();
    magnus::totp::generate(&raw, (chrono::Utc::now().timestamp().div_euclid(30)) as u64, 6)
}

/// One cookie of a response, by name, if it set a non-empty one.
fn cookie_of(r: &reqwest::Response, name: &str) -> Option<String> {
    r.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next()?.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

/// The console session cookie of a response, if it set one.
fn sid_of(r: &reqwest::Response) -> Option<String> {
    cookie_of(r, "__Host-magnus_admin")
}

/// A local form's CSRF binding: its cookie and its hidden field. A
/// cross-origin attacker reads neither (#456).
async fn form_at(b: &reqwest::Client, url: &str, path: &str, sid: Option<&str>) -> (String, String, String) {
    let mut req = b.get(format!("{url}{path}"));
    if let Some(sid) = sid {
        req = with_cookie(req, sid);
    }
    let r = req.send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let raw = r
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("__Host-magnus_local="))
        .expect("the form set no login cookie")
        .to_string();
    // Browsers enforce the `__Host-` contract; a narrower path or a
    // missing flag silently drops the cookie there (#456).
    let attrs: Vec<String> = raw.split(';').map(|p| p.trim().to_string()).collect();
    for attr in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
        assert!(attrs.iter().any(|p| p == attr), "the login cookie lost {attr}: {raw}");
    }
    let cookie = cookie_of(&r, "__Host-magnus_local").expect("the form set no login cookie");
    let body = r.text().await.unwrap();
    let token = body.split("name=login_token value=\"").nth(1).and_then(|s| s.split('"').next()).expect("the form carries no login token").to_string();
    assert!(!token.is_empty(), "the login token is empty");
    (cookie, token, body)
}

/// Submit a local form the way the browser does: fetch it for its login
/// cookie and token, then post with both.
async fn submit(b: &reqwest::Client, url: &str, path: &str, sid: Option<&str>, fields: Vec<(String, String)>) -> reqwest::Response {
    let (cookie, token, _) = form_at(b, url, path, sid).await;
    let mut req = b.post(format!("{url}{path}")).header("cookie", format!("__Host-magnus_local={cookie}"));
    if let Some(sid) = sid {
        req = with_cookie(req, sid);
    }
    let mut fields = fields;
    fields.push(("login_token".to_string(), token));
    req.form(&fields).send().await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_signs_in_locally_and_a_member_and_a_wrong_password_do_not() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    password(&app, "alice", "alice-long-password");
    role(&app, "alice", "admin");
    password(&app, "bob", "bob-long-password");
    role(&app, "bob", "admin");
    password(&app, "carol", "carol-long-password");
    let mine = format!("{}/32", lan_ip());
    let p = prepare(&app, &[&mine], &[("refresh_seconds", 2.into()), ("step_up_seconds", 90.into())]);
    let url = p.url();
    let c = launch(p).await;
    let api = |path: &str| format!("{url}/api/v1{path}");
    let b = browser(&c, false);
    // No hand-crafted security headers anywhere here: the test behaves
    // like the browser, which under the console's no-referrer policy sends
    // `Origin: null` (#456).
    let login = |who: &str, pw: &str, extra: Vec<(String, String)>| {
        let mut fields = vec![("person".to_string(), who.to_string()), ("password".to_string(), pw.to_string()), ("then".to_string(), "/".to_string())];
        fields.extend(extra);
        submit(&b, &url, "/auth/local", None, fields)
    };

    // The form is there, bound to its page by cookie and token.
    let (_, _, body) = form_at(&b, &url, "/auth/local", None).await;
    assert!(body.contains("<form") && body.contains("name=password"), "no local sign-in form: {body}");

    // A post without the binding, however right its credentials, refuses:
    // that is all a cross-origin attacker can send (#456).
    let r = b.post(format!("{url}/auth/local")).form(&[("person", "alice"), ("password", "alice-long-password"), ("then", "/")]).send().await.unwrap();
    assert!(sid_of(&r).is_none(), "an unbound post signed in");
    let r = b
        .post(format!("{url}/auth/local"))
        .header("cookie", "__Host-magnus_local=forged")
        .form(&[("person", "alice"), ("password", "alice-long-password"), ("login_token", "forged"), ("then", "/")])
        .send()
        .await
        .unwrap();
    assert!(sid_of(&r).is_none(), "a forged binding signed in");

    // A wrong password, an unknown name and a member refuse alike, with no session.
    for (who, pw) in [("alice", "wrong-password"), ("nobody", "whatever-password"), ("carol", "carol-long-password")] {
        let r = login(who, pw, vec![]).await;
        assert_eq!(r.status(), StatusCode::OK, "{who} was not refused");
        assert!(sid_of(&r).is_none(), "{who} was given a console session");
        let body = r.text().await.unwrap();
        assert!(body.contains("wrong"), "{who} got no alike refusal: {body}");
    }

    // The admin signs in with the password; she is stepped up from it.
    let r = login("alice", "alice-long-password", vec![]).await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER, "the password did not sign in");
    let sid = sid_of(&r).expect("no session cookie on local sign-in");
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    assert!(me["csrf"].as_str().is_some_and(|t| t.len() >= 20) && me["stepped_up_until"].is_string(), "{me}");
    let csrf = me["csrf"].as_str().unwrap().to_string();

    // A change without the session's token is refused; with it, done.
    let r = with_cookie(b.post(api("/session/step-up")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "a change without the CSRF token was taken");
    let r = with_cookie(b.post(api("/session/step-up")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT, "a fresh step-up did not count");

    // A step-up is the local form, which confirms with the password and
    // comes back.
    let r = with_cookie(b.get(format!("{url}/auth/local/step-up?then=/auth/done")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "the step-up form did not answer");
    let r = submit(&b, &url, "/auth/local/step-up", Some(&sid), vec![("password".to_string(), "alice-long-password".to_string()), ("then".to_string(), "/auth/done".to_string())])
        .await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER, "the password did not confirm the step-up");

    // Disabling ends the session at the next check, within seconds.
    status(&app, "alice", "disabled");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let r = with_cookie(b.get(api("/session")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the disabled admin kept the console");
    status(&app, "alice", "active");

    // Enrolling the code asks for it from then on: the password alone
    // shows the code screen, and with the code she is in again.
    // (Enrolled last so no code is ever used twice in this test.)
    let uri = enroll(&app, "alice");
    let r = login("alice", "alice-long-password", vec![]).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(sid_of(&r).is_none(), "the password alone opened a session past the code");
    assert!(r.text().await.unwrap().contains("6-digit code"), "no code screen");
    let r = login("alice", "alice-long-password", vec![("code".to_string(), code(&uri))]).await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER, "the code did not sign in");
    let sid = sid_of(&r).expect("no session cookie past the code");

    // Signing out ends the session.
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    let csrf = me["csrf"].as_str().unwrap().to_string();
    let r = with_cookie(b.delete(api("/session")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(with_cookie(b.get(api("/session")), &sid).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
}
