//! Google's connectors, against Google: no fakes (owner, 2026-11-01: "No
//! fakes. Test everything against real."). The catalog's Gmail, Calendar,
//! Drive and Chat are in every Library already set up, and sign in through
//! Alpha Vector's Desktop OAuth client with Google (owner: "connectors
//! should be built for developer org accounts and users have their own
//! oauth sign in"; connectors spec rev 46: nobody sets a connector up).
//! This finds each live server's sign-in from the server itself and starts
//! a person's sign-in at Google.

use magnus_invariants::*;
use serde_json::{json, Value};

const SERVERS: [(&str, &str, &str); 4] = [
    ("google-gmail", "Gmail", "https://www.googleapis.com/auth/gmail.modify"),
    ("google-calendar", "Google Calendar", "https://www.googleapis.com/auth/calendar"),
    ("google-drive", "Google Drive", "https://www.googleapis.com/auth/drive.readonly"),
    ("google-chat", "Google Chat", "https://www.googleapis.com/auth/chat.messages"),
];

fn client() -> (String, String) {
    // Alpha Vector's Google client ID is public, kept in the catalog. These
    // tests never reach the token exchange, the one step that uses the
    // client secret, so they provision a stand-in for it, as Alpha
    // Vector's deployment provisions the real one.
    let path = magnus_invariants::appliance::workspace_root().join("catalog/registrations.toml");
    let r: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let id = r["google"]["client_id"].as_str().expect("catalog/registrations.toml names Alpha Vector's Google client").to_string();
    (id, "not-used-before-the-token-exchange".into())
}

#[tokio::test(flavor = "multi_thread")]
async fn google_s_connectors_are_in_every_library_already_set_up_and_sign_in_at_google() {
    let (id, secret) = client();
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));

    // In everyone's Library from the start; until Alpha Vector's secret is
    // provisioned (a deployment step, never typed by anyone), the card says
    // it is not ready rather than offering a sign-in that cannot finish.
    let (_, list) = carol.get("/v1/connectors").await;
    let gmail = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == "google-gmail").cloned().unwrap_or_else(|| panic!("{list}"));
    assert_eq!((gmail["state"].as_str(), gmail["ready"].as_bool()), (Some("available"), Some(false)), "{gmail}");
    assert!(!carol.post("/v1/connectors/google-gmail/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await.0.is_success());

    // Only the owner's credentials (Alpha Vector's deployment) provision it;
    // it is kept in the vault and never shown again, nor logged.
    assert_eq!(carol.put("/v1/registrations/google", json!({ "client_secret": secret })).await.0, 403);
    assert_eq!(alice.put("/v1/registrations/google", json!({ "client_secret": secret })).await.0, 204);
    assert_eq!(alice.get("/v1/registrations").await.0, 404, "registrations are listed for someone to set");
    let (_, all) = alice.get("/v1/connectors").await;
    assert!(!all.to_string().contains(&secret), "the client secret is shown: {all}");
    let log = app.db.admin().await.query_one("SELECT string_agg(body::text, ' ') FROM public.events", &[]).await.unwrap().get::<_, Option<String>>(0).unwrap_or_default();
    assert!(!log.contains(&secret), "the client secret is in the log");

    // Each is in carol's Library, in plain words, and her sign-in starts at
    // Google with the organisation's client, the access its manifest names,
    // PKCE, and a refresh token that lasts; the answer comes to her computer.
    let (_, list) = carol.get("/v1/connectors").await;
    let callback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = callback.local_addr().unwrap().port();
    for (name, title, scope) in SERVERS {
        let c = list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("{name} is not in carol's Library: {list}"));
        assert_eq!((c["title"].as_str(), c["state"].as_str(), c["catalog"].as_bool(), c["ready"].as_bool()), (Some(title), Some("available"), Some(true), Some(true)), "{c}");
        let (st, v) = carol.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": format!("http://127.0.0.1:{port}/callback") })).await;
        assert!(st.is_success(), "{name}: Magnus could not find the live server's sign-in: {v}");
        let u = url::Url::parse(v["authorize_url"].as_str().unwrap()).unwrap();
        assert_eq!(u.host_str(), Some("accounts.google.com"), "{name}: {u}");
        let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q.get("client_id"), Some(&id), "{name}: {u}");
        assert!(q.get("scope").is_some_and(|s| s.split(' ').any(|x| x == scope)), "{name} does not ask for {scope}: {u}");
        assert_eq!((q.get("code_challenge_method").map(String::as_str), q.get("access_type").map(String::as_str)), (Some("S256"), Some("offline")), "{name}: {u}");
        assert!(q.get("redirect_uri").is_some_and(|r| r.starts_with("http://127.0.0.1:")), "{name}: {u}");
        // Google answers the address Magnus built with its sign-in page, not an error.
        let page = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap().get(u.as_str()).send().await.unwrap();
        let to = page.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or_default().to_string();
        assert!(page.status().is_success() || page.status().is_redirection(), "{name}: Google answered {}", page.status());
        assert!(!to.contains("error=") && !to.contains("/signin/oauth/error"), "{name}: Google refused the sign-in request: {to}");
    }
}

/// Tool drift (connectors spec, Required tests): each remote Google
/// manifest names exactly the tools its live server lists, so none is
/// hidden for want of a class and none is classed that does not exist. The
/// servers list their tools without a sign-in. Gmail, Calendar and Chat are
/// brokered: each image's tools are checked against its manifest where the
/// image is built (`every_tool_is_classed_in_the_manifest`).
#[tokio::test(flavor = "multi_thread")]
async fn each_google_manifest_names_exactly_the_tools_its_live_server_lists() {
    let http = reqwest::Client::new();
    for name in ["google-drive"] {
        let path = magnus_invariants::appliance::workspace_root().join(format!("catalog/{name}.toml"));
        let m: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        let endpoint = m["endpoint"].as_str().unwrap();
        let mut named: Vec<String> = m["tools"].as_table().unwrap().keys().cloned().collect();
        let v: Value = http
            .post(endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }))
            .send()
            .await
            .unwrap_or_else(|e| panic!("{endpoint}: {e}"))
            .json()
            .await
            .unwrap_or_else(|e| panic!("{endpoint}: {e}"));
        let mut live: Vec<String> =
            v["result"]["tools"].as_array().unwrap_or_else(|| panic!("{endpoint}: {v}")).iter().filter_map(|t| t["name"].as_str().map(String::from)).collect();
        named.sort();
        live.sort();
        assert_eq!(named, live, "{name}: the manifest's tools differ from what {endpoint} lists now");
    }
}

/// A person's card for Gmail, from its manifest: its name, the manifest's
/// one sentence, what it allows in plain words, and nothing of the
/// protocol (connectors spec, The connector card).
#[tokio::test(flavor = "multi_thread")]
async fn a_person_s_card_for_gmail_says_what_magnus_can_do_in_plain_words() {
    let (id, secret) = client();
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let _ = id;
    assert_eq!(app.api("alice").put("/v1/registrations/google", json!({ "client_secret": secret })).await.0, 204);
    let (st, c) = app.api("carol").get("/v1/connectors/google-gmail").await;
    assert!(st.is_success(), "{c}");
    assert_eq!((c["title"].as_str(), c["state"].as_str()), (Some("Gmail"), Some("available")), "{c}");
    assert_eq!(c["summary"], "Magnus can search and read your email, write drafts and send one when you allow it, and file, label or trash messages.", "{c}");
    let allows: Vec<String> = c["allows"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
    assert!(allows[0].starts_with("Reads: ") && allows[0].contains("search threads"), "{allows:?}");
    assert!(allows.iter().any(|l| l.starts_with("Changes that can be undone: ") && l.contains("create draft")), "{allows:?}");
    assert!(c["recent"].as_array().unwrap().is_empty() && c["connected_at"].is_null(), "{c}");
    for word in ["oauth", "scope", "token", "mcp", "http", "port", "googleapis"] {
        assert!(!c.to_string().to_lowercase().contains(word), "carol's card says {word:?}: {c}");
    }
}

/// The Library's connectors in the browser (`desktop/tests/connectors.mjs`),
/// with Google's online through the organisation's registration.
#[tokio::test(flavor = "multi_thread")]
async fn the_library_shows_google_s_connectors_as_the_spec_s_cards_in_the_browser() {
    use magnus_scratch::{chromium, set_password, shots, PASSWORD, PERSON};
    let (id, secret) = client();
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, PERSON, PASSWORD));
    let _ = id;
    assert_eq!(app.api(PERSON).put("/v1/registrations/google", json!({ "client_secret": secret })).await.0, 204);
    let out = tokio::process::Command::new("node")
        .arg(magnus_invariants::appliance::workspace_root().join("desktop/tests/connectors.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", PERSON)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the Library's connectors failed:\n{log}");
}
