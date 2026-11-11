//! Recovering a forgotten password by email (#533): asking always answers
//! the same, the holder gets a one-hour link, following it sets a new
//! password, ends every session, and clears the authenticator; a mismatch
//! or a short password re-renders without burning the token, and a spent
//! link, a bad token, or an unknown address leaves nothing usable. The
//! mailed link uses the configured base only (#541): with no trusted base
//! nothing goes out, a stale session cookie does not block the form, and
//! the answer never waits for the mail server.

use magnus_invariants::appliance::{bin, Appliance, Spec};
use magnus_invariants::*;
use magnus_scratch::mail;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::{json, Value};

const DAVE: &str = "dave@example.com";
const OLD: &str = "dave-old-password";
const NEW: &str = "dave-new-password";
const LINK_BASE: &str = "https://recovery.example";

fn cli(app: &Appliance, args: &[&str]) -> std::process::Output {
    std::process::Command::new(bin("magnus-appliance")).args(args).arg("--config").arg(app.dir.path().join("magnus.toml")).output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forgotten_password_is_recovered_by_email() {
    let sink = mail::Inbox::start_local().await;
    let m = FakeModel::start(steps(vec![])).await;
    let app =
        Appliance::start(Spec::default().mail(json!({ "host": "127.0.0.1", "port": sink.addr.port(), "tls": "none", "from": "magnus@example.com", "link_base": LINK_BASE })), &m)
            .await;
    let out = cli(&app, &["person-add", "--person", "dave", "--name", "Dave", "--email", DAVE]);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    assert!(set_password(&app, "dave", OLD), "magnus password for dave");
    let out = cli(&app, &["totp-enroll", "--person", "dave"]);
    assert!(out.status.success(), "magnus totp-enroll: {}", String::from_utf8_lossy(&out.stderr));
    let uri = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).expect("no secret in the enroll uri").to_string();
    let raw = magnus::totp::base32_decode(&secret).unwrap();
    let code = || magnus::totp::generate(&raw, (chrono::Utc::now().timestamp().div_euclid(30)) as u64, 6);
    let c = reqwest::Client::new();
    let sign_in = async |password: &str, code: Option<String>| {
        let mut body = json!({ "person": "dave", "password": password, "client": "app" });
        if let Some(code) = code {
            body["code"] = json!(code);
        }
        c.post(format!("{}/v1/sign-in", app.base)).header("x-magnus-app", "0.5.0").json(&body).send().await.unwrap()
    };
    // The password alone is not enough while codes are enrolled; with the
    // code, a session to prove the recovery ends.
    let r = sign_in(OLD, None).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(r.json::<Value>().await.unwrap()["code_needed"], true, "no code was asked");
    let r = sign_in(OLD, Some(code())).await;
    assert_eq!(r.status(), StatusCode::OK);
    let session = r.json::<Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    // Asking answers alike for the holder and for nobody; only the holder
    // gets mail, with a link back here.
    for email in [DAVE, "nobody@example.com"] {
        let r = c.post(format!("{}/v1/password-reset/request", app.base)).json(&json!({ "email": email })).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::OK, "asking for {email} answered wrongly");
        assert_eq!(r.json::<Value>().await.unwrap()["ok"], true);
    }
    assert_eq!(sink.count_for("nobody@example.com"), 0, "mail went to nobody");
    let mail = sink.wait_for(DAVE).await;
    let link = mail::text_of(&mail.data).split_whitespace().find(|w| w.contains("/reset?token=")).expect("the mail holds no link").to_string();
    assert!(link.starts_with(LINK_BASE), "the link ignores the configured base: {link}");
    let reset = link.split("token=").nth(1).unwrap_or_default().trim().to_string();
    assert!(!reset.is_empty(), "the link holds no token");
    // The configured base is where holders are sent; the test follows the
    // same token here.
    let page = format!("{}/reset?token={reset}", app.base);

    // The page shows the form; a mismatch and a short password re-render
    // without burning the token.
    let r = c.get(&page).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.text().await.unwrap().contains("Choose a new password"), "no form behind the link");
    let r = c.post(format!("{}/reset", app.base)).form(&[("token", reset.as_str()), ("password", NEW), ("confirm", "something else")]).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("do not match"), "a mismatch was taken");
    let r = c.post(format!("{}/reset", app.base)).form(&[("token", reset.as_str()), ("password", "short"), ("confirm", "short")]).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("at least"), "a short password was taken");

    // Confirming sets the password, ends the old session, and clears the
    // authenticator: the new password signs in with no code. The browser
    // still holds its old session cookie, which must not block the
    // token-authorized form (#541).
    let r = c
        .post(format!("{}/reset", app.base))
        .header("cookie", "magnus_session=stale")
        .form(&[("token", reset.as_str()), ("password", NEW), ("confirm", NEW)])
        .send()
        .await
        .unwrap();
    let set = r.text().await.unwrap();
    assert!(set.contains("new password is set"), "confirming did not set it");
    assert!(set.contains("<b>dave</b>"), "the page does not name the username: {set}");
    let r = sign_in(NEW, None).await;
    assert_eq!(r.status(), StatusCode::OK, "the new password does not sign in without a code");
    // The address the recovery ran on signs in as well as the username,
    // whatever its case: the mail and the page offer both.
    assert!(mail::text_of(&mail.data).contains("your username, dave."), "the mail does not name the username");
    let by_email = json!({ "person": " Dave@Example.com ", "password": NEW, "client": "app" });
    let r = c.post(format!("{}/v1/sign-in", app.base)).header("x-magnus-app", "0.5.0").json(&by_email).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "the email address does not sign in");
    let by_wrong = json!({ "person": DAVE, "password": OLD, "client": "app" });
    let r = c.post(format!("{}/v1/sign-in", app.base)).header("x-magnus-app", "0.5.0").json(&by_wrong).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the email address signed in with the old password");
    let r = sign_in(OLD, None).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the old password still signs in");
    let r = c.get(format!("{}/v1/me", app.base)).bearer_auth(&session).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the old session survived the recovery");

    // Spent: the link and the token answer alike now.
    let r = c.get(&page).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("spent or expired"), "the spent link still shows the form");
    let r = c.post(format!("{}/reset", app.base)).form(&[("token", reset.as_str()), ("password", "dave-newer-password"), ("confirm", "dave-newer-password")]).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("spent or expired"), "the spent token confirmed again");
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_sends_nothing_without_a_trusted_base() {
    let sink = mail::Inbox::start_local().await;
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default().mail(json!({ "host": "127.0.0.1", "port": sink.addr.port(), "tls": "none", "from": "magnus@example.com" })), &m).await;
    let out = cli(&app, &["person-add", "--person", "dave", "--name", "Dave", "--email", DAVE]);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    assert!(set_password(&app, "dave", OLD), "magnus password for dave");
    // No link base and no pairing hostname: the answer stays kind, and
    // with no trusted base the request's own host must not stand in, so
    // no mail goes out (#541).
    let c = reqwest::Client::new();
    let r = c.post(format!("{}/v1/password-reset/request", app.base)).json(&json!({ "email": DAVE })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "asking answered wrongly");
    assert_eq!(r.json::<Value>().await.unwrap()["ok"], true);
    // Delivery runs behind the response: give a wrongly-sent mail time to land.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(sink.connections(), 0, "the mail server was reached with no trusted base");
    assert_eq!(sink.count_for(DAVE), 0, "mail went out with no trusted base");
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_answers_before_the_mail_server_does() {
    // A mail server that takes the connection and never speaks: delivery
    // blocks on its greeting, so only an answer sent ahead of delivery
    // comes back in time (#541).
    let hole = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hole_port = hole.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = hole.accept().await else { break };
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                drop(socket);
            });
        }
    });
    let m = FakeModel::start(steps(vec![])).await;
    let app =
        Appliance::start(Spec::default().mail(json!({ "host": "127.0.0.1", "port": hole_port, "tls": "none", "from": "magnus@example.com", "link_base": LINK_BASE })), &m).await;
    let out = cli(&app, &["person-add", "--person", "dave", "--name", "Dave", "--email", DAVE]);
    assert!(out.status.success(), "magnus person-add: {}", String::from_utf8_lossy(&out.stderr));
    assert!(set_password(&app, "dave", OLD), "magnus password for dave");
    let c = reqwest::Client::new();
    let r = tokio::time::timeout(std::time::Duration::from_secs(15), c.post(format!("{}/v1/password-reset/request", app.base)).json(&json!({ "email": DAVE })).send())
        .await
        .expect("the answer waited for the mail server")
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK, "asking answered wrongly");
    assert_eq!(r.json::<Value>().await.unwrap()["ok"], true);
}
