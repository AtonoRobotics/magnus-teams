//! A connector Magnus builds is one person's, needs their approval, runs as
//! it was approved, and goes completely when they delete it.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use magnus_invariants::*;
use serde_json::{json, Value};
use std::time::Duration;

const TOOL_WAIT: Duration = Duration::from_secs(3);

fn definition(base: &str) -> Value {
    json!({
        "title": "Weather", "description": "Forecasts for a place", "base_url": base, "token": true,
        "tools": [{ "name": "forecast", "description": "A forecast", "method": "GET", "path": "/forecast/{city}" }],
    })
}

fn connector_named(list: &Value, title: &str) -> Option<String> {
    let cards = list.as_array().or(list["connectors"].as_array())?;
    cards.iter().find(|c| c["title"] == title && c["generated"] == true)?["name"].as_str().map(String::from)
}

/// Waits for a task to settle: checking is internal, and nothing is put to
/// the person to accept (spec rev 140).
async fn settled(who: &Api, task: &str) {
    let t = who.wait_settled(task).await;
    assert_ne!(t["waiting_on"], "review", "criteria were put to the person: {t}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_generated_connector_is_approved_private_to_its_person_and_gone_when_deleted() {
    let computerd = Computerd::start();
    let secret = canary("api-token");
    let site = FakeWeb::start(&[("/forecast/oslo", "cold and clear")]).await;
    let model =
        FakeModel::start(steps(vec![contract_unchecked("build it", "built"), tool("connector_build", json!({ "definition": definition(&site.base) })), complete("built")])).await;
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (_, made) = alice.post("/v1/computers", json!({})).await;
    let computer = made["id"].as_str().unwrap().to_string();
    let (code, started) = alice.post(&format!("/v1/computers/{computer}/start"), json!({})).await;
    assert_eq!(code, 200, "{started}");

    let task = alice.submit("Build me a weather connector.").await;
    let ask = alice.wait_item(&task, "approval").await;
    never("it was built without approval", TOOL_WAIT, || async { connector_named(&alice.get("/v1/connectors").await.1, "Weather").is_some() }).await;
    assert!(alice.respond(ask["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
    settled(&alice, &task).await;

    let name = connector_named(&alice.get("/v1/connectors").await.1, "Weather").expect("it is listed with their connectors");
    assert!(connector_named(&carol.get("/v1/connectors").await.1, "Weather").is_none(), "another person sees it");
    assert_eq!(carol.get(&format!("/v1/connectors/{name}/definition")).await.0, 404, "another person read its definition");
    let (code, downloaded) = alice.get(&format!("/v1/connectors/{name}/definition")).await;
    assert_eq!((code.as_u16(), downloaded["base_url"].as_str()), (200, Some(site.base.as_str())), "{downloaded}");
    let kept = format!("/v1/computers/{computer}/files?path=/workspace/connectors/{name}.json");
    let (code, file) = alice.get(&kept).await;
    assert_eq!(code, 200, "it is not kept on the volume: {file}");

    assert_eq!(carol.put(&format!("/v1/connectors/{name}/token"), json!({ "token": "x" })).await.0, 404, "another person set its token");
    assert!(alice.put(&format!("/v1/connectors/{name}/token"), json!({ "token": secret })).await.0.is_success());
    model.set_script(steps(vec![contract_unchecked("read", "read"), tool(&format!("{name}__forecast"), json!({ "city": "oslo" })), complete("read")]));
    let read = alice.submit("What is the forecast?").await;
    settled(&alice, &read).await;
    let hit = site.seen().into_iter().find(|s| s.path == "/forecast/oslo").expect("the connector called its address");
    assert_eq!(hit.authorization.as_deref(), Some(format!("Bearer {secret}").as_str()), "the call did not carry the token");
    let seen_by_model = format!("{:?}", model.requests());
    assert!(seen_by_model.contains("cold and clear"), "the answer did not reach the agent");
    assert!(!seen_by_model.contains(&secret), "the token reached the agent");

    let (code, rule) = alice.post("/v1/rules", json!({ "connector": name, "tool": "forecast", "destinations": [site.base.clone()] })).await;
    assert!(code.is_success(), "{rule}");
    let before = alice.get("/v1/rules").await.1;
    assert!(before["rules"].as_array().unwrap().iter().any(|r| r["compiled"]["connector"] == name.as_str()), "the rule to remove was not made: {before}");

    assert_eq!(carol.delete(&format!("/v1/connectors/{name}/generated")).await.0, 404, "another person deleted it");
    let (code, gone) = alice.delete(&format!("/v1/connectors/{name}/generated")).await;
    assert_eq!((code.as_u16(), gone["tokens_revoked"].as_u64()), (200, Some(1)), "{gone}");
    assert!(connector_named(&alice.get("/v1/connectors").await.1, "Weather").is_none(), "it is still listed");
    let rules = alice.get("/v1/rules").await.1;
    assert!(rules["rules"].as_array().unwrap().iter().all(|r| r["compiled"]["connector"] != name.as_str()), "its rule stayed: {rules}");
    let listing = alice.get(&format!("/v1/computers/{computer}/listing?path=/workspace/connectors")).await.1;
    assert!(!listing.to_string().contains(&name), "its file stayed on the volume: {listing}");

    let calls = site.seen().len();
    model.set_script(steps(vec![contract_unchecked("read", "read"), tool(&format!("{name}__forecast"), json!({ "city": "oslo" })), complete("read")]));
    let after = alice.submit("And now?").await;
    settled(&alice, &after).await;
    assert_eq!(site.seen().len(), calls, "a deleted connector was still called");
}
