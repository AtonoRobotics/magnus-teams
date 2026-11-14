//! Brokered connectors run on the appliance (connectors spec, Brokered API
//! connectors), each person's instance in its own Firecracker microVM
//! booted from the server image its manifest names by digest: the test
//! server (`brokered/test-server`), built from this checkout. A local HTTPS
//! host, with a certificate from the test's own CA, stands in for the
//! provider's API, and FakeOAuthApp for the provider's
//! authorization server (the real ones need an account there; the contract
//! tests use one).

use magnus_invariants::*;
use magnus_scratch::brokered::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const TOOLS: &[(&str, &str)] = &[("fetch", "read"), ("probe", "read"), ("leak", "read")];

/// `who` signs in to the connector at the provider.
async fn sign_in(api: &Api) {
    sign_in_to(api, "testapp").await
}

async fn sign_in_to(api: &Api, connector: &str) {
    let (st, v) = api.post(&format!("/v1/connectors/{connector}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(api.post(&format!("/v1/connectors/{connector}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
}

/// `p` asks for a task and accepts what it did once it is done.
async fn run(p: &Api, text: &str) {
    let t = p.submit(text).await;
    p.wait_status(&t, &["completed"]).await;
    assert_eq!(p.wait_settled(&t).await["status"], "completed");
}

/// What each tool call of a task's run returned, in order, as the model saw it.
fn results(m: &FakeModel, task_text: &str) -> Vec<String> {
    let of = |c: &Conversation| -> Vec<String> {
        c.messages()
            .iter()
            .filter(|x| x["role"] == "user")
            .flat_map(|x| x["content"].as_array().cloned().unwrap_or_default())
            .filter(|b| b["type"] == "tool_result")
            .map(|b| b["content"].to_string())
            .collect()
    };
    // The run's longest conversation with the model: the one at its end.
    m.requests().iter().filter(|c| c.text().contains(task_text)).map(of).max_by_key(|r| r.len()).expect("the task's run")
}

/// A field of the server's JSON answer, in the text of a tool result.
fn field(result: &str, key: &str) -> String {
    let content: Value = serde_json::from_str(result).unwrap_or_default();
    let text = content.as_array().and_then(|a| a.first()).and_then(|b| b["text"].as_str()).map(String::from).unwrap_or_else(|| content.as_str().unwrap_or(result).to_string());
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else { return String::new() };
    let v: Value = serde_json::from_str(&text[a..=b]).unwrap_or_default();
    v[key].as_str().map(String::from).unwrap_or_else(|| v[key].to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn each_person_has_their_own_microvm_called_with_their_current_token_reaching_only_its_hosts() {
    let api = ProviderApi::start().await;
    let elsewhere = Elsewhere::start().await;
    let (host, other) = (api.host.clone(), elsewhere.host.clone());
    let m = FakeModel::start(script(move |c| {
        let (host, other) = (host.clone(), other.clone());
        async move {
            let fetch = tool("testapp__fetch", json!({ "host": host, "path": "/inbox" }));
            match c.turn() {
                0 => plan("read the inbox"),
                1 => fetch,
                2 => fetch,
                3 if c.text().contains("and try the rest") => tool("testapp__leak", json!({})),
                4 if c.text().contains("and try the rest") => tool("testapp__probe", json!({ "host": other })),
                _ => complete("done"),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    // Every call's token is a fresh one: each lasts a minute, and is
    // refreshed a minute before it ends.
    let provider = FakeOAuthApp::provider_api(60).await;
    let image = image("magnus-brokered-test");
    // Longer than the steps of one run are apart, on a busy test machine.
    let idle_s = 20;
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, idle_s, "testapp", std::slice::from_ref(&api.host), &provider.base, TOOLS);
        trust(cfg, &api.ca);
    })
    .await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    sign_in(&alice).await;
    sign_in(&carol).await;

    run(&alice, "Check the inbox and try the rest").await;
    assert_eq!(running(&app.state_dir), 1, "alice's microVM");
    run(&carol, "Check my inbox").await;
    assert!(running(&app.state_dir) >= 1, "carol's microVM");

    // Each call reached the provider over TLS with the person's own token,
    // the one current then: never a refresh token, never one call's for
    // another.
    assert!(!String::from_utf8_lossy(&api.raw.lock().unwrap()).contains("Bearer"), "a token went out in the clear");
    let seen = api.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4, "{seen:?}");
    let tokens: Vec<String> = seen
        .iter()
        .map(|(path, auth)| {
            assert_eq!(path, "/inbox");
            auth.strip_prefix("Bearer at-").map(|n| format!("at-{n}")).unwrap_or_else(|| panic!("not an access token: {auth:?}"))
        })
        .collect();
    assert!(tokens.iter().all(|t| provider.issued(t)), "{tokens:?}");
    let mut distinct = tokens.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "a refreshed token was not the one sent: {tokens:?}");
    assert!(provider.state.lock().unwrap().refreshes >= 4);

    // Alice's two calls ran in one microVM, Carol's in another.
    let (ra, rc) = (results(&m, "and try the rest"), results(&m, "Check my inbox"));
    let instance = |r: &str| field(r, "instance");
    assert!(field(&ra[1], "body").contains("hello from the provider"), "{ra:?}");
    assert_eq!(instance(&ra[1]), instance(&ra[2]), "{ra:?}");
    assert_eq!(instance(&rc[1]), instance(&rc[2]), "{rc:?}");
    assert_ne!(instance(&ra[1]), instance(&rc[1]), "two people shared a microVM");

    // A server that answers with its token leaks it nowhere.
    let leaked = &ra[3];
    assert!(!tokens.iter().any(|t| leaked.contains(t.as_str())), "the token reached the model: {leaked}");
    for r in m.requests() {
        assert!(!tokens.iter().any(|t| r.text().contains(&format!("\"{t}\""))), "a token reached the model");
    }

    // The only way out is the proxy, which admits only the manifest's host.
    let probe = &ra[4];
    assert!(field(probe, "through_proxy").contains("403"), "{probe}");
    assert!(field(probe, "direct").contains("unreachable"), "{probe}");
    assert_ne!(field(probe, "other_port"), "connected", "{probe}");
    assert_eq!(elsewhere.reached.load(Ordering::SeqCst), 0, "a connection got out to a host the manifest does not name");
    assert!(app.logs().contains("the egress proxy refused"), "the refusal is not in the log");

    // Unused, each stops.
    eventually("both microVMs stopped once idle", PATIENCE, || async { (running(&app.state_dir) == 0).then_some(()) }).await;
    run(&alice, "Check my inbox").await;
    let rd = results(&m, "Check my inbox");
    assert_ne!(instance(&rd[1]), instance(&ra[1]), "the stopped microVM answered again");
}

/// A connector reaches its provider only over TLS whose certificate it has
/// checked: a manifest host that answers in the clear, or with a
/// certificate from a CA the appliance does not trust, is sent nothing, its
/// token least of all.
#[tokio::test(flavor = "multi_thread")]
async fn a_connector_sends_its_token_only_over_tls_it_has_checked() {
    let (clear, untrusted) = (ProviderApi::in_the_clear().await, ProviderApi::start().await);
    let (h1, h2) = (clear.host.clone(), untrusted.host.clone());
    let m = FakeModel::start(script(move |c| {
        let (h1, h2) = (h1.clone(), h2.clone());
        async move {
            match c.turn() {
                0 => plan("read the inbox"),
                1 => tool("testapp__fetch", json!({ "host": h1, "path": "/inbox" })),
                2 => tool("testapp__fetch", json!({ "host": h2, "path": "/inbox" })),
                _ => complete("done"),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let provider = FakeOAuthApp::provider_api(3600).await;
    let image = image("magnus-brokered-test");
    // The appliance trusts a CA, but not the one `untrusted` was given.
    let other = TestCa::new("Another test CA");
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 3600, "testapp", &[clear.host.clone(), untrusted.host.clone()], &provider.base, TOOLS);
        trust(cfg, &other);
    })
    .await;
    let alice = app.api("alice");
    sign_in(&alice).await;
    run(&alice, "Read my inbox").await;
    let r = results(&m, "Read my inbox");
    for (i, host) in [(1, &clear), (2, &untrusted)] {
        assert!(r[i].contains("could not reach 127.0.0.1 securely"), "{r:?}");
        assert!(host.seen.lock().unwrap().is_empty(), "a request reached {}", host.host);
        assert!(!String::from_utf8_lossy(&host.raw.lock().unwrap()).contains("Bearer"), "a token went out in the clear");
    }
    // Something did reach each: the TLS hello, and nothing after it.
    assert!(!clear.raw.lock().unwrap().is_empty() && !untrusted.raw.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_erasing_and_the_kill_switch_stop_the_persons_microvm() {
    let api = ProviderApi::start().await;
    let host = api.host.clone();
    let m = FakeModel::start(script(move |c| {
        let host = host.clone();
        async move {
            match c.turn() {
                0 => plan("read the inbox"),
                1 => tool("testapp__fetch", json!({ "host": host, "path": "/inbox" })),
                _ => complete("done"),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let kc = Keycloak::start().await;
    kc.add_person("carol").await;
    kc.configure(&mut app).await;
    let provider = FakeOAuthApp::provider_api(3600).await;
    let image = image("magnus-brokered-test");
    // Never idle here: what stops them is what is tested.
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 3600, "testapp", std::slice::from_ref(&api.host), &provider.base, TOOLS);
        trust(cfg, &api.ca);
    })
    .await;
    let (st, v) = kc.app_sign_in(&app, "carol").await;
    assert_eq!(st, 200, "{v}");
    let mut carol = app.api("carol");
    carol.token = v["token"].as_str().unwrap().to_string();
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    for p in [&alice, &bob, &carol] {
        sign_in(p).await;
        run(p, "Read my inbox").await;
    }
    assert_eq!(running(&app.state_dir), 3);

    assert!(alice.send(reqwest::Method::DELETE, "/v1/connectors/testapp/connection", None).await.0.is_success());
    assert_eq!(running(&app.state_dir), 2, "disconnecting left alice's microVM running");
    assert!(app.logs().contains("testapp: stopped alice's microVM"));

    let (_, people) = control(&app, "GET", "/v1/people", Value::Null).await;
    let sub = people["people"].as_object().unwrap().keys().next().expect("carol's account is bound").clone();
    let (st, v) = control(&app, "POST", &format!("/v1/people/{sub}/erase"), json!({ "actor": "admin:root" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(running(&app.state_dir), 1, "deleting carol left her microVM running");
    assert!(app.logs().contains("testapp: stopped carol's microVM"));

    let (st, v) = alice.post("/v1/kill-switch", json!({ "engaged": true })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(running(&app.state_dir), 0, "the kill switch left bob's microVM running");
    assert!(app.logs().contains("testapp: stopped bob's microVM"));
}

/// The provider did what it was asked, once, and answered with valid JSON
/// that breaks the tool's output schema (#193). Its answer is checked by
/// the server's kit (`send`) or, from a server that does not check it, by
/// the appliance (`send_unchecked`). A read that answers so is an ordinary
/// failure, and the run goes on. A write that answers so may have
/// happened: its outcome is unknown, as after a crash. It is never failed as
/// if nothing happened: the person is told to check, no one is asked (spec
/// rev 140), and the model's identical retry never reaches the provider.
async fn a_write_whose_answer_breaks_its_schema(tool: &'static str, write: &'static str) -> (Appliance, Vec<(String, String)>) {
    let api = ProviderApi::start().await;
    let host = api.host.clone();
    let m = FakeModel::start(script(move |c| {
        let host = host.clone();
        async move {
            let call = |app: &str, path: &str| self::tool(&format!("{app}__{tool}"), json!({ "host": host, "path": path }));
            match c.turn() {
                0 => plan("send the report"),
                1 => call("testread", "/json/read"),
                2 => call("testapp", write),
                // A model told the send failed sends it again.
                3 => call("testapp", write),
                _ => complete("sent"),
            }
        }
    }))
    .await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let provider = FakeOAuthApp::provider_api(3600).await;
    let image = image("magnus-brokered-test");
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 300, "testread", std::slice::from_ref(&api.host), &provider.base, &[(tool, "read")]);
        configure(cfg, &image, 300, "testapp", std::slice::from_ref(&api.host), &provider.base, &[(tool, "reversible_write")]);
        trust(cfg, &api.ca);
    })
    .await;
    let alice = app.api("alice");
    sign_in_to(&alice, "testread").await;
    sign_in_to(&alice, "testapp").await;
    let task = alice.submit("Send the report").await;
    // Whatever asks first (what it read was untrusted), Alice allows, until
    // the person is told the write's outcome is unknown.
    let mut asked = 0;
    let item = loop {
        let item =
            eventually("the task tells alice", PATIENCE, || async { alice.inbox().await.into_iter().find(|i| i["task_id"] == task.as_str() && i["status"] == "open") }).await;
        if item["kind"] != "approval" || asked == 4 {
            break item;
        }
        asked += 1;
        assert!(alice.respond(item["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    };
    alice.wait_settled(&task).await;

    let seen = api.seen.lock().unwrap().clone();
    let count = |p: &str| seen.iter().filter(|(path, _)| path == p).count();
    assert_eq!(count(write), 1, "the write reached the provider again after its answer broke its schema: {seen:?}");
    assert_eq!(count("/json/read"), 1, "{seen:?}");
    // The read failed as any read does, and the run went on.
    let r = results(&m, "Send the report");
    assert!(r[1].contains("does not match its schema"), "{r:?}");
    // The write's outcome is unknown: the person is told to check, and the
    // model is told it is unknown, not that it failed; its retry is refused.
    assert_eq!(item["kind"], "notice", "{item}");
    assert!(item["details"]["tool"].as_str().is_some_and(|t| t.ends_with(tool)), "{item}");
    assert!(item["summary"].as_str().is_some_and(|t| t.contains("not sent again")), "{item}");
    assert!(r.len() >= 4 && r[2].contains("outcome is unknown") && r[3].contains("not made again"), "{r:?}");
    (app, seen)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_whose_answer_breaks_its_schema_at_the_server_is_unknown_never_sent_again() {
    a_write_whose_answer_breaks_its_schema("send", "/json/write").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_whose_answer_breaks_its_schema_at_the_appliance_is_unknown_never_sent_again() {
    a_write_whose_answer_breaks_its_schema("send_unchecked", "/json/write").await;
}

/// The appliance's own check names what is wrong with an answer, a property
/// name too; when the provider put the caller's token there, the message
/// says the token nowhere: not in the logs, the person's card or the task.
#[tokio::test(flavor = "multi_thread")]
async fn a_bad_answer_that_names_the_token_never_shows_it() {
    let (app, seen) = a_write_whose_answer_breaks_its_schema("send_unchecked", "/json/named-by-token").await;
    let token = seen.iter().find(|(p, _)| p == "/json/named-by-token").map(|(_, a)| a.trim_start_matches("Bearer ").to_string()).unwrap();
    assert!(!token.is_empty());
    let alice = app.api("alice");
    let shown = format!("{}\n{}", serde_json::to_string(&alice.inbox().await).unwrap(), serde_json::to_string(&alice.tasks().await).unwrap());
    assert!(!app.logs().contains(&token), "the appliance logged the provider's token from a bad answer");
    assert!(!shown.contains(&token), "the token from a bad answer was shown: {shown}");
}
