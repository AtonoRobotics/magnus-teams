//! Knowledge in the admin console (admin console spec, Knowledge; the
//! invariant tests "An admin edits an ontology type and a memory entry, sees
//! provenance on both, and rolls each back" and "An agent's change to org
//! memory appears in Activity, and reverting it restores the prior state").
//!
//! An agent writes a type and an entry to the org context bundle, then
//! changes the entry. Root:
//! - sees each with its provenance (the agent, its run, its sources);
//! - approves the type as stable and edits the entry, each a new version;
//! - rolls each back;
//! - sees the agent's change in Activity and undoes it;
//! - cannot delete a type in use, and merges it into another instead;
//! - reads the map as a person's agent receives it.
//!
//! Once through the kernel's own operations (the control socket), and once
//! through the console in a real browser against the local store.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

const TASK: &str = "Map our customers into the bundle.";

fn gen() -> Value {
    json!({ "by": "magnus/0.2.4", "at": "2026-11-02T10:00:00+00:00" })
}

fn customer() -> Value {
    json!({ "path": "/types/customer.md", "frontmatter": { "type": "Entity Type", "title": "Customer", "description": "A company that buys from us.", "generated": gen(), "status": "draft",
        "system_of_record": "/sources/hubspot.md", "identity_keys": ["domain"], "relations": [{ "type": "owned_by", "to": "/types/team.md", "cardinality": "one" }], "cedar_entity": "Customer", "scope": "org" }, "body": "A company with a closed deal." })
}

fn acme(body: &str) -> Value {
    json!({ "path": "/entities/customer/acme.md", "frontmatter": { "type": "Customer", "title": "Acme", "resource": "/sources/hubspot.md", "sources": [{ "resource": "/sources/hubspot.md" }],
        "generated": gen(), "identity": { "domain": "acme.example" }, "relations": [], "scope": "org" }, "body": body })
}

fn model() -> impl Fn(Conversation) -> std::pin::Pin<Box<dyn std::future::Future<Output = Reply> + Send>> + Send + Sync + 'static {
    |c: Conversation| {
        Box::pin(async move {
            if !c.body.to_string().contains(TASK) {
                return Reply::Text(String::new());
            }
            let steps = [
                contract_unchecked("map the customers", "the bundle has them"),
                tool("bundle_write", customer()),
                tool("bundle_write", acme("Acme buys the blue plan.")),
                tool("bundle_write", acme("Acme buys the red plan.")),
                tool(
                    "bundle_write",
                    json!({ "path": "/index.md", "frontmatter": { "okf_version": "0.2" }, "body": "# Customers\n* [Customer](/types/customer.md) - who buys\n* [Acme](/entities/customer/acme.md) - a customer\n" }),
                ),
                tool("task_complete", json!({ "summary": "Mapped.", "deliverable": [{ "type": "text", "text": "Mapped." }] })),
            ];
            steps.get(c.turn()).cloned().unwrap_or_else(|| tool("task_fail", json!({ "reason": "done" })))
        })
    }
}

/// One request over the kernel's control socket: its status and answer.
async fn control(app: &Appliance, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(app.state_dir.join("control.sock")).await.expect("the kernel's control socket");
    let b = body.map(|b| b.to_string()).unwrap_or_default();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: kernel\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn q(path: &str) -> String {
    url::form_urlencoded::byte_serialize(path.as_bytes()).collect()
}

async fn agent_maps_the_customers(app: &Appliance) {
    let alice = app.api("alice");
    let t = alice.submit(TASK).await;
    alice.wait_item(&t, "review").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_kernel_keeps_every_knowledge_change_and_root_rolls_any_back() {
    let m = FakeModel::start(script(model())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    agent_maps_the_customers(&app).await;
    let sub = "a1b2c3";

    // Provenance: the agent, its run, its sources.
    let (st, d) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/entities/customer/acme.md")), None).await;
    assert_eq!(st, 200, "{d}");
    assert_eq!(d["versions"].as_array().unwrap().len(), 2, "{d}");
    let first = &d["versions"][1];
    assert!(first["by"].as_str().unwrap().starts_with("magnus/") && first["run"].is_string(), "{d}");
    assert_eq!(d["sources"], json!(["/sources/hubspot.md"]));
    assert_eq!(d["body"], "Acme buys the red plan.");

    // Only an admin makes a type stable: as one, it is version 2.
    let (_, t) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/types/customer.md")), None).await;
    let mut fm = t["frontmatter"].clone();
    fm["status"] = json!("stable");
    let (st, v) = control(&app, "POST", "/v1/admin/knowledge/doc", Some(json!({ "sub": sub, "path": "/types/customer.md", "frontmatter": fm, "body": t["body"] }))).await;
    assert_eq!((st, v["version"].as_u64()), (200, Some(2)), "{v}");
    let (_, t) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/types/customer.md")), None).await;
    assert_eq!((t["frontmatter"]["status"].as_str(), t["versions"][0]["by"].as_str()), (Some("stable"), Some("human:a1b2c3")), "{t}");
    assert_eq!(t["entities"], json!(["/entities/customer/acme.md"]), "{t}");
    // A malformed edit is refused with the reason, and nothing is stored.
    let (st, v) =
        control(&app, "POST", "/v1/admin/knowledge/doc", Some(json!({ "sub": sub, "path": "/types/customer.md", "frontmatter": { "type": "Entity Type" }, "body": "" }))).await;
    assert_eq!(st, 400, "{v}");
    // Rolled back to version 1: draft again, as version 3.
    let (st, v) = control(&app, "POST", "/v1/admin/knowledge/rollback", Some(json!({ "sub": sub, "path": "/types/customer.md", "version": 1 }))).await;
    assert_eq!((st, v["version"].as_u64()), (200, Some(3)), "{v}");
    let (_, t) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/types/customer.md")), None).await;
    assert_eq!(t["frontmatter"]["status"], "draft");

    // The agent's change to the entry is in Activity, with what it replaced.
    let (_, a) = control(&app, "GET", "/v1/admin/knowledge/activity?agents=1", None).await;
    let changed = a["changes"].as_array().unwrap().iter().find(|c| c["path"] == "/entities/customer/acme.md" && c["change"] == "updated").cloned().unwrap_or_else(|| panic!("{a}"));
    assert!(changed["by"].as_str().unwrap().starts_with("magnus/") && changed["run"].is_string(), "{changed}");
    assert!(a["changes"].as_array().unwrap().iter().all(|c| !c["by"].as_str().unwrap_or_default().starts_with("human:")), "an admin's change is listed as an agent's");
    let seq = changed["seq"].as_i64().unwrap();
    let (_, ch) = control(&app, "GET", &format!("/v1/admin/knowledge/activity/{seq}"), None).await;
    assert!(ch["before"].as_str().unwrap().contains("blue plan") && ch["after"].as_str().unwrap().contains("red plan"), "{ch}");
    // Undone, the entry is as it was before the agent's change.
    let (st, v) = control(&app, "POST", "/v1/admin/knowledge/revert", Some(json!({ "sub": sub, "seqs": [seq] }))).await;
    assert_eq!(st, 200, "{v}");
    let (_, d) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/entities/customer/acme.md")), None).await;
    assert_eq!((d["body"].as_str(), d["version"].as_u64()), (Some("Acme buys the blue plan."), Some(3)), "{d}");

    // A type in use is not deleted: it is merged into another.
    let (st, v) = control(&app, "DELETE", "/v1/admin/knowledge/doc", Some(json!({ "sub": sub, "path": "/types/customer.md" }))).await;
    assert_eq!(st, 409, "{v}");
    let client = json!({ "type": "Entity Type", "title": "Client", "description": "Anyone who buys.", "generated": { "by": "human:a1b2c3", "at": "2026-11-02T12:00:00+00:00" }, "status": "draft", "system_of_record": "/sources/hubspot.md", "identity_keys": ["domain"], "relations": [], "cedar_entity": "Client", "scope": "org" });
    let (st, v) = control(&app, "POST", "/v1/admin/knowledge/doc", Some(json!({ "sub": sub, "path": "/types/client.md", "frontmatter": client, "body": "Who buys." }))).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = control(&app, "POST", "/v1/admin/knowledge/merge", Some(json!({ "sub": sub, "from": "/types/customer.md", "into": "/types/client.md" }))).await;
    assert_eq!((st, v["entities_moved"].as_u64()), (200, Some(1)), "{v}");
    let (st, d) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/entities/client/acme.md")), None).await;
    assert_eq!((st, d["frontmatter"]["type"].as_str()), (200, Some("Client")), "{d}");
    let (st, _) = control(&app, "GET", &format!("/v1/admin/knowledge/doc?path={}", q("/types/customer.md")), None).await;
    assert_eq!(st, 404, "the merged type is still there");
    // Its versions are still in the log, to bring back.
    let (st, v) = control(&app, "GET", &format!("/v1/admin/knowledge/version?path={}&version=1", q("/types/customer.md")), None).await;
    assert_eq!((st, v["frontmatter"]["title"].as_str()), (200, Some("Customer")), "{v}");

    // The map as a person's agent receives it.
    let (st, m) = control(&app, "GET", "/v1/admin/knowledge/assembled?person=bob", None).await;
    assert_eq!(st, 200, "{m}");
    assert!(m["text"].as_str().is_some_and(|t| t.contains("# Customers")), "{m}");
}

#[tokio::test(flavor = "multi_thread")]
async fn root_oversees_corrects_and_rolls_back_the_organisations_knowledge_from_the_console() {
    let m = FakeModel::start(script(model())).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    agent_maps_the_customers(&app).await;
    let mine = format!("{}/32", lan_ip());
    // Long enough for a browser's sign-in to hand back its session (that
    // has taken 23 seconds after the password on a loaded machine).
    let lapse = 60;
    let p = prepare(&app, &[&mine], &[("step_up_seconds", lapse.into())]);
    let url = p.url();
    let c = launch(p).await;
    let b = browser(&c, false);
    let session = || async {
        let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
        let me: Value = with_cookie(b.get(format!("{url}/api/v1/session")), &sid).send().await.unwrap().json().await.unwrap();
        (sid, me["csrf"].as_str().unwrap().to_string())
    };
    let send = |s: (String, String), method: Method, path: String, body: Option<Value>| {
        let (b, url) = (b.clone(), url.clone());
        async move {
            let mut rq = with_cookie(b.request(method, format!("{url}/api/v1{path}")), &s.0).header("x-csrf-token", &s.1);
            if let Some(x) = body {
                rq = rq.json(&x);
            }
            let r = rq.send().await.unwrap();
            let st = r.status();
            (st, r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let s = session().await;

    // Ontology: the agent's type, with its relation and its entry.
    let (st, v) = send(s.clone(), Method::GET, "/ontology/types".into(), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["documents"].as_array().unwrap().iter().any(|d| d["title"] == "Customer"), "{v}");
    let (_, t) = send(s.clone(), Method::GET, "/ontology/types/customer".into(), None).await;
    assert_eq!(
        (t["relations"][0]["type"].as_str(), t["relations"][0]["cardinality"].as_str(), t["entities"][0]["id"].as_str()),
        (Some("owned_by"), Some("one"), Some("entities~customer~acme")),
        "{t}"
    );
    assert!(t["versions"][0]["by"].as_str().unwrap().starts_with("magnus/") && t["versions"][0]["run"].is_string(), "the type's provenance: {t}");
    // Approved as stable, a relation added, each a version; rolled back.
    let (st, v) = send(s.clone(), Method::PATCH, "/ontology/types/customer".into(), Some(json!({ "status": "stable" }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let rels = json!([{ "type": "owned_by", "to": "/types/team.md", "cardinality": "one" }, { "type": "parent", "to": "/types/customer.md", "cardinality": "many" }]);
    let (st, v) = send(s.clone(), Method::PATCH, "/ontology/types/customer".into(), Some(json!({ "relations": rels }))).await;
    assert_eq!((st, v["version"].as_u64()), (StatusCode::OK, Some(3)), "{v}");
    let (st, v) = send(s.clone(), Method::POST, "/ontology/types/customer/rollback".into(), Some(json!({ "version": 1 }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, t) = send(s.clone(), Method::GET, "/ontology/types/customer".into(), None).await;
    assert_eq!((t["frontmatter"]["status"].as_str(), t["relations"].as_array().map(Vec::len)), (Some("draft"), Some(1)), "{t}");
    // In use: not deleted.
    let (st, _) = send(s.clone(), Method::DELETE, "/ontology/types/customer".into(), None).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Memory: the entry with its provenance, edited and rolled back.
    let (_, mem) = send(s.clone(), Method::GET, "/memory".into(), None).await;
    let e = mem["documents"].as_array().unwrap().iter().find(|d| d["id"] == "entities~customer~acme").cloned().unwrap_or_else(|| panic!("{mem}"));
    assert_eq!(e["sources"], json!(["/sources/hubspot.md"]));
    let (_, d) = send(s.clone(), Method::GET, "/memory/entities~customer~acme".into(), None).await;
    assert!(d["versions"][0]["by"].as_str().unwrap().starts_with("magnus/"), "the entry's provenance: {d}");
    let text = d["text"].as_str().unwrap().replace("red plan", "green plan");
    let (st, v) = send(s.clone(), Method::PATCH, "/memory/entities~customer~acme".into(), Some(json!({ "text": text }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, d) = send(s.clone(), Method::GET, "/memory/entities~customer~acme".into(), None).await;
    assert_eq!(d["body"], "Acme buys the green plan.");
    let (st, _) = send(s.clone(), Method::POST, "/memory/entities~customer~acme/rollback".into(), Some(json!({ "version": 2 }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, d) = send(s.clone(), Method::GET, "/memory/entities~customer~acme".into(), None).await;
    assert_eq!(d["body"], "Acme buys the red plan.");

    // Activity: the agent's change, with its before and after; undone.
    let (_, a) = send(s.clone(), Method::GET, "/activity".into(), None).await;
    let changed = a["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "/entities/customer/acme.md" && c["change"] == "updated" && c["by"].as_str().is_some_and(|b| b.starts_with("magnus/")))
        .cloned()
        .unwrap_or_else(|| panic!("{a}"));
    let seq = changed["seq"].as_i64().unwrap();
    let (_, ch) = send(s.clone(), Method::GET, format!("/activity/{seq}"), None).await;
    assert!(ch["before"].as_str().unwrap().contains("blue plan") && ch["after"].as_str().unwrap().contains("red plan"), "{ch}");
    let (st, v) = send(s.clone(), Method::POST, format!("/activity/{seq}/revert"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, d) = send(s.clone(), Method::GET, "/memory/entities~customer~acme".into(), None).await;
    assert_eq!(d["body"], "Acme buys the blue plan.", "reverting the agent's change did not restore the entry");

    // Context: the map, and as Bob's agent receives it.
    let (_, ctx) = send(s.clone(), Method::GET, "/context/map".into(), None).await;
    assert!(ctx["documents"].as_array().unwrap().iter().any(|d| d["path"] == "/index.md"), "{ctx}");
    let (st, seen) = send(s.clone(), Method::GET, "/context/assembled?person=bob".into(), None).await;
    assert_eq!(st, StatusCode::OK, "{seen}");
    assert!(seen["text"].as_str().is_some_and(|t| t.contains("# Customers")), "{seen}");

    // In bulk, and a batch of changes undone, only with a fresh password.
    tokio::time::sleep(std::time::Duration::from_secs(lapse as u64 + 1)).await;
    for (path, body) in [("/memory/bulk", json!({ "action": "rollback", "ids": ["entities~customer~acme"] })), ("/activity/revert-batch", json!({ "seqs": [seq] }))] {
        let (st, v) = send(s.clone(), Method::POST, path.into(), Some(body)).await;
        assert_eq!((st, v["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("step_up_required")), "{path}: {v}");
    }
    let fresh = session().await;
    let (st, v) =
        send(fresh.clone(), Method::POST, "/memory/bulk".into(), Some(json!({ "action": "delete", "ids": ["entities~customer~acme", "entities~customer~nobody"] }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let res = v["results"].as_array().unwrap();
    assert_eq!((res[0]["done"].as_bool(), res[1]["done"].as_bool()), (Some(true), Some(false)), "{v}");

    // Each change is in the audit log.
    let (_, audit) = send(fresh.clone(), Method::GET, "/audit".into(), None).await;
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in
        ["ontology_type_status_changed", "ontology_relations_changed", "ontology_type_rolled_back", "memory_changed", "memory_rolled_back", "knowledge_reverted", "memory_deleted"]
    {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }

    // The screens, in every look and at phone width.
    for screen in ["#/knowledge/ontology/customer", "#/knowledge/context", "#/knowledge/memory", "#/knowledge/activity"] {
        sign_in_and_visit(&url, c.addr, "Alice", PASSWORD, "admin", false, Some(screen)).await.unwrap_or_else(|e| panic!("{e}"));
    }
}
