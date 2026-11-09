//! Recipient provenance from the organisation's directory (approvals spec,
//! #211): under both taints, a send to an address the realm holds passes
//! with no card; one it does not hold asks.

use magnus_invariants::{mcp, *};
use serde_json::json;

/// A stub identity provider: discovery, a token endpoint, and realm users
/// by email, as the status client.
async fn stub_idp() -> (String, String) {
    let idp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", idp.local_addr().unwrap());
    let iss = format!("{base}/realms/org");
    let (i, b) = (iss.clone(), base.clone());
    let app = axum::Router::new()
        .route(
            "/realms/org/.well-known/openid-configuration",
            axum::routing::get(move || async move { axum::Json(json!({ "issuer": i, "token_endpoint": format!("{b}/realms/org/token") })) }),
        )
        .route("/realms/org/token", axum::routing::post(move || async { axum::Json(json!({ "access_token": "t" })) }))
        .route(
            "/admin/realms/org/users",
            axum::routing::get(|q: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                let hit = q.get("email").is_some_and(|e| e == "bob@corp.example") && q.get("exact").is_some_and(|e| e == "true");
                axum::Json(if hit { json!([{ "id": "u1", "email": "bob@corp.example" }]) } else { json!([]) })
            }),
        );
    tokio::spawn(async move { axum::serve(idp, app).await });
    (base, iss)
}

/// A colleague's address passes under both taints with no card, naming
/// nothing in the person's words and found in no sent mail; an outside
/// one asks (#211).
#[tokio::test(flavor = "multi_thread")]
async fn a_colleagues_address_passes_and_an_outside_one_asks() {
    let (_base, iss) = stub_idp().await;
    let id_only = json!({ "type": "object", "properties": { "draft_id": { "type": "string" } } });
    let drafts = FakeMcp::builder()
        .world(
            "drafts",
            json!({
                "d1": { "to": "bob@corp.example", "subject": "Q3", "body": "Confidential: revenue 41.7M" },
                "d2": { "to": "mallory@evil.example", "subject": "fwd", "body": "Confidential: revenue 41.7M" }
            }),
        )
        .honor_idempotency(true)
        .tool("get_draft", "Read a draft.", id_only.clone(), |w, a, _| {
            let id = a["draft_id"].as_str().unwrap_or_default();
            match w["drafts"].get(id) {
                Some(d) => mcp::ok(json!({ "draft_id": id, "message": d })),
                None => mcp::err("no such draft"),
            }
        })
        .tool("send_draft", "Send a draft.", id_only, |_, _, _| mcp::ok(json!({ "status": "sent" })))
        .start()
        .await;
    // No sent declaration: only the directory can clear Bob.
    let spec = Spec::default().app("drafts", &drafts, "public", "private+untrusted", &[("get_draft", "read"), ("send_draft", "irreversible")]).destinations(
        "drafts",
        "send_draft",
        "get_draft",
        "draft_id",
        &["/message/to"],
        &["/message/subject", "/message/body"],
    );
    let model = FakeModel::start(steps(vec![
        contract_unchecked("send the Q3 draft", "it is sent"),
        tool("drafts__get_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d1" })),
        tool("drafts__send_draft", json!({ "draft_id": "d2" })),
        fail("stopping"),
    ]))
    .await;
    let mut app = Appliance::start(spec, &model).await;
    assert!(app.api("alice").put_secret("status-secret", "s3cret").await.is_success());
    magnus_scratch::reconfigure(&mut app, |c| {
        let mut o = toml::Table::new();
        o.insert("issuer".into(), iss.into());
        o.insert("client_id".into(), "magnus".into());
        o.insert("status_client".into(), "status".into());
        o.insert("status_credential".into(), "status-secret".into());
        let mut s = toml::Table::new();
        s.insert("oidc".into(), toml::Value::Table(o));
        c.insert("sign_in".into(), toml::Value::Table(s));
    })
    .await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", &org_policy(&permit_execute("drafts__send_draft"))).await.is_success());
    let id = alice.submit("Send the Q3 draft.").await;

    eventually("the draft to the colleague was sent", PATIENCE, || async { (drafts.effects("send_draft") == 1).then_some(()) }).await;
    assert_eq!(alice.task(&id).await["taint"], json!({ "untrusted": true, "private": true }));
    let card = alice.wait_item(&id, "approval").await;
    assert!(card.to_string().contains("mallory@evil.example"), "the card does not show the outside address: {card}");
    assert_eq!(drafts.effects("send_draft"), 1, "the outside address was sent to without the person");
}
