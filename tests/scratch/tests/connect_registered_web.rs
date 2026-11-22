//! Apps whose client Alpha Vector registered with the provider connect from
//! a web browser (`decisions/2026-11-21-connect-registered-apps-on-the-web.md`):
//! on a server whose return address the client lists, the card offers the
//! browser's sign-in, and the sign-in comes back to `/connected` through
//! that client (Google's web one, Microsoft's with its secret, the others'
//! own), provisioned by the release's own script. The desktop's sign-in is
//! unchanged, and another server's address is refused.
#![allow(clippy::unwrap_used)] // a test file: its helpers are tests too

use magnus_invariants::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The server's public address, the one the catalog's clients list.
const PUBLIC: &str = "https://teams.pyrallon.ai";
const BACK: &str = "https://teams.pyrallon.ai/connected";
/// The stand-in web client the release provisions here.
const WEB_ID: &str = "teams-web.apps.googleusercontent.com";
const ASANA_ID: &str = "teams-asana";

fn card(list: &Value, name: &str) -> Value {
    list["connectors"].as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap_or_else(|| panic!("{name} is not in the Library: {list}"))
}

fn query(v: &Value) -> HashMap<String, String> {
    url::Url::parse(v["authorize_url"].as_str().unwrap_or_else(|| panic!("no sign-in address: {v}"))).unwrap().query_pairs().into_owned().collect()
}

fn catalog_id(provider: &str) -> String {
    let r: toml::Table = std::fs::read_to_string(appliance::workspace_root().join("catalog/registrations.toml")).unwrap().parse().unwrap();
    r[provider]["client_id"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn registered_apps_connect_from_a_browser_through_the_clients_that_list_the_server() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let config = app.dir.path().join("magnus.toml");
    let mut cfg: toml::Table = toml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    cfg["appliance"].as_table_mut().unwrap().insert("public_url".into(), PUBLIC.into());
    std::fs::write(&config, toml::to_string(&cfg).unwrap()).unwrap();
    app.restart().await;
    let carol = app.api("carol");

    // Before the release provisions Google's web client, Gmail is connected
    // from the desktop app; Vercel's public client is in the catalog.
    let (_, list) = carol.get("/v1/connectors").await;
    assert_eq!(card(&list, "google-gmail")["web_sign_in"], false, "{}", card(&list, "google-gmail"));
    assert_eq!(card(&list, "vercel")["web_return"], BACK, "{}", card(&list, "vercel"));

    // The release's own script provisions the clients (stand-ins: these
    // sign-ins never reach a token exchange).
    let mut script = std::process::Command::new(appliance::workspace_root().join("release/provision-registrations.sh"));
    script.arg(&config);
    for (k, v) in [
        ("GOOGLE_DESKTOP_CLIENT_SECRET", "stand-in"),
        ("GOOGLE_WEB_CLIENT_ID", WEB_ID),
        ("GOOGLE_WEB_CLIENT_SECRET", "stand-in"),
        ("MICROSOFT_CLIENT_SECRET", "stand-in"),
        ("GH_OAUTH_CLIENT_SECRET", "stand-in"),
        ("SLACK_CLIENT_SECRET", "stand-in"),
        ("ASANA_CLIENT_ID", ASANA_ID),
        ("ASANA_CLIENT_SECRET", "stand-in"),
    ] {
        script.env(k, v);
    }
    let out = script.output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    assert!(said.contains("google_web (confidential): client ID provisioned, client secret provisioned: HTTP 2"), "{said}");
    assert!(said.contains("microsoft_web (confidential): client ID from the catalog, client secret provisioned: HTTP 2"), "{said}");
    assert!(!said.contains("stand-in") && !said.contains(WEB_ID), "the script printed a value: {said}");

    // Each card offers the browser's sign-in, back to this server.
    let (_, list) = carol.get("/v1/connectors").await;
    for name in
        ["google-gmail", "google-calendar", "google-drive", "google-chat", "microsoft-outlook", "microsoft-calendar", "microsoft-onedrive", "github", "slack", "asana", "vercel"]
    {
        let c = card(&list, name);
        assert_eq!((c["web_sign_in"].as_bool(), c["web_return"].as_str()), (Some(true), Some(BACK)), "{name}: {c}");
    }

    // The browser's sign-in goes through the client that lists the address.
    for (name, id) in [("google-gmail", WEB_ID.to_string()), ("microsoft-outlook", catalog_id("microsoft")), ("asana", ASANA_ID.to_string()), ("vercel", catalog_id("vercel"))] {
        let (st, v) = carol.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": BACK })).await;
        assert!(st.is_success(), "{name}: {v}");
        let q = query(&v);
        assert_eq!((q.get("client_id"), q.get("redirect_uri").map(String::as_str)), (Some(&id), Some(BACK)), "{name}: {v}");
    }

    // The desktop's sign-ins are unchanged: Google's desktop client at a
    // local address, Vercel's fixed one.
    let (st, v) = carol.post("/v1/connectors/google-gmail/sign-in", json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    assert_eq!(query(&v).get("client_id"), Some(&catalog_id("google")), "{v}");
    let (st, v) = carol.post("/v1/connectors/vercel/sign-in", json!({ "redirect_uri": "http://127.0.0.1:17321/callback" })).await;
    assert!(st.is_success(), "{v}");

    // An address the clients do not list is refused before any provider sees it.
    for name in ["google-gmail", "vercel", "asana"] {
        let (st, v) = carol.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "https://other.example/connected" })).await;
        assert!(!st.is_success(), "{name} signed in to another server's address: {v}");
    }
}
