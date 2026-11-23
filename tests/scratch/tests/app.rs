//! What the desktop app relies on from the kernel: sign-in (by password and
//! through a real Keycloak) and sessions,
//! inbox flags, plans, composed outcomes, the playbook, versions and the
//! developer view. Behavior through the HTTP API only.

use magnus_invariants::appliance::{bin, Person};
use magnus_invariants::{mcp, *};
use magnus_scratch::{code_for, loopback_once, password_of, server_url, sign_in_code, Keycloak, IDP_LABEL};
use serde_json::{json, Value};
use std::io::Write;

const PASSWORD: &str = "correct horse battery";

fn spec() -> Spec {
    let mut s = Spec::default();
    s.people.push(Person { id: "dana".into(), name: "Dana".into(), token: format!("tok-dana-{}", uuid()), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    s
}

fn uuid() -> String {
    canary("x").rsplit('-').next().unwrap().to_string()
}

fn set_password(app: &Appliance, person: &str, pw: &str) -> std::process::Output {
    let mut c = std::process::Command::new(bin("magnus-appliance"))
        .args(["password", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--person", person])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(c.stdin.take().unwrap(), "{pw}").unwrap();
    c.wait_with_output().unwrap()
}

/// An API client acting with a session token instead of a config token.
fn with_token(app: &Appliance, token: &str) -> Api {
    let mut a = app.api("carol");
    a.token = token.to_string();
    a
}

async fn sign_in(app: &Appliance, person: &str, pw: &str) -> (reqwest::StatusCode, Value) {
    sign_in_with(app, person, pw, None).await
}

/// With the owner's backup, the code `magnus sign-in-code` printed.
async fn sign_in_with(app: &Appliance, person: &str, pw: &str, machine_code: Option<&str>) -> (reqwest::StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": person, "password": pw, "machine_code": machine_code, "client": "app" }))
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_in_opens_a_session_that_never_reaches_the_log() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;

    let short = set_password(&app, "carol", "short");
    assert!(!short.status.success(), "a short password was accepted");
    assert!(set_password(&app, "carol", PASSWORD).status.success());

    let (st, _) = sign_in(&app, "carol", "wrong password here").await;
    assert_eq!(st, 401);
    let (st, unknown) = sign_in(&app, "nobody", PASSWORD).await;
    assert_eq!(st, 401);
    assert_eq!(unknown["error"], "The name or password is wrong.", "an unknown name must look like a wrong password");

    let (st, v) = sign_in(&app, "Carol", PASSWORD).await;
    assert_eq!(st, 200, "{v}");
    let token = v["token"].as_str().expect("an app sign-in returns a token").to_string();
    let carol = with_token(&app, &token);
    let (st, me) = carol.get("/v1/me").await;
    assert_eq!((st.as_u16(), me["id"].as_str()), (200, Some("carol")));

    // The token is a credential: not in the log, not in the database.
    let dump = app.db.dump().await;
    assert!(!dump.contains(&token), "the session token reached the database");
    assert!(dump.contains("signed_in"), "the sign-in is not recorded");

    let (st, _) = carol.post("/v1/sign-out", json!({})).await;
    assert_eq!(st, 200);
    let (st, _) = carol.get("/v1/me").await;
    assert_eq!(st, 401, "a signed-out token still works");
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_wrong_passwords_lock_the_name() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;
    assert!(set_password(&app, "carol", PASSWORD).status.success());
    for _ in 0..5 {
        assert_eq!(sign_in(&app, "carol", "wrong password here").await.0, 401);
    }
    let (st, v) = sign_in(&app, "carol", PASSWORD).await;
    assert_eq!(st, 429, "the right password got through a lockout: {v}");
    assert!(v["retry_at"].is_string());
}

/// Wrong passwords sent from elsewhere on the network lock the name there,
/// but not on the appliance's own machine: someone on the network cannot
/// lock the owner out of the appliance itself.
#[tokio::test(flavor = "multi_thread")]
async fn wrong_passwords_from_the_network_do_not_lock_the_appliances_own_machine() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;
    assert!(set_password(&app, "carol", PASSWORD).status.success());
    // Through a proxy on the appliance, as from another computer.
    let from_elsewhere = |pw: &'static str| {
        let base = app.base.clone();
        async move {
            let r = reqwest::Client::new()
                .post(format!("{base}/v1/sign-in"))
                .header("x-magnus-app", "0.5.0")
                .header("x-forwarded-for", "192.168.1.66")
                .json(&json!({ "person": "carol", "password": pw, "client": "app" }))
                .send()
                .await
                .unwrap();
            r.status()
        }
    };
    for _ in 0..5 {
        assert_eq!(from_elsewhere("wrong password here").await, 401);
    }
    assert_eq!(from_elsewhere(PASSWORD).await, 429, "the right password got through a lockout from the network");
    let (st, v) = sign_in(&app, "carol", PASSWORD).await;
    assert_eq!(st, 200, "the network locked the name out on the appliance's own machine: {v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_session_needs_the_client_header_to_change_anything() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;
    assert!(set_password(&app, "carol", PASSWORD).status.success());
    let http = reqwest::Client::new();
    let r = http.post(format!("{}/v1/sign-in", app.base)).json(&json!({ "person": "carol", "password": PASSWORD })).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let cookie = r.headers().get("set-cookie").expect("a browser sign-in sets a cookie").to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"), "{cookie}");
    let body: Value = r.json().await.unwrap();
    assert!(body.get("token").is_none(), "a browser sign-in must not hand the page the token");
    let pair = cookie.split(';').next().unwrap().to_string();

    let me = http.get(format!("{}/v1/me", app.base)).header("cookie", &pair).send().await.unwrap();
    assert_eq!(me.status(), 200);
    let forged = http.post(format!("{}/v1/tasks", app.base)).header("cookie", &pair).json(&json!({ "text": "hi" })).send().await.unwrap();
    assert_eq!(forged.status(), 403, "a cookie alone changed something");
}

/// Start a sign-in as the appliance's own page in a browser does.
async fn device_start(app: &Appliance) -> Value {
    let r = reqwest::Client::new().post(format!("{}/v1/sign-in/device", app.base)).json(&json!({ "client": "browser" })).send().await.unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap_or_default());
    r.json().await.unwrap()
}

/// Poll a device sign-in the way the page does, until it is no longer pending.
async fn device_finish(app: &Appliance, flow: &str) -> (u16, Value) {
    let end = std::time::Instant::now() + PATIENCE;
    loop {
        let r = reqwest::Client::new().post(format!("{}/v1/sign-in/device/{flow}", app.base)).send().await.unwrap();
        let st = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if st != 202 || std::time::Instant::now() > end {
            return (st, v);
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// Something shaped like a JWT (a Keycloak access, refresh or ID token).
fn has_jwt(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')).any(|w| w.starts_with("eyJ") && w.matches('.').count() == 2)
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_in_through_keycloak_keeps_no_provider_token_and_only_the_owner_keeps_a_password_as_backup() {
    let model = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(spec(), &model).await;
    let kc = Keycloak::start().await;
    kc.add_person("Carol").await;
    // Carol had a password on the appliance before it used Keycloak.
    assert!(set_password(&app, "carol", PASSWORD).status.success());
    kc.configure(&mut app).await;

    let (_, m) = app.api("carol").get("/v1/sign-in").await;
    assert_eq!((m["method"].as_str(), m["label"].as_str(), m["password"].as_bool()), (Some("oidc"), Some(IDP_LABEL), Some(true)), "{m}");

    // The app's way: the person's browser, back to a loopback address.
    let (st, v) = kc.app_sign_in(&app, "Carol").await;
    assert_eq!(st, 200, "{v}");
    let token = v["token"].as_str().expect("an app sign-in returns a token").to_string();
    let (st, me) = with_token(&app, &token).get("/v1/me").await;
    assert_eq!((st.as_u16(), me["id"].as_str()), (200, Some("carol")), "the provider's name maps to the person, whatever its case");

    // The page's way: a code approved on Keycloak's page.
    let s = device_start(&app).await;
    let flow = s["flow"].as_str().unwrap().to_string();
    let r = reqwest::Client::new().post(format!("{}/v1/sign-in/device/{flow}", app.base)).send().await.unwrap();
    assert_eq!(r.status(), 202, "a sign-in nobody approved went through");
    kc.browser(s["verification_uri_complete"].as_str().unwrap(), "carol").await.unwrap();
    let (st, v) = device_finish(&app, &flow).await;
    assert_eq!((st, v["person"]["id"].as_str()), (200, Some("carol")), "{v}");
    let (st, _) = device_finish(&app, &flow).await;
    assert_eq!(st, 410, "one approval opened a second session");

    let dump = app.db.dump().await;
    assert!(!dump.contains(&token), "the session token reached the database");
    assert!(!has_jwt(&dump), "a Keycloak token reached the database");
    assert!(dump.contains("signed_in"));
    let kept = std::fs::read_dir(app.state_dir.join("accounts")).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap_or_default()).collect::<String>();
    assert!(!has_jwt(&kept), "a Keycloak token was kept");

    // The password on the appliance is the owner's backup alone (owner,
    // 2026-10-30: "The back up password is only for me"), so turning someone
    // off in Keycloak shuts them out.
    assert!(!set_password(&app, "carol", PASSWORD).status.success(), "a password was set for someone who signs in through Keycloak");
    let (st, v) = sign_in(&app, "carol", PASSWORD).await;
    assert_eq!(st, 401, "carol's old appliance password still signs her in: {v}");
    kc.disable("carol").await;
    let (st, v) = sign_in(&app, "carol", PASSWORD).await;
    assert_eq!(st, 401, "carol, turned off in Keycloak, signed in with her old appliance password: {v}");
    assert!(set_password(&app, "alice", PASSWORD).status.success());
    // The backup password alone opens nothing: it takes a code printed on
    // the appliance's machine too, once (a relay adds no proxy header, so
    // the address alone cannot say "this machine").
    let (st, v) = sign_in(&app, "alice", PASSWORD).await;
    assert_ne!(st, 200, "the owner's backup password alone signed her in: {v}");
    let code = sign_in_code(&app);
    let (st, v) = sign_in_with(&app, "alice", PASSWORD, Some(&code)).await;
    assert_eq!((st.as_u16(), v["person"]["id"].as_str()), (200, Some("alice")), "the owner's backup did not sign her in: {v}");
    let (st, v) = sign_in_with(&app, "alice", PASSWORD, Some(&code)).await;
    assert_eq!(st, 401, "a machine code signed in twice: {v}");

    // Only on the appliance's own machine (owner, 2026-10-30: "Make it
    // available only from the appliances own machine"): not through a
    // proxy in front of it, and not from another address.
    let code = sign_in_code(&app);
    for (name, value) in [("x-forwarded-for", "203.0.113.7"), ("forwarded", "for=203.0.113.7"), ("x-real-ip", "203.0.113.7")] {
        let r = reqwest::Client::new()
            .post(format!("{}/v1/sign-in", app.base))
            .header(name, value)
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "person": "alice", "password": PASSWORD, "machine_code": code, "client": "app" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401, "the owner's backup worked through a proxy ({name})");
    }
    // The machine's own network address, which is not loopback.
    let lan = {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        s.connect("192.0.2.1:9").unwrap();
        s.local_addr().unwrap().ip()
    };
    assert!(!lan.is_loopback(), "no network address to sign in from");
    let port = std::net::TcpListener::bind("0.0.0.0:0").unwrap().local_addr().unwrap().port();
    magnus_scratch::reconfigure(&mut app, |c| {
        c["appliance"].as_table_mut().unwrap().insert("tls_listen".into(), format!("0.0.0.0:{port}").into());
    })
    .await;
    let over = |host: String, code: String| async move {
        let r = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
            .post(format!("https://{host}/v1/sign-in"))
            .header("x-magnus-app", "0.5.0")
            .json(&json!({ "person": "alice", "password": PASSWORD, "machine_code": code, "client": "app" }))
            .send()
            .await
            .unwrap();
        let st = r.status();
        (st, r.json::<Value>().await.unwrap_or(Value::Null))
    };
    let (st, v) = over(format!("127.0.0.1:{port}"), sign_in_code(&app)).await;
    assert_eq!(st, 200, "the owner's backup did not work on this machine over HTTPS: {v}");
    let (st, v) = over(format!("{lan}:{port}"), sign_in_code(&app)).await;
    assert_eq!(st, 401, "the owner's backup worked from another address ({lan}): {v}");
}

/// When Keycloak cannot check a password typed in the app (Direct access
/// grants off, or the status client cannot ask whether the person uses a
/// code), the person is sent to Keycloak's page in the browser: never told
/// a right password is wrong, and never locked out for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_password_keycloak_cannot_check_in_the_app_goes_to_the_browser() {
    let model = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(spec(), &model).await;
    let kc = Keycloak::start().await;
    kc.add_person("carol").await;
    // The status client's secret is not in the vault under this name.
    kc.configure_with(&mut app, &[("status_credential", "no_such_secret")]).await;
    let pw = password_of("carol");
    let (st, v) = kc.app_sign_in(&app, "carol").await;
    assert_eq!(st, 200, "{v}");

    // Keycloak refuses passwords typed in the app altogether: its page.
    kc.set_direct_grants(false).await;
    let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "not sent to the browser: {v}");
    let (st, v) = kc.app_sign_in(&app, "carol").await;
    assert_eq!(st, 200, "the browser did not sign her in: {v}");
    kc.set_direct_grants(true).await;

    // She uses a code, and the appliance cannot ask Keycloak whether she does.
    let secret = kc.add_code("carol").await;
    for _ in 0..8 {
        let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
        assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "not sent to the browser: {v}");
        assert!(!v["error"].as_str().unwrap_or_default().contains("wrong"), "a right password was called wrong: {v}");
    }
    // With her code too: the appliance still cannot tell whether her
    // account needs a security key (the provider ignores a code from
    // someone without an authenticator app), so the browser.
    let (st, v) = kc.app_password(&app, "carol", &pw, Some(&code_for(&secret))).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "signed in without knowing whether a security key is needed: {v}");

    // Keycloak's admin API on an address of its own (`hostname-admin`), as
    // strix-alpha's is: the status client asks there.
    kc.configure_with(&mut app, &[("status_admin_url", "http://127.0.0.1:1")]).await;
    let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "the admin address set was not the one asked: {v}");
    kc.configure_with(&mut app, &[("status_admin_url", &server_url())]).await;
    let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
    assert_eq!((st, v["code_needed"].as_bool()), (401, Some(true)), "no code asked for through the admin address: {v}");
    // None of the times she was sent to the browser counted against her
    // (with the next code: Keycloak took the one above).
    let wait = 30 - std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 30;
    tokio::time::sleep(std::time::Duration::from_secs(wait) + std::time::Duration::from_millis(500)).await;
    let (st, v) = kc.app_password(&app, "carol", &pw, Some(&code_for(&secret))).await;
    assert_eq!((st, v["person"]["id"].as_str()), (200, Some("carol")), "locked out after being sent to the browser: {v}");
}

/// Option 1 of docs/proposals/accounts.md: the username and password typed
/// in the app, checked with Keycloak, then the code from an authenticator
/// app if the person uses one. Keycloak's own page in the browser the first
/// time, when the account needs finishing there, and when Keycloak knows no
/// such username.
#[tokio::test(flavor = "multi_thread")]
async fn a_password_typed_in_the_app_signs_in_through_keycloak_with_a_code_if_used_and_new_people_start_in_the_browser() {
    let model = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(spec(), &model).await;
    let kc = Keycloak::start().await;
    for who in ["carol", "bob"] {
        kc.add_person(who).await;
    }
    kc.configure(&mut app).await;
    let pw = password_of("carol");

    // The first time: Keycloak's own page, with the username filled in.
    let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "a first sign-in did not go to the browser: {v}");
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!("http://127.0.0.1:{}/callback", l.local_addr().unwrap().port());
    drop(l);
    let begun: Value = reqwest::Client::new()
        .post(format!("{}/v1/sign-in/browser", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "redirect_uri": redirect, "login_hint": "carol", "client": "app" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let page = url::Url::parse(begun["authorize_url"].as_str().unwrap()).unwrap();
    assert_eq!(page.query_pairs().find(|(k, _)| k == "login_hint").map(|(_, v)| v.into_owned()).as_deref(), Some("carol"), "the username typed was not passed on");
    let (st, v) = kc.app_sign_in(&app, "carol").await;
    assert_eq!(st, 200, "{v}");

    // From then on, the password typed in the app.
    let (st, v) = kc.app_password(&app, "carol", &pw, None).await;
    assert_eq!(st, 200, "{v}");
    let token = v["token"].as_str().expect("an app sign-in returns a token").to_string();
    let (st, me) = with_token(&app, &token).get("/v1/me").await;
    assert_eq!((st.as_u16(), me["id"].as_str()), (200, Some("carol")));
    let (st, v) = kc.app_password(&app, "carol", "not-her-password", None).await;
    assert_eq!((st, v["error"].as_str(), v.get("code_needed"), v.get("browser")), (401, Some("The name or password is wrong."), None, None), "{v}");

    // Two-factor on: the code is asked for, whether or not the password is
    // right, so asking tells nobody the password was.
    let secret = kc.add_code("carol").await;
    for tried in [pw.as_str(), "not-her-password"] {
        let (st, v) = kc.app_password(&app, "carol", tried, None).await;
        assert_eq!((st, v["code_needed"].as_bool()), (401, Some(true)), "no code asked for: {v}");
    }
    let right = code_for(&secret);
    let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000);
    let (st, v) = kc.app_password(&app, "carol", &pw, Some(&wrong)).await;
    assert_eq!((st, v["error"].as_str(), v.get("code_needed")), (401, Some("The name, password or code is wrong."), None), "{v}");
    let (st, v) = kc.app_password(&app, "carol", "not-her-password", Some(&right)).await;
    assert_eq!(st, 401, "a wrong password with the right code signed in: {v}");
    let (st, v) = kc.app_password(&app, "carol", &pw, Some(&code_for(&secret))).await;
    assert_eq!((st, v["person"]["id"].as_str()), (200, Some("carol")), "{v}");

    // An account that must be finished on Keycloak's page: the browser,
    // where the person sets their password, then the app's form again.
    let (st, _) = kc.app_sign_in(&app, "bob").await;
    assert_eq!(st, 200);
    kc.require_new_password("bob").await;
    let (st, v) = kc.app_password(&app, "bob", &password_of("bob"), None).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "an account needing setup was not sent to the browser: {v}");
    let (st, v) = kc.app_sign_in(&app, "bob").await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = kc.app_password(&app, "bob", &password_of("bob"), None).await;
    assert_eq!(st, 200, "{v}");

    // Someone Keycloak no longer knows: its own page, which says so.
    kc.delete("bob").await;
    let (st, v) = kc.app_password(&app, "bob", &password_of("bob"), None).await;
    assert_eq!((st, v["browser"].as_bool()), (401, Some(true)), "{v}");

    // No password reached the event log or the accounts folder.
    let dump = app.db.dump().await;
    assert!(!dump.contains(&pw), "a password reached the database");
    let kept = std::fs::read_dir(app.state_dir.join("accounts")).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap_or_default()).collect::<String>();
    assert!(!kept.contains(&pw), "a password was kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keycloak_sign_in_refuses_strangers_forgeries_other_clients_and_declines() {
    let model = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(spec(), &model).await;
    let kc = Keycloak::start().await;
    for who in ["mallory", "carol"] {
        kc.add_person(who).await;
    }
    kc.add_client("another-client").await;
    kc.configure(&mut app).await;
    let http = reqwest::Client::new();
    let begin = |redirect: String| {
        let (http, base) = (http.clone(), app.base.clone());
        async move { http.post(format!("{base}/v1/sign-in/browser")).header("x-magnus-app", "0.5.0").json(&json!({ "redirect_uri": redirect, "client": "app" })).send().await.unwrap() }
    };
    let finish = |body: Value| {
        let (http, base) = (http.clone(), app.base.clone());
        async move {
            let r = http.post(format!("{base}/v1/sign-in/browser/finish")).json(&body).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    // A browser's trip to a sign-in page, and what came back to the loopback.
    let trip = |page: String| {
        let kc = &kc;
        async move {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = l.local_addr().unwrap().port();
            let page = page.replace("PORT", &port.to_string());
            let (q, b) = tokio::join!(loopback_once(l), kc.browser(&page, "carol"));
            b.unwrap();
            q
        }
    };

    // Someone Keycloak knows but the appliance does not.
    let (st, v) = kc.app_sign_in(&app, "mallory").await;
    assert_eq!(st, 403, "someone not on the appliance got in: {v}");
    assert!(v["error"].as_str().unwrap().contains("not a person on this server"), "{v}");

    // The code only ever goes to the person's own computer.
    for elsewhere in ["https://evil.example/callback", "http://192.0.2.1:8080/callback", "http://127.0.0.1/callback"] {
        assert_eq!(begin(elsewhere.into()).await.status(), 400, "a sign-in may come back to {elsewhere}");
    }

    // A state the appliance never made, and a code it never asked for.
    let (st, _) = finish(json!({ "state": "made-up", "code": "made-up" })).await;
    assert_eq!(st, 410, "a made-up sign-in was finished");

    // Each check below gets a fresh code: Keycloak spends a code at its
    // first exchange, even a refused one.
    let fresh = || async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let begun: Value = begin(format!("http://127.0.0.1:{}/callback", l.local_addr().unwrap().port())).await.json().await.unwrap();
        let (q, browsed) = tokio::join!(loopback_once(l), kc.browser(begun["authorize_url"].as_str().unwrap(), "carol"));
        browsed.unwrap();
        (begun, q)
    };

    // A real code, said to come back from another issuer (RFC 9207).
    let (a, q) = fresh().await;
    let (st, v) = finish(json!({ "state": a["state"], "code": q["code"], "iss": "https://evil.example/realms/x" })).await;
    assert_eq!(st, 410, "a sign-in from another issuer was accepted: {v}");
    // And once a state is used, it is gone.
    let (st, _) = finish(json!({ "state": a["state"], "code": q["code"], "iss": q.get("iss") })).await;
    assert_eq!(st, 410, "a used sign-in was finished again");

    // A real code, answered for a different sign-in: its verifier and
    // redirect address are not that sign-in's, and Keycloak refuses it.
    let (_, q) = fresh().await;
    let other: Value = begin("http://127.0.0.1:9/callback".into()).await.json().await.unwrap();
    let (st, v) = finish(json!({ "state": other["state"], "code": q["code"], "iss": q.get("iss") })).await;
    assert_eq!(st, 410, "a code was accepted for another sign-in: {v}");

    // A code Keycloak gave another client, exchanged by ours.
    let c: Value = begin("http://127.0.0.1:9/callback".into()).await.json().await.unwrap();
    let theirs = c["authorize_url"]
        .as_str()
        .unwrap()
        .replace(&format!("client_id={}", magnus_scratch::IDP_CLIENT), "client_id=another-client")
        .replace("127.0.0.1%3A9%2F", "127.0.0.1%3APORT%2F");
    let c_redirect = c["authorize_url"].as_str().unwrap().replace("127.0.0.1%3A9%2F", "127.0.0.1%3APORT%2F");
    assert_ne!(theirs, c_redirect, "the page was not changed to another client");
    let q = trip(theirs).await;
    let (st, v) = finish(json!({ "state": c["state"], "code": q["code"] })).await;
    assert_eq!(st, 410, "a code for another client was accepted: {v}");

    // Declined on Keycloak's page.
    let s = device_start(&app).await;
    // Keycloak shows its own error page after a decline, so the browser's
    // answer is shown only when the decline did not reach the appliance.
    let browsed = kc.browser_declining(s["verification_uri_complete"].as_str().unwrap(), "carol").await;
    let (st, v) = device_finish(&app, s["flow"].as_str().unwrap()).await;
    assert_eq!((st, v["error"].as_str()), (410, Some("The sign-in was declined.")), "{v}; the browser: {browsed:?}");

    assert!(!app.db.dump().await.contains("signed_in"), "a refused sign-in was recorded as one");
}

#[path = "app/flows.rs"]
mod flows;
