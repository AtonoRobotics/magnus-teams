//! A person's own rules through the routes: mo, a member with no policy
//! role, writes and reads their own layer; sam, the owner, and ana, an
//! admin, neither read nor write it; and no layer below `org` loosens.

use crate::admin_app::tests::server_with;
use axum::Router;
use reqwest::Method;
use serde_json::json;

const FORBID: &str = r#"forbid(principal, action, resource == Tool::"mail__send");"#;
const PERMIT: &str = r#"permit(principal, action == Action::"execute", resource == Tool::"mail__send");"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_own_rules_are_theirs_and_only_tighten() {
    let s = server_with("", |_| Router::new()).await;
    let (sam, ana, mo) = (s.token("sam").await, s.token("ana").await, s.token("mo").await);
    let put = |layer: &str, cedar: &str| (format!("/v1/policies/{layer}"), Some(json!({ "cedar": cedar })));

    let (path, body) = put("user/mo", FORBID);
    assert_eq!(s.call(&mo, Method::PUT, &path, body.clone()).await.0, 200, "mo could not tighten their own rules");
    for (who, t) in [("the owner", &sam), ("an admin", &ana)] {
        assert_eq!(s.call(t, Method::PUT, &path, body.clone()).await.0, 403, "{who} wrote mo's own rules");
        let (_, v) = s.get(t, "/v1/policies").await;
        assert!(v["layers"].get("user/mo").is_none(), "{who} read mo's own rules: {v}");
    }
    assert_eq!(s.get(&mo, "/v1/policies").await.1["layers"]["user/mo"], FORBID);

    let (path, body) = put("user/mo", PERMIT);
    let (st, v) = s.call(&mo, Method::PUT, &path, body).await;
    assert_eq!(st, 422, "a permit in mo's own layer was taken: {v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("org"), "the refusal does not name the layer above: {v}");
    assert_eq!(s.get(&mo, "/v1/policies").await.1["layers"]["user/mo"], FORBID, "a refused write changed the layer");

    // The owner keeps the organisation's and the groups' layers; a group's
    // still only tightens.
    assert_eq!(s.post(&sam, "/v1/admin/step-up", json!({ "password": "a-long-admin-password" })).await.0, 200);
    let (path, body) = put("group/staff", PERMIT);
    assert_eq!(s.call(&sam, Method::PUT, &path, body).await.0, 422, "a group's layer loosened the organisation's");
    let (path, body) = put("org", PERMIT);
    assert_eq!(s.call(&sam, Method::PUT, &path, body).await.0, 200);
    let sets = s.k.policy_sets();
    assert!(sets.iter().all(|l| l["id"] != "user/mo"), "the admin's editor lists mo's own rules: {sets:?}");
    s.done().await;
}
