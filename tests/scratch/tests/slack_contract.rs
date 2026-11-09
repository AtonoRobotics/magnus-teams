//! Slack's contract test (connectors spec, Brokered API connectors). The
//! image the catalog names, built from this checkout, in its own
//! Firecracker microVM, against Slack itself: with no account, that Slack
//! is reached and an unknown token is said as a sign-in to do again. With
//! the owner's Slack test account, every tool, once it is given:
//!
//! - `MAGNUS_SLACK_TEST_REFRESH_TOKEN`: a user refresh token for the
//!   manifest's scopes, from Alpha Vector's Slack client;
//! - `MAGNUS_SLACK_CLIENT_SECRET`: that client's secret;
//! - `MAGNUS_SLACK_TEST_CHANNEL`: a channel id the account can post in,
//!   which the test posts one message to.
//!
//! Without them it says so.

use magnus::apps::{CallError, CallOutput};
use magnus::brokered::Brokered;
use magnus::config;
use serde_json::{json, Value};
use std::path::PathBuf;

fn root() -> PathBuf {
    magnus_invariants::appliance::workspace_root()
}

fn manifest() -> toml::Table {
    std::fs::read_to_string(root().join("catalog/slack.toml")).unwrap().parse().unwrap()
}

async fn access_token(refresh: &str, secret: &str) -> String {
    let r: toml::Table = std::fs::read_to_string(root().join("catalog/registrations.toml")).unwrap().parse().unwrap();
    let client = r["slack"]["client_id"].as_str().unwrap();
    let v: Value = reqwest::Client::new()
        .post("https://slack.com/api/oauth.v2.user.access")
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client), ("client_secret", secret)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v["access_token"].as_str().unwrap_or_else(|| panic!("Slack refused the test account's refresh token: {v}")).to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn slacks_tools_do_what_they_say_against_slack() {
    let mf = manifest();
    let images = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(root().join("brokered/build.sh")).arg("magnus-slack").arg(images.path()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(mf["endpoint"].as_str(), Some(digest.as_str()), "catalog/slack.toml names another image than the one built from this commit");

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
        toml::from_str(&format!("name = \"slack\"\ntransport = \"brokered\"\nurl = \"{digest}\"\naudience = \"public\"\nlabel = \"private+untrusted\"")).unwrap();
    c.hosts = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();

    let listed: Vec<String> = b.list_tools(&c).await.unwrap().into_iter().map(|t| t.name).collect();
    let mut classed: Vec<String> = mf["tools"].as_table().unwrap().keys().cloned().collect();
    let mut l = listed.clone();
    l.sort();
    classed.sort();
    assert_eq!(l, classed);

    let r = b.call(&c, "contract", Some("xoxp-not-a-token"), "lookup_user", &json!({ "user": "USLACKBOT" }), &|| Ok(())).await.unwrap();
    assert!(r.is_error && r.text.contains("Sign in again"), "{}", r.text);

    let (Ok(refresh), Ok(secret), Ok(channel)) =
        (std::env::var("MAGNUS_SLACK_TEST_REFRESH_TOKEN"), std::env::var("MAGNUS_SLACK_CLIENT_SECRET"), std::env::var("MAGNUS_SLACK_TEST_CHANNEL"))
    else {
        eprintln!("the rest of Slack's contract test needs the owner's Slack test account (MAGNUS_SLACK_TEST_REFRESH_TOKEN, MAGNUS_SLACK_CLIENT_SECRET, MAGNUS_SLACK_TEST_CHANNEL); not given, so it was not run");
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
    let who = call("lookup_user", json!({ "user": "USLACKBOT" })).await;
    assert_eq!(who["user_id"], "USLACKBOT", "{who}");
    let found = call("search_messages", json!({ "query": "in:#general", "max_results": 1 })).await;
    assert!(found["messages"].is_array(), "{found}");
    let listed = call("list_conversations", json!({ "max_results": 5 })).await;
    assert!(listed["conversations"].as_array().unwrap().iter().any(|x| x["id"] == channel), "{listed}");
    let read = call("read_conversation", json!({ "channel": channel, "max_results": 1 })).await;
    assert!(read["messages"].is_array(), "{read}");
    let text = format!("Magnus contract test {}", uuid::Uuid::new_v4());
    let posted = call("post_message", json!({ "channel": channel, "text": text })).await;
    assert!(posted["ts"].as_str().is_some_and(|t| !t.is_empty()), "{posted}");
    b.stop(None, None).await;
}
