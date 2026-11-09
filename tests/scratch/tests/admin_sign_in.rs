//! Signing in to the admin console (admin console spec, invariant tests,
//! Sign-in and authority; owner's rulings 1 and 2, and #429: no passkeys
//! and no provider page — the password, and the code when there is one, on
//! the console's own form), in a real Chromium against a real appliance.
//! Only the console role `admin` enters; a member's app session or token
//! opens nothing; every change carries the session's CSRF token; a step-up
//! is a fresh password and lapses; an admin turned off at the appliance
//! loses the console at the next check, well within five minutes.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::{json, Value};

/// A browser sign-in on a busy build box outlasted 30 seconds, so the
/// session's step-up had already lapsed when it was read (run 41949527809,
/// `stepped_up_until: null`). The test still waits the window out.
const STEP_UP_SECONDS: i64 = 90;

#[tokio::test(flavor = "multi_thread")]
async fn only_an_admin_signs_in_on_the_consoles_own_form_and_the_session_holds_to_its_rules() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    local_user(&app, "Carol", PASSWORD, false);
    // A second admin, so Alice can be turned off and on below: the
    // appliance never leaves the organisation with none.
    local_user(&app, "Bob", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    // Short windows, so the test sees a step-up lapse and a turned-off
    // admin found out without waiting minutes.
    // A step-up counts from the password typed on the console's form; a
    // real browser's sign-in on a busy machine takes seconds to come back,
    // so the window is wide enough for that, and the test waits it out.
    let p = prepare(&app, &[&mine], &[("step_up_seconds", STEP_UP_SECONDS.into()), ("refresh_seconds", 2.into())]);
    let url = p.url();
    let c = launch(p).await;
    let api = |path: &str| format!("{url}/api/v1{path}");
    let b = browser(&c, false);

    // A member signs in on the console's form and is turned away at the door.
    let carol = sign_in(&url, c.addr, "Carol", PASSWORD, "refused", false).await.unwrap_or_else(|e| panic!("{e}"));
    assert!(carol.is_empty(), "a member was given a console session");
    // A member's app session, or the admin's own, opens nothing here.
    let r = reqwest::Client::new()
        .post(format!("{}/v1/sign-in", app.base))
        .header("x-magnus-app", "0.5.0")
        .json(&json!({ "person": "alice", "password": PASSWORD, "client": "app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = r.json().await.unwrap();
    let token = v["token"].as_str().unwrap().to_string();
    for path in ["/session", "/users", "/session/step-up"] {
        let r = b.get(api(path)).bearer_auth(&token).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{path} took the app's token");
        let r = b.post(api(path)).header("x-magnus-token", &token).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{path} took the app's token");
    }

    // The admin signs in on the console's form, and steps up there too.
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", true).await.unwrap_or_else(|e| panic!("{e}"));
    assert!(!sid.is_empty(), "the admin has no console session");
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    assert!(me["csrf"].as_str().is_some_and(|t| t.len() >= 20) && me["stepped_up_until"].is_string(), "{me}");
    let csrf = me["csrf"].as_str().unwrap().to_string();

    // A change without the session's token is refused; with it, done.
    let r = with_cookie(b.post(api("/session/step-up")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "a change without the CSRF token was taken");
    let r = with_cookie(b.post(api("/session/step-up")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT, "a fresh step-up did not count");
    // Once the step-up lapses, an action that needs one says so.
    tokio::time::sleep(std::time::Duration::from_secs(STEP_UP_SECONDS as u64 + 1)).await;
    let r = with_cookie(b.post(api("/session/step-up")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["error"], "step_up_required", "{v}");

    // Signing out ends the session.
    let r = with_cookie(b.delete(api("/session")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(with_cookie(b.get(api("/session")), &sid).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);

    // Signed in again, then turned off at the appliance: out at the next check.
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(with_cookie(b.get(api("/session")), &sid).send().await.unwrap().status(), StatusCode::OK);
    set_status(&app, "alice", "disabled");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let r = with_cookie(b.get(api("/session")), &sid).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "an admin turned off at the appliance kept the console");
    set_status(&app, "alice", "active");
    assert_eq!(with_cookie(b.get(api("/session")), &sid).send().await.unwrap().status(), StatusCode::UNAUTHORIZED, "the ended session came back");

    // Every one of those is in the audit log, with who and from where; the
    // log's CSV says the same.
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let audit: Value = with_cookie(b.get(api("/audit")), &sid).send().await.unwrap().json().await.unwrap();
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["sign_in_refused", "sign_in", "step_up", "sign_out", "session_ended"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {audit}");
    }
    let ip = lan_ip().to_string();
    assert!(audit["entries"].as_array().unwrap().iter().all(|e| e["source"] == ip.as_str() && e["at"].is_string()), "{audit}");
    let filtered: Value = with_cookie(b.get(api("/audit?action=sign_out")), &sid).send().await.unwrap().json().await.unwrap();
    assert!(filtered["entries"].as_array().unwrap().iter().all(|e| e["action"] == "sign_out") && !filtered["entries"].as_array().unwrap().is_empty(), "{filtered}");
    let csv = with_cookie(b.get(api("/audit.csv")), &sid).send().await.unwrap().text().await.unwrap();
    assert!(csv.starts_with("seq,at,admin,name,action,target,source,before,after") && csv.contains("\"sign_in_refused\""), "{csv}");
}

/// A sign-in finishes only in the browser that opened its form, and once:
/// the form's binding is spent by the first submission, right or wrong.
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_finishes_only_in_the_browser_that_opened_it_and_once() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    let p = prepare(&app, &[&mine], &[]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    // The form's binding: its token and the cookie that carries it back.
    let form = async || {
        let r = b.get(format!("{url}/auth/local")).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let cookie = r
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("__Host-magnus_local="))
            .expect("the form sets its binding cookie")
            .to_string();
        let html = r.text().await.unwrap();
        let token = html.split("name=login_token value=\"").nth(1).and_then(|s| s.split('"').next()).expect("the form carries its binding").to_string();
        (token, cookie)
    };
    let post = async |token: &str, cookie: Option<&str>, password: &str| {
        let mut rq = b.post(format!("{url}/auth/local")).form(&[("person", "alice"), ("password", password), ("login_token", token)]);
        if let Some(c) = cookie {
            rq = rq.header("cookie", c);
        }
        rq.send().await.unwrap()
    };
    let session = |r: &reqwest::Response| r.headers().get_all("set-cookie").iter().any(|v| v.to_str().unwrap_or_default().starts_with("__Host-magnus_admin="));
    // Another browser, with the right token but not its cookie, is refused.
    let (token, _) = form().await;
    let r = post(&token, None, PASSWORD).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(!session(&r), "a sign-in with no binding was given a console session");
    let html = r.text().await.unwrap();
    assert!(html.contains("This sign-in expired"), "{html}");
    // A wrong password opens nothing, even with the cookie — and spends it.
    let (token, cookie) = form().await;
    let r = post(&token, Some(&cookie), "not the password").await;
    assert!(!session(&r), "a wrong password was given a console session");
    assert!(r.text().await.unwrap().contains("The name, password or code is wrong"));
    let r = post(&token, Some(&cookie), PASSWORD).await;
    assert!(!session(&r), "the spent binding worked again");
    assert!(r.text().await.unwrap().contains("This sign-in expired"));
    // Fresh, the password signs in — and that binding is spent too.
    let (token, cookie) = form().await;
    let r = post(&token, Some(&cookie), PASSWORD).await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    assert!(session(&r), "the password was not given a console session");
    let r = post(&token, Some(&cookie), PASSWORD).await;
    assert!(!session(&r), "the spent binding worked again");
    assert!(r.text().await.unwrap().contains("This sign-in expired"));
}

/// From the console: "Stop all agents" needs no step-up, Release does
/// (admin console spec, Kill switch; invariant "Release without step-up
/// fails"), and both are in the audit log; Overview and Health answer.
#[tokio::test(flavor = "multi_thread")]
async fn the_console_stops_all_agents_at_once_and_releases_only_with_a_fresh_password() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    // Wide enough for a browser's sign-in on a busy machine (see above).
    let p = prepare(&app, &[&mine], &[("step_up_seconds", STEP_UP_SECONDS.into())]);
    let url = p.url();
    let c = launch(p).await;
    let api = |path: &str| format!("{url}/api/v1{path}");
    let b = browser(&c, false);
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    let csrf = me["csrf"].as_str().unwrap().to_string();
    for path in ["/overview", "/health"] {
        let r = with_cookie(b.get(api(path)), &sid).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{path}");
    }
    let o: Value = with_cookie(b.get(api("/overview")), &sid).send().await.unwrap().json().await.unwrap();
    assert!(o["cards"]["running_tasks"].is_number() && o["status"]["version"].is_string(), "{o}");

    // Long after the sign-in's own step-up has lapsed: engaging still works.
    tokio::time::sleep(std::time::Duration::from_secs(STEP_UP_SECONDS as u64 + 1)).await;
    let r = with_cookie(b.post(api("/kill-switch/engage")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(app.api("alice").get("/v1/kill-switch").await.1["engaged"], true, "the app's switch is not on");
    // Release without a fresh password fails, and leaves it on.
    let r = with_cookie(b.post(api("/kill-switch/release")), &sid).header("x-csrf-token", &csrf).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(r.json::<Value>().await.unwrap()["error"], "step_up_required");
    assert_eq!(app.api("alice").get("/v1/kill-switch").await.1["engaged"], true);
    // With one, it is released.
    let sid2 = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let me: Value = with_cookie(b.get(api("/session")), &sid2).send().await.unwrap().json().await.unwrap();
    let r = with_cookie(b.post(api("/kill-switch/release")), &sid2).header("x-csrf-token", me["csrf"].as_str().unwrap()).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(app.api("alice").get("/v1/kill-switch").await.1["engaged"], false);
    let audit: Value = with_cookie(b.get(api("/audit?action=kill_switch")), &sid2).send().await.unwrap().json().await.unwrap();
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    assert!(actions.contains(&"kill_switch_engaged") && actions.contains(&"kill_switch_released"), "{audit}");
}

/// Signing in again asks for the password anew, and counts as a step-up:
/// nothing remembers the browser, so there is no silent entry (admin
/// console spec, Step-up; owner's ruling 2).
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_again_asks_for_the_password_and_counts_as_a_step_up() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    let p = prepare(&app, &[&mine], &[("step_up_seconds", 30.into())]);
    let url = p.url();
    let c = launch(p).await;
    let api = |path: &str| format!("{url}/api/v1{path}");
    let b = browser(&c, false);
    let sid = sign_in_with(&url, c.addr, "Alice", PASSWORD, "admin", false, None, &[("MAGNUS_AGAIN", "1".into())]).await.unwrap_or_else(|e| panic!("{e}"));
    let me: Value = with_cookie(b.get(api("/session")), &sid).send().await.unwrap().json().await.unwrap();
    assert!(me["stepped_up_until"].is_string(), "a sign-in with the password typed did not count as a step-up: {me}");
    let r = with_cookie(b.post(api("/session/step-up")), &sid).header("x-csrf-token", me["csrf"].as_str().unwrap()).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT, "a fresh step-up did not count");
}

/// A page load that the machine's network changing cut off is loaded again,
/// at most three times; any other failure is still the test's at once.
/// Release run 42032928589 (#171) failed two console tests at their first
/// page with `net::ERR_NETWORK_CHANGED` on a shared build box.
#[test]
fn the_console_sign_in_script_loads_a_page_again_when_the_network_changes() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests");
    let src = std::fs::read_to_string(tests.join("admin-sign-in.mjs")).unwrap();
    assert!(!src.contains("page.goto("), "every page the script opens goes through goto(): {src}");
    let helper = url::Url::from_file_path(tests.join("goto.mjs").canonicalize().expect("desktop/tests/goto.mjs")).unwrap();
    let check = format!(
        r#"
const {{ goto }} = await import("{helper}");
let a = 0;
const flaky = {{ goto: async u => {{ if (++a === 1) throw new Error("page.goto: net::ERR_NETWORK_CHANGED at " + u); return "loaded " + u; }} }};
const r = await goto(flaky, "https://console/");
if (r !== "loaded https://console/" || a !== 2) throw new Error(`a network change: ${{r}} after ${{a}} loads`);
let b = 0;
const refused = {{ goto: async () => {{ b++; throw new Error("page.goto: net::ERR_CONNECTION_REFUSED"); }} }};
await goto(refused, "https://console/").then(() => {{ throw new Error("a refusal passed"); }}, e => {{ if (!e.message.includes("REFUSED") || b !== 1) throw new Error(`a refusal: ${{e.message}} after ${{b}} loads`); }});
let c = 0;
const changing = {{ goto: async () => {{ c++; throw new Error("page.goto: net::ERR_NETWORK_CHANGED"); }} }};
await goto(changing, "https://console/").then(() => {{ throw new Error("a network that never settles passed"); }}, e => {{ if (!e.message.includes("NETWORK_CHANGED") || c !== 3) throw new Error(`never settling: ${{e.message}} after ${{c}} loads`); }});
console.log("ok");
"#
    );
    let out = std::process::Command::new("node").args(["--input-type=module", "-e", &check]).output().expect("run node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// A network change Chromium answered with its error page is loaded again
/// too: Playwright reports that load as interrupted by
/// `chrome-error://chromewebdata/`, and only the page's own request names
/// `net::ERR_NETWORK_CHANGED` (#218; runs 42139753250, 42146521203). Any
/// other failed request is still reported at once, with its error named.
/// Playwright can reject the load before it reports that failure, or not
/// report it: the failure is waited for, and an error page with no cause
/// named is loaded again too (#248; run 42164847174).
#[test]
fn a_network_change_shown_as_chromes_error_page_is_loaded_again() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests");
    let helper = url::Url::from_file_path(tests.join("goto.mjs").canonicalize().expect("desktop/tests/goto.mjs")).unwrap();
    let check = format!(
        r#"
const {{ goto }} = await import("{helper}");
// A page as Playwright drives one: the main frame's request fails with
// `error`, then the load is reported as interrupted by Chrome's error page.
// `when` is "late" when the failure is reported after the load was
// rejected, and "never" when it is not reported at all (#248).
function page(error, failures, when = "first") {{
  const frame = {{}};
  const on = new Map();
  let loads = 0;
  return {{
    get loads() {{ return loads; }},
    mainFrame: () => frame,
    on: (ev, f) => on.set(ev, f),
    off: ev => on.delete(ev),
    goto: async u => {{
      if (++loads > failures) return "loaded " + u;
      const fail = on.get("requestfailed");
      const report = () => fail?.({{ isNavigationRequest: () => true, frame: () => frame, failure: () => ({{ errorText: error }}) }});
      if (when === "first") report();
      if (when === "late") setTimeout(report, 200);
      throw new Error(`page.goto: Navigation to "${{u}}" is interrupted by another navigation to "chrome-error://chromewebdata/"`);
    }},
  }};
}}
const changed = page("net::ERR_NETWORK_CHANGED", 1);
const r = await goto(changed, "https://console/");
if (r !== "loaded https://console/" || changed.loads !== 2) throw new Error(`a network change shown as Chrome's error page: ${{r}} after ${{changed.loads}} loads`);
const refused = page("net::ERR_CONNECTION_REFUSED", 9);
await goto(refused, "https://console/").then(() => {{ throw new Error("a refusal passed"); }}, e => {{ if (!e.message.includes("net::ERR_CONNECTION_REFUSED") || refused.loads !== 1) throw new Error(`a refusal: ${{e.message}} after ${{refused.loads}} loads`); }});
const never = page("net::ERR_NETWORK_CHANGED", 9);
await goto(never, "https://console/").then(() => {{ throw new Error("a network that never settles passed"); }}, e => {{ if (never.loads !== 3) throw new Error(`never settling: ${{e.message}} after ${{never.loads}} loads`); }});
const late = page("net::ERR_NETWORK_CHANGED", 1, "late");
const r2 = await goto(late, "https://console/");
if (r2 !== "loaded https://console/" || late.loads !== 2) throw new Error(`a network change reported after the load failed: ${{r2}} after ${{late.loads}} loads`);
const lateRefusal = page("net::ERR_CONNECTION_REFUSED", 9, "late");
await goto(lateRefusal, "https://console/").then(() => {{ throw new Error("a late refusal passed"); }}, e => {{ if (!e.message.includes("net::ERR_CONNECTION_REFUSED") || lateRefusal.loads !== 1) throw new Error(`a late refusal: ${{e.message}} after ${{lateRefusal.loads}} loads`); }});
const unsaid = page("", 1, "never");
const r3 = await goto(unsaid, "https://console/");
if (r3 !== "loaded https://console/" || unsaid.loads !== 2) throw new Error(`Chrome's error page with no failure named: ${{r3}} after ${{unsaid.loads}} loads`);
const silent = page("", 9, "never");
await goto(silent, "https://console/").then(() => {{ throw new Error("an error page that never clears passed"); }}, e => {{ if (!e.message.includes("named no failure") || silent.loads !== 3) throw new Error(`never clearing: ${{e.message}} after ${{silent.loads}} loads`); }});
console.log("ok");
"#
    );
    let out = std::process::Command::new("node").args(["--input-type=module", "-e", &check]).output().expect("run node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[test]
fn the_console_sign_in_script_waits_two_minutes() {
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/tests/admin-sign-in.mjs")).unwrap();
    assert!(src.contains("const T = 120_000"), "{src}");
}

/// The console and Keycloak helpers' HTTP clients give up after
/// `HTTP_PATIENCE`: a request that never answers fails its test, named,
/// instead of holding it silently until the runner kills it at 600 s, as
/// master's run 42152977968 was (#239).
#[test]
fn the_console_and_keycloak_helpers_never_wait_forever_on_an_answer() {
    assert!(magnus_scratch::HTTP_PATIENCE < std::time::Duration::from_secs(600), "a request outlasts the runner's own limit");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for f in ["keycloak.rs", "admin.rs"] {
        let text = std::fs::read_to_string(src.join(f)).unwrap();
        assert!(!text.contains("Client::new()"), "{f} builds an HTTP client with no timeout");
        for (i, _) in text.match_indices("Client::builder()") {
            let statement = &text[i..i + text[i..].find(";").unwrap()];
            assert!(statement.contains(".timeout("), "{f} builds an HTTP client with no timeout: {statement}");
        }
    }
}

/// The mail stand-in's docker commands give up after `DOCKER_PATIENCE`
/// seconds and say which command stalled: a container engine stalled under
/// load held the invite test silently until the runner killed it at 600 s,
/// on master's runs 42152977968 and 42164793481 (#246).
#[test]
fn the_mail_stand_in_never_waits_forever_on_docker() {
    let secs: u64 = magnus_scratch::mail::DOCKER_PATIENCE.parse().unwrap();
    assert!(secs < 600, "a docker command outlasts the runner's own limit");
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mail.rs")).unwrap();
    for (i, _) in text.match_indices("Command::new(") {
        let call = &text[i..i + text[i..].find(')').unwrap()];
        assert!(!call.contains("\"docker\""), "mail.rs runs docker with no time limit: {call}");
    }
}
