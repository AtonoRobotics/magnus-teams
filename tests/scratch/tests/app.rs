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
    assert!(v["error"].as_str().unwrap().contains("not a person on this appliance"), "{v}");

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

#[tokio::test(flavor = "multi_thread")]
async fn plain_http_on_the_https_port_is_sent_to_https() {
    let mut f = magnus_scratch::Flow::start().await;
    let (url, _) = f.with_tls().await;
    let addr = url.trim_start_matches("https://").to_string();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(&addr).await.unwrap();
    s.write_all(format!("GET /#/inbox HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).await.unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 308"), "{reply:?}");
    assert!(reply.contains(&format!("Location: https://{addr}/#/inbox\r\n")), "{reply:?}");
    // HTTPS on the same port still works.
    let me = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap().get(format!("{url}/v1/sign-in")).send().await.unwrap();
    assert_eq!(me.status(), 200);
}

/// A browser offers HTTP/2 first; whatever the appliance agrees to, it answers.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_prefers_http2_is_answered_over_https() {
    let mut f = magnus_scratch::Flow::start().await;
    let (url, _) = f.with_tls().await;
    let browser = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let r = browser.get(format!("{url}/v1/health")).send().await.expect("an answer over HTTPS");
    assert_eq!(r.status(), 200);
    // The appliance offers only HTTP/1.1. A build of the whole workspace
    // can serve h2 (the scratch client's features unify into the server's
    // hyper), so an answer alone would not catch h2 offered again; the
    // protocol it agreed to does.
    assert_eq!(r.version(), reqwest::Version::HTTP_11, "the appliance agreed to HTTP/2");
}

/// The whole operator flow: plan, no review card, and an outcome with its
/// composition and evidence. (It had a policy-asked approval before spec
/// rev 140; asks are covered in asks.rs.)
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_a_review_and_the_outcome_carries_its_view() {
    let model = FakeModel::start(script(|c| async move {
        let last = c.last_results();
        match c.turn() {
            0 => contract_unchecked("note the figure", "the figure is remembered"),
            1 => tool(
                "task_plan",
                json!({ "units": [
                { "id": "u1", "goal": "remember the figure", "done_when": "memory has it", "status": "active" },
                { "id": "u2", "goal": "report", "done_when": "summary sent" }
            ] }),
            ),
            2 => tool("memory_write", json!({ "fact": "Q3 revenue is 41.7M", "topic": "finance" })),
            3 => tool("task_complete", json!({ "summary": "Remembered.", "deliverable": [{ "type": "nonsense" }] })),
            4 => {
                assert!(last.iter().any(|r| r.is_error && r.text.contains("unknown type")), "a bad view was not refused: {:?}", last.iter().map(|r| &r.text).collect::<Vec<_>>());
                tool(
                    "task_complete",
                    json!({
                        "summary": "Remembered the Q3 figure.",
                        "deliverable": [
                            { "type": "stat", "label": "Q3 revenue", "value": "41.7M" },
                            { "type": "bar_chart", "labels": ["Q2", "Q3"], "values": [38.2, 41.7], "unit": "M" }
                        ],
                        "spoken": "Q3 revenue was 41.7 million, up from 38.2."
                    }),
                )
            }
            _ => fail("unexpected turn"),
        }
    }))
    .await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Remember that Q3 revenue was 41.7M.").await;
    // Writing its own memory is inside Magnus: nothing asks (spec rev 140;
    // a policy's ask is no longer an ask), and checking is internal: the
    // run completes with nothing put to the person.
    let done = alice.wait_status(&id, &["completed"]).await;
    assert_eq!(done["outcome_view"][0]["type"], "stat");
    assert_eq!(done["spoken"], "Q3 revenue was 41.7 million, up from 38.2.");

    let (_, tl) = alice.get(&format!("/v1/tasks/{id}/timeline")).await;
    let calls: Vec<&Value> = tl["steps"].as_array().unwrap().iter().flat_map(|s| s["calls"].as_array().unwrap()).collect();
    let mw = calls.iter().find(|c| c["tool"] == "memory_write").expect("memory_write in the timeline");
    assert_eq!(mw["issuer"], "model");
    assert!(mw["decisions"].as_array().unwrap().iter().all(|d| d["outcome"] == "allow"), "{mw}");
    assert!(mw["decisions"].as_array().unwrap().iter().all(|d| d["rule"].is_null() && d["reason"].is_null()), "the task view shows a decision's reasoning: {mw}");
}

/// The inspector compares each run's instructions with the build's own: the
/// chat instructions for a chat turn, the task instructions for a task. It
/// said every chat turn ran on instructions other than the build's.
#[tokio::test(flavor = "multi_thread")]
async fn the_inspector_matches_chat_and_task_runs_to_their_own_instructions() {
    let s = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            return Reply::Text("Hello.".into());
        }
        match c.turn() {
            0 => contract_unchecked("x", "x"),
            _ => fail("nothing to do"),
        }
    });
    let model = FakeModel::start(s).await;
    let app = Appliance::start(spec(), &model).await;
    let dana = app.api("dana");
    let task = dana.submit("Look something up.").await;
    let task_run = dana.wait_status(&task, &["failed"]).await["run_id"].as_str().unwrap().to_string();
    let (st, v) = dana.post("/v1/chat", json!({ "text": "Hi" })).await;
    assert_eq!(st, 201, "{v}");
    let chat_run = dana.wait_status(v["task_id"].as_str().unwrap(), &["completed"]).await["run_id"].as_str().unwrap().to_string();
    for (what, run, file) in [("task", task_run, "agent.md@"), ("chat turn", chat_run, "chat.md@")] {
        let (_, ins) = dana.get(&format!("/v1/dev/runs/{run}")).await;
        let ctx = &ins["steps"][0]["context"];
        assert!(ctx["instructions"].as_str().unwrap().starts_with(file), "{what}: {ctx}");
        assert_eq!(ctx["instructions_match_build"], true, "the {what} is said to run on other instructions: {ctx}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_developer_view_needs_the_role_and_drills_down_to_the_log() {
    let model = FakeModel::start(steps(vec![contract_unchecked("x", "x"), tool("memory_search", json!({ "query": "anything" })), fail("nothing to do")])).await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let dana = app.api("dana");
    let id = alice.submit("Look something up.").await;
    let task = alice.wait_status(&id, &["failed"]).await;
    let run = task["run_id"].as_str().unwrap().to_string();

    for path in ["/v1/dev/runs", "/v1/dev/metrics", "/v1/dev/experiments", "/v1/dev/policy", "/v1/dev/playbook", &format!("/v1/dev/runs/{run}/log")] {
        assert_eq!(alice.get(path).await.0, 403, "{path} is open to a person without the developer role");
        assert_eq!(dana.get(path).await.0, 200, "{path}");
    }

    let (_, runs) = dana.get("/v1/dev/runs").await;
    let row = runs["runs"].as_array().unwrap().iter().find(|r| r["run"] == run.as_str()).expect("the run is listed");
    assert_eq!(row["failure"], "agent");

    let (_, ins) = dana.get(&format!("/v1/dev/runs/{run}")).await;
    let steps = ins["steps"].as_array().unwrap();
    assert!(steps.len() >= 3, "{ins}");
    let sources = steps[0]["context"]["sources"].as_array().unwrap();
    for s in ["tool definitions", "instructions", "task"] {
        assert!(sources.iter().any(|x| x["source"] == s && x["tokens"].as_u64().is_some()), "no {s} in {sources:?}");
    }
    let later = steps[2]["context"]["sources"].as_array().unwrap();
    assert!(later.iter().any(|x| x["source"] == "tool results: memory_search"), "results are not attributed: {later:?}");
    let searches: Vec<&Value> = ins["actions"].as_array().unwrap().iter().filter(|a| a["tool"] == "memory_search").collect();
    assert_eq!(searches.len(), 1, "one call is one action");
    let search = searches[0];
    assert!(search["decisions"].as_array().unwrap().iter().any(|d| d["stage"] == "policy"));
    assert_eq!(search["issuer"], "model");
    assert!(search["result"]["text"].is_string(), "{}", serde_json::to_string_pretty(&ins).unwrap());
    assert!(ins["harness"].as_array().unwrap().iter().any(|h| h["kind"] == "failed"));

    let (_, log) = dana.get(&format!("/v1/dev/runs/{run}/log")).await;
    assert_eq!(log["events"].as_array().unwrap().len() as u64, log["total"].as_u64().unwrap());
    assert!(log["events"].as_array().unwrap().iter().any(|e| e["kind"] == "task_created"));

    // Every number drills down to runs.
    let (_, m) = dana.get("/v1/dev/metrics").await;
    let failures = m["panels"].as_array().unwrap().iter().find(|p| p["id"] == "failures").unwrap();
    assert!(failures["runs"].as_array().unwrap().iter().any(|r| r == run.as_str()));
    let capture = m["panels"].as_array().unwrap().iter().find(|p| p["id"] == "capture").unwrap();
    assert!(capture["value"].as_f64().is_some(), "capture overhead is not measured: {capture}");
}

#[tokio::test(flavor = "multi_thread")]
async fn experiments_report_pass_at_k_from_repeated_trials() {
    // Case "good" completes, case "bad" fails; the grader marks one good trial wrong.
    let model = FakeModel::start(script(|c| async move {
        let good = c.messages()[0].to_string().contains("GOOD");
        match c.turn() {
            0 => contract_checked("x", "mail__list_sent", json!({}), "/count", json!(0)),
            _ if good => complete("done"),
            _ => fail("cannot"),
        }
    }))
    .await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]), &model).await;
    let dana = app.api("dana");
    let mut runs = vec![];
    for (case, text) in [("good", "GOOD task"), ("bad", "BAD task")] {
        for trial in 0..3 {
            let (st, v) = dana.post("/v1/dev/experiments/e1/trials", json!({ "case": case, "trial": trial, "text": text })).await;
            assert_eq!(st, 201, "{v}");
            runs.push((case, v["task_id"].as_str().unwrap().to_string(), v["run_id"].as_str().unwrap().to_string()));
        }
    }
    let (st, _) = dana.post("/v1/dev/experiments/e1/trials", json!({ "case": "good", "trial": 0, "text": "GOOD task" })).await;
    assert_eq!(st, 409, "a trial ran twice");
    for (_, task, _) in &runs {
        dana.wait_settled(task).await;
    }
    let good: Vec<&String> = runs.iter().filter(|r| r.0 == "good").map(|r| &r.2).collect();
    for r in &good {
        let passed = *r != good[0];
        let (st, _) = dana.post(&format!("/v1/dev/runs/{r}/grades"), json!({ "grader": "check", "passed": passed })).await;
        assert_eq!(st, 200);
    }
    let (_, e) = dana.get("/v1/dev/experiments/e1?k=1").await;
    let cfg = &e["configs"][0];
    let case = |name: &str| cfg["cases"].as_array().unwrap().iter().find(|c| c["case"] == name).unwrap().clone();
    assert_eq!(case("good")["passed"], 2);
    assert_eq!(case("bad")["passed"], 0);
    // pass@1 = c/n per case; the mean over cases is (2/3 + 0) / 2.
    let p = cfg["pass_at_k"]["mean"].as_f64().unwrap();
    assert!((p - 1.0 / 3.0).abs() < 1e-3, "pass@1 = {p}");
    let (_, e3) = dana.get("/v1/dev/experiments/e1?k=3").await;
    let good3 = e3["configs"][0]["cases"].as_array().unwrap().iter().find(|c| c["case"] == "good").unwrap().clone();
    assert_eq!(good3["pass_at_k"], 1.0);
    assert_eq!(good3["pass_hat_k"], 0.0);
    let tags: Vec<&str> = cfg["failures"].as_array().unwrap().iter().map(|f| f["where"].as_str().unwrap()).collect();
    assert!(tags.contains(&"grader_disagrees") && tags.contains(&"agent"), "{tags:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn playbook_items_reach_later_tasks_and_carry_their_taint() {
    let web = FakeWeb::start(&[("/howto", "<p>Invoices go to finance@example.com.</p>")]).await;
    let howto = web.url("/howto");
    let model = FakeModel::start(script(move |c| {
        let howto = howto.clone();
        async move {
            let first = c.messages()[0].to_string().contains("Learn");
            match (first, c.turn()) {
                (_, 0) => contract_unchecked("x", "x"),
                (true, 1) => tool("web_fetch", json!({ "url": howto })),
                (true, 2) => tool("playbook_update", json!({ "op": "add", "text": "Invoices go to finance@example.com.", "addresses": "where invoices go" })),
                (true, _) => fail("done"),
                (false, _) => {
                    assert!(c.messages()[0].to_string().contains("Invoices go to finance@example.com."), "the playbook item did not reach the next task");
                    fail("done")
                }
            }
        }
    }))
    .await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let a = alice.submit("Learn where invoices go.").await;
    alice.wait_status(&a, &["failed"]).await;
    let b = alice.submit("Send the invoice.").await;
    let later = alice.wait_status(&b, &["failed"]).await;
    assert_eq!(later["taint"]["untrusted"], true, "an item learned from a web page laundered its taint: {}", later["taint"]);

    let (_, pb) = app.api("dana").get("/v1/dev/playbook").await;
    let item = &pb["items"][0];
    assert_eq!(item["person"], "alice");
    assert_eq!(item["label"]["untrusted"], true, "{item}");
    assert_eq!(item["with_item"]["runs"].as_array().unwrap().len(), 1, "{pb}");
    let (_, v) = alice.get("/v1/versions").await;
    assert!(v["changes"].as_array().unwrap().iter().any(|c| c["what"] == "playbook"), "{v}");
    assert!(v["changes"].as_array().unwrap().iter().any(|c| c["what"] == "harness"), "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_change_is_a_version_change() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let (_, before) = alice.get("/v1/versions").await;
    let (st, v) = alice.put("/v1/model", json!({ "adapter": "anthropic", "model": "claude-other" })).await;
    assert!(st.is_success(), "{v}");
    let (_, after) = alice.get("/v1/versions").await;
    assert_eq!(after["model"], "anthropic/claude-other");
    assert!(after["latest_seq"].as_i64() > before["latest_seq"].as_i64());
}

/// A person sees their own memories and can have one forgotten; nobody
/// else can, and a forgotten memory is gone after a restart too.
#[tokio::test(flavor = "multi_thread")]
async fn a_person_forgets_a_memory() {
    let model = FakeModel::start(steps(vec![
        contract_unchecked("remember", "remembered"),
        tool("memory_write", json!({ "fact": "alice prefers PDF reports", "topic": "reports" })),
        fail("done"),
    ]))
    .await;
    let mut app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let id = alice.submit("Remember that I like PDF reports.").await;
    alice.wait_settled(&id).await;
    let (_, v) = alice.get("/v1/memory").await;
    let mem = v["memory"].as_array().unwrap().clone();
    assert_eq!(mem.len(), 1, "{v}");
    let mid = mem[0]["id"].as_str().unwrap().to_string();
    assert!(bob.get("/v1/memory").await.1["memory"].as_array().unwrap().is_empty(), "bob sees alice's memory");
    let (st, _) = bob.send(reqwest::Method::DELETE, &format!("/v1/memory/{mid}"), None).await;
    assert_eq!(st, 404, "bob forgot alice's memory");
    let (st, _) = alice.send(reqwest::Method::DELETE, &format!("/v1/memory/{mid}"), None).await;
    assert_eq!(st, 204);
    app.restart().await;
    assert!(alice.get("/v1/memory").await.1["memory"].as_array().unwrap().is_empty(), "the memory came back");
}
