//! Google Chat's contract test (connectors spec, Brokered API
//! connectors: "Every brokered connector runs contract tests in CI against
//! provider test accounts before its manifest is published"): the image the
//! catalog names, built from this checkout, in its own Firecracker microVM,
//! against Chat itself: with no account, that Chat is reached and
//! an unknown token is said as a sign-in to do again; with the owner's
//! Google test account, every tool, once it is given:
//!
//! - `MAGNUS_GOOGLE_TEST_REFRESH_TOKEN`: a refresh token of the test
//!   account's, signed in with Alpha Vector's Google client
//!   (`catalog/registrations.toml`) for the consent screen's scopes;
//! - `MAGNUS_GOOGLE_CLIENT_SECRET`: that client's secret;
//! - `MAGNUS_GOOGLE_TEST_CHAT_SPACE`: a space of the test account's with no
//!   one else in it (`spaces/...`), where it writes.
//!
//! Chat answers only once the project has a Chat app configured and the
//! consent screen has Chat's scopes (DECISIONS.md).
//!
//! Without them it skips that part and says `SKIPPED`; on CI or with
//! `MAGNUS_REQUIRE_CONTRACTS` it fails instead (`magnus_scratch::contract`). Its one message, in a space no one else is in,
//! it deletes.

use magnus::apps::{CallError, CallOutput};
use magnus::brokered::Brokered;
use magnus::config;
use serde_json::{json, Value};
use std::path::PathBuf;

fn root() -> PathBuf {
    magnus_invariants::appliance::workspace_root()
}

/// An access token for the test account, from Google.
async fn access_token(refresh: &str, secret: &str) -> String {
    let r: toml::Table = std::fs::read_to_string(root().join("catalog/registrations.toml")).unwrap().parse().unwrap();
    let client = r["google"]["client_id"].as_str().unwrap();
    let v: Value = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client), ("client_secret", secret)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("Google refused the test account's refresh token (a Testing app's lasts seven days): {v}")).to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_s_tools_do_what_they_say_against_chat() {
    // Checked first: where contracts are required, a missing credential
    // fails before a microVM is booted.
    let account = magnus_scratch::contract::credentials(
        "Chat's contract test with an account",
        "the owner's Google test account",
        ["MAGNUS_GOOGLE_TEST_REFRESH_TOKEN", "MAGNUS_GOOGLE_CLIENT_SECRET", "MAGNUS_GOOGLE_TEST_CHAT_SPACE"],
    );
    let mf: toml::Table = std::fs::read_to_string(root().join("catalog/google-chat.toml")).unwrap().parse().unwrap();
    let images = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(root().join("brokered/build.sh")).arg("magnus-chat").arg(images.path()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(mf["endpoint"].as_str(), Some(digest.as_str()), "catalog/google-chat.toml names another image than the one built from this commit");

    let guest = |var: &str, file: &str| std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| root().join("sandbox/out").join(file));
    let sandbox = config::Sandbox {
        firecracker: std::env::var("MAGNUS_FIRECRACKER").unwrap_or("/usr/local/bin/firecracker".into()).into(),
        kernel_image: guest("MAGNUS_GUEST_KERNEL", "vmlinux"),
        rootfs: guest("MAGNUS_GUEST_ROOTFS", "rootfs.ext4"),
        jailer: None,
        vcpus: 1,
        memory_mib: 256,
        workspace_mib: 64,
    };
    let state = tempfile::tempdir().unwrap();
    let cfg = config::Brokered { images: Some(images.path().into()), ..Default::default() };
    let b = Brokered::new(Some(sandbox), cfg, state.path());
    let mut c: config::Connector =
        toml::from_str(&format!("name = \"google-chat\"\ntransport = \"brokered\"\nurl = \"{digest}\"\naudience = \"public\"\nlabel = \"private+untrusted\"")).unwrap();
    c.hosts = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    c.scopes = mf["scopes"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();

    // Every tool it lists is classed in the manifest, and the other way round.
    let mut listed: Vec<String> = b.list_tools(&c).await.unwrap().into_iter().map(|t| t.name).collect();
    let mut classed: Vec<String> = mf["tools"].as_table().unwrap().keys().cloned().collect();
    listed.sort();
    classed.sort();
    assert_eq!(listed, classed);

    // Chat itself, reached from the microVM over TLS it checks: a token
    // Google does not know is said as a sign-in to do again.
    let r = b.call(&c, "contract", Some("ya29.not-a-token"), "search_conversations", &json!({}), &|| Ok(())).await.unwrap();
    assert!(r.is_error && r.text.contains("Google Chat rejected the sign-in"), "{}", r.text);

    let Some([refresh, secret, space]) = account else {
        b.stop(None, None).await;
        return;
    };

    let token = access_token(&refresh, &secret).await;
    let call = |tool: &'static str, args: Value| {
        let (b, c, token) = (&b, &c, token.clone());
        async move {
            let r: Result<CallOutput, CallError> = b.call(c, "contract", Some(&token), tool, &args, &|| Ok(())).await;
            let r = r.unwrap_or_else(|e| panic!("{tool}: {e:?}"));
            assert!(!r.is_error, "{tool}: {}", r.text);
            r.structured.unwrap_or_else(|| panic!("{tool}: no structured answer"))
        }
    };

    // Its space, found; its one member; a message sent there, read,
    // found, edited and deleted.
    let spaces = call("search_conversations", json!({})).await;
    assert!(spaces["spaces"].as_array().unwrap().iter().any(|s| s["space_id"] == space.as_str()), "{spaces}");
    let members = call("list_members", json!({ "space_id": space })).await;
    assert_eq!(members["members"].as_array().unwrap().len(), 1, "the test space has others in it: {members}");
    let words = format!("Magnus contract test {}", uuid::Uuid::new_v4());
    let sent = call("send_message", json!({ "space_id": space, "text": words })).await;
    let id = sent["message_id"].as_str().unwrap().to_string();
    assert_eq!(call("get_message", json!({ "message_id": id })).await["text"], words.as_str());
    assert!(call("list_messages", json!({ "space_id": space, "max_results": 10 })).await["messages"].as_array().unwrap().iter().any(|m| m["message_id"] == id.as_str()));
    assert!(call("search_messages", json!({ "space_id": space, "query": words })).await["messages"].as_array().unwrap().iter().any(|m| m["message_id"] == id.as_str()));
    let edited = call("update_message", json!({ "message_id": id, "text": format!("{words} (edited)") })).await;
    assert!(edited["text"].as_str().unwrap().ends_with("(edited)"), "{edited}");
    assert_eq!(call("delete_message", json!({ "message_id": id })).await["deleted"], true);
    b.stop(None, None).await;
}
