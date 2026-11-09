//! Invariant 2: credentials never appear in model context, the sandbox or the log.
//!
//! Prevents: credentials harvested through the agent (Nx s1ngularity, 2025).

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
        assert!(!scan::contains(logs.as_bytes(), s), "{what} is in the appliance's output");
        let files = scan::files_containing(&app.state_dir, s);
        assert!(files.is_empty(), "{what} is in plain text in appliance state (workspaces included): {files:?}");
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
