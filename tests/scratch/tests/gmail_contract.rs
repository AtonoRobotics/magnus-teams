//! Gmail's contract test (connectors spec, Brokered API connectors:
//! "Every brokered connector runs contract tests in CI against provider
//! test accounts before its manifest is published"): the image the catalog
//! names, built from this checkout, in its own Firecracker microVM, against
//! Gmail itself: with no account, that Gmail is reached and an unknown
//! token is said as a sign-in to do again; with the owner's Google test
//! account, every kind of tool, once it is given:
//!
//! - `MAGNUS_GMAIL_TEST_REFRESH_TOKEN`: a refresh token of the test
//!   account's, signed in with Alpha Vector's Google client
//!   (`catalog/registrations.toml`) for `gmail.modify`;
//! - `MAGNUS_GOOGLE_CLIENT_SECRET`: that client's secret;
//! - `MAGNUS_GMAIL_TEST_ADDRESS`: the test account's address, which the
//!   test writes to and sends to itself.
//!
//! Without them it skips that part and says `SKIPPED`; on CI or with
//! `MAGNUS_REQUIRE_CONTRACTS` it fails instead (`magnus_scratch::contract`). It leaves the account as it found it, but for
//! one sent message in the trash and its label.

use magnus::apps::{CallError, CallOutput};
use magnus::brokered::Brokered;
use magnus::config;
use serde_json::{json, Value};
use std::path::PathBuf;

fn root() -> PathBuf {
    magnus_invariants::appliance::workspace_root()
}

fn manifest() -> toml::Table {
    std::fs::read_to_string(root().join("catalog/google-gmail.toml")).unwrap().parse().unwrap()
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
async fn gmail_s_tools_do_what_they_say_against_gmail() {
    // Checked first: where contracts are required, a missing credential
    // fails before a microVM is booted.
    let account = magnus_scratch::contract::credentials(
        "Gmail's contract test with an account",
        "the owner's Google test account",
        ["MAGNUS_GMAIL_TEST_REFRESH_TOKEN", "MAGNUS_GOOGLE_CLIENT_SECRET", "MAGNUS_GMAIL_TEST_ADDRESS"],
    );
    let mf = manifest();
    let images = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(root().join("brokered/build.sh")).arg("magnus-gmail").arg(images.path()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(mf["endpoint"].as_str(), Some(digest.as_str()), "catalog/google-gmail.toml names another image than the one built from this commit");

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
        toml::from_str(&format!("name = \"google-gmail\"\ntransport = \"brokered\"\nurl = \"{digest}\"\naudience = \"public\"\nlabel = \"private+untrusted\"")).unwrap();
    c.hosts = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    c.scopes = mf["scopes"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();

    // Every tool it lists is classed in the manifest, and the other way round.
    let listed: Vec<String> = b.list_tools(&c).await.unwrap().into_iter().map(|t| t.name).collect();
    let mut classed: Vec<String> = mf["tools"].as_table().unwrap().keys().cloned().collect();
    let mut l = listed.clone();
    l.sort();
    classed.sort();
    assert_eq!(l, classed);

    // Gmail itself, reached from the microVM over TLS it checks: a token
    // Gmail does not know is said as a sign-in to do again.
    let r = b.call(&c, "contract", Some("ya29.not-a-token"), "list_labels", &json!({}), &|| Ok(())).await.unwrap();
    assert!(r.is_error && r.text.contains("Gmail rejected the sign-in"), "{}", r.text);

    let Some([refresh, secret, me]) = account else {
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

    let labels = call("list_labels", json!({})).await;
    assert!(labels["labels"].as_array().unwrap().iter().any(|l| l["label_id"] == "INBOX"), "{labels}");
    call("search_threads", json!({ "query": "in:inbox", "max_results": 2 })).await;

    // A draft: written, read back, rewritten, deleted for good.
    let subject = format!("Magnus contract test {}", uuid::Uuid::new_v4());
    let d = call("create_draft", json!({ "to": [me], "subject": subject, "body": "Written by Magnus's contract test." })).await;
    let id = d["draft_id"].as_str().unwrap().to_string();
    let got = call("get_draft", json!({ "draft_id": id })).await;
    assert_eq!(got["message"]["subject"], subject.as_str(), "{got}");
    call("update_draft", json!({ "draft_id": id, "to": [me], "subject": subject, "body": "Rewritten." })).await;
    assert_eq!(call("delete_draft", json!({ "draft_id": id })).await["deleted"], true);
    let gone = b.call(&c, "contract", Some(&token), "get_draft", &json!({ "draft_id": id }), &|| Ok(())).await.unwrap();
    assert!(gone.is_error && gone.text.contains("could not find"), "{}", gone.text);

    // A draft sent to the account itself, found, replied to as a draft,
    // labelled and filed in the trash.
    let d = call("create_draft", json!({ "to": [me], "subject": subject, "body": "Sent by Magnus's contract test." })).await;
    let sent = call("send_draft", json!({ "draft_id": d["draft_id"] })).await;
    let mid = sent["message_id"].as_str().unwrap().to_string();
    let m = call("get_message", json!({ "message_id": mid })).await;
    assert!(m["body"].as_str().unwrap().contains("Sent by Magnus's contract test."), "{m}");
    let reply = call("reply_draft", json!({ "message_id": mid, "body": "A reply, never sent." })).await;
    assert_eq!(reply["thread_id"], m["thread_id"], "{reply}");
    let r = call("get_draft", json!({ "draft_id": reply["draft_id"] })).await;
    assert!(r["message"]["subject"].as_str().unwrap().starts_with("Re: "), "{r}");
    call("delete_draft", json!({ "draft_id": reply["draft_id"] })).await;
    let name = "Magnus contract test";
    let label = match labels["labels"].as_array().unwrap().iter().find(|l| l["name"] == name) {
        Some(l) => l["label_id"].as_str().unwrap().to_string(),
        None => call("create_label", json!({ "name": name })).await["label_id"].as_str().unwrap().to_string(),
    };
    let on = call("label_message", json!({ "message_id": mid, "label_ids": [label] })).await;
    assert!(on["labels"].as_array().unwrap().iter().any(|l| l == label.as_str()), "{on}");
    let off = call("update_message_labels", json!({ "message_id": mid, "remove_label_ids": [label] })).await;
    assert!(!off["labels"].as_array().unwrap().iter().any(|l| l == label.as_str()), "{off}");
    let trashed = call("trash_message", json!({ "message_id": mid })).await;
    assert!(trashed["labels"].as_array().unwrap().iter().any(|l| l == "TRASH"), "{trashed}");
    b.stop(None, None).await;
}
