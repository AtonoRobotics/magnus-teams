//! The Chat server's own pieces: its tools against its manifest, names as
//! Chat gives them, and a message as a person reads it.

use super::*;

#[test]
fn every_tool_is_classed_in_the_manifest() {
    let m: toml::Table = include_str!("../../catalog/google-chat.toml").parse().unwrap();
    assert_eq!(m["kind"].as_str(), Some("brokered"));
    assert_eq!(m["version"].as_str(), Some("2.0.0"));
    let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
    let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
    classed.sort();
    listed.sort();
    assert_eq!(classed, listed);
    // Each is seen by everyone in the conversation, and cannot be unseen.
    for t in ["send_message", "update_message", "delete_message"] {
        assert_eq!(m["tools"][t].as_str(), Some("irreversible"), "{t}");
    }
}

#[test]
fn a_disabled_chat_account_keeps_googles_actionable_error() {
    let response = magnus_brokered_kit::Response {
        status: 400,
        head: "HTTP/1.1 400 Bad Request\r\n".into(),
        body: json!({ "error": { "code": 400, "status": "FAILED_PRECONDITION", "message": "Google Chat is turned off. To use Chat API, turn on Google Chat." } })
            .to_string()
            .into_bytes(),
    };
    assert!(matches!(API.explain(&response, false), Failure::NotDone(t) if t.contains("Google Chat is turned off") && t.contains("turn on Google Chat")));
}

#[test]
fn ownership_requires_the_current_account_and_a_human_sender() {
    let person = json!({ "sub": "123" });
    let own = json!({ "sender": { "name": "users/123", "type": "HUMAN" } });
    assert!(owns_message(&person, &own));
    assert!(!owns_message(&json!({ "sub": "456" }), &own));
    for unknown in [json!({}), json!({ "sub": "" }), json!({ "sub": null })] {
        assert!(!owns_message(&unknown, &own));
    }
    for unknown in [json!({}), json!({ "sender": { "name": "users/123" } }), json!({ "sender": { "name": "users/123", "type": "BOT" } })] {
        assert!(!owns_message(&person, &unknown));
    }
}

#[test]
fn every_tool_names_its_schemas() {
    let ts = tools();
    assert_eq!(ts.len(), 8);
    for t in &ts {
        assert_eq!(t.input["type"], "object", "{}", t.name);
        assert_eq!(t.output["type"], "object", "{}", t.name);
    }
}

/// Only a name as Chat gives it goes in a path: a space, or a message in
/// one, and nothing that would reach elsewhere.
#[test]
fn only_chat_s_own_names_go_in_a_path() {
    let n = |v: &str, m: bool| name(&json!({ "k": v }), "k", m);
    assert_eq!(n("spaces/AAAA1b", false).unwrap(), "spaces/AAAA1b");
    assert_eq!(n("AAAA1b", false).unwrap(), "spaces/AAAA1b");
    assert_eq!(n("spaces/AAA/messages/BBB.CCC", true).unwrap(), "spaces/AAA/messages/BBB.CCC");
    for bad in ["spaces/AAA/../../users/me", "spaces/AAA?x=1", "spaces/", "spaces/AAA/members/BBB", "spaces/AAA/messages/.."] {
        assert!(n(bad, true).is_err() && n(bad, false).is_err(), "{bad}");
    }
    assert!(n("spaces/AAA/messages/BBB", false).is_err());
    assert!(n("spaces/AAA", true).is_err());
}

#[test]
fn a_message_is_read_as_a_person_reads_it() {
    let m = json!({ "name": "spaces/A/messages/B", "sender": { "name": "users/1", "displayName": "Bob" }, "text": "Friday?", "createTime": "2026-11-03T10:00:00Z", "thread": { "name": "spaces/A/threads/T" } });
    let v = message(&m);
    assert_eq!((v["from"].as_str(), v["text"].as_str(), v["thread_id"].as_str(), v["edited"].as_bool()), (Some("Bob"), Some("Friday?"), Some("spaces/A/threads/T"), Some(false)));
    assert_eq!(when(&json!({ "after": "2026-11-01T00:00:00Z\" OR x" })).as_deref(), Some("createTime > \"2026-11-01T00:00:00Z OR x\""));
    assert_eq!(when(&json!({})), None);
}
