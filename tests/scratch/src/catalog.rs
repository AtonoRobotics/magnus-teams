//! The catalog's connectors whose provider lets Magnus register itself
//! (`registration = "dcr"`), against the provider: no fakes (owner,
//! 2026-11-01: "No fakes. Test everything against real."). Each such
//! connector's own test (`tests/<provider>.rs`) calls these.

use magnus_invariants::*;
use serde_json::json;
use std::collections::HashMap;

/// A catalog manifest as the repository ships it.
pub fn manifest(id: &str) -> toml::Table {
    let path = magnus_invariants::appliance::workspace_root().join(format!("catalog/{id}.toml"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).parse().unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The connector is in a person's Library from the start, ready to connect
/// with nothing set up, and her sign-in starts on the provider's own page:
/// Magnus finds the live server's sign-in from the server, registers itself
/// there for the address on her computer the answer comes back to, and asks
/// for exactly the access its manifest names, with PKCE, for that server.
/// The provider answers the address Magnus built with its sign-in or
/// consent page (or a redirect to one), not an error. Returns that address.
pub async fn registers_itself_and_signs_in_at(id: &str, host: &str) -> url::Url {
    let m = manifest(id);
    assert_eq!(m["registration"].as_str(), Some("dcr"), "{id}: Magnus does not register itself");
    let endpoint = m["endpoint"].as_str().unwrap().to_string();
    let scopes: Vec<String> = m["scopes"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let carol = app.api("carol");

    let (_, list) = carol.get("/v1/connectors").await;
    let c = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == id).cloned().unwrap_or_else(|| panic!("{id} is not in carol's Library: {list}"));
    assert_eq!((c["title"].as_str(), c["state"].as_str(), c["catalog"].as_bool(), c["ready"].as_bool()), (m["name"].as_str(), Some("available"), Some(true), Some(true)), "{c}");

    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect = format!("http://127.0.0.1:{}/callback", callback.local_addr().unwrap().port());
    let (st, v) = carol.post(&format!("/v1/connectors/{id}/sign-in"), json!({ "redirect_uri": redirect })).await;
    assert!(st.is_success(), "{id}: Magnus could not register itself at the live server's sign-in: {v}");
    let u = url::Url::parse(v["authorize_url"].as_str().unwrap_or_else(|| panic!("{v}"))).unwrap();
    assert_eq!(u.host_str(), Some(host), "{id}: {u}");
    let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
    assert!(q.get("client_id").is_some_and(|c| !c.is_empty()), "{id}: no client registered: {u}");
    assert_eq!(q.get("redirect_uri"), Some(&redirect), "{id}: {u}");
    assert_eq!(q.get("code_challenge_method").map(String::as_str), Some("S256"), "{id}: {u}");
    assert_eq!(q.get("resource"), Some(&endpoint), "{id}: {u}");
    assert_eq!(q.get("scope").map(|s| s.split(' ').map(String::from).collect::<Vec<_>>()), Some(scopes), "{id}: not exactly the manifest's access: {u}");

    let page = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) Magnus catalog test")
        .build()
        .unwrap()
        .get(u.as_str())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{id}: {e}"));
    let status = page.status();
    let to = page.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or_default().to_string();
    let body = page.text().await.unwrap_or_default();
    assert!(status.is_success() || status.is_redirection(), "{id}: the provider answered {status}: {}", &body[..body.len().min(600)]);
    assert!(!to.contains("error="), "{id}: the provider refused the sign-in request: {to}");
    u
}

/// The tools a live server lists to anyone, without a sign-in.
pub async fn tools_listed_without_sign_in(endpoint: &str) -> Vec<String> {
    let http = reqwest::Client::new();
    let call = |id: u32, method: &str, session: Option<String>| {
        let mut r = http
            .post(endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .json(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": if method == "initialize" { json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "magnus", "version": "test" } }) } else { json!({}) } }));
        if let Some(s) = session {
            r = r.header("mcp-session-id", s);
        }
        r.send()
    };
    let init = call(0, "initialize", None).await.unwrap_or_else(|e| panic!("{endpoint}: {e}"));
    let session = init.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
    let raw = call(1, "tools/list", session).await.unwrap_or_else(|e| panic!("{endpoint}: {e}")).text().await.unwrap();
    // A streamed answer carries its message in `data:` lines.
    let body = if raw.trim_start().starts_with('{') { raw } else { raw.lines().filter_map(|l| l.strip_prefix("data:")).collect::<String>() };
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{endpoint}: {e}: {body}"));
    let mut live: Vec<String> = v["result"]["tools"].as_array().unwrap_or_else(|| panic!("{endpoint}: {v}")).iter().filter_map(|t| t["name"].as_str().map(String::from)).collect();
    live.sort();
    live
}
