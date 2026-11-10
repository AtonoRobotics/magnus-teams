//! The Outlook mail server's own pieces: its tools against its manifest,
//! a draft as Graph takes it, and the digest a send's approval holds for.

use super::*;

#[test]
fn every_tool_is_classed_in_the_manifest() {
    let m: toml::Table = include_str!("../../catalog/microsoft-outlook.toml").parse().unwrap();
    let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
    let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
    classed.sort();
    listed.sort();
    assert_eq!(classed, listed);
    assert_eq!(m["tools"]["send_draft"].as_str(), Some("irreversible"));
    for t in ["create_draft", "update_draft", "reply_draft"] {
        assert_eq!(m["tools"][t].as_str(), Some("reversible_write"), "{t}");
        assert_eq!(m["reach"][t].as_str(), Some("self"), "{t}");
    }
    // Who a send reaches is read from the draft, by the tool that reads it.
    let d = &m["destinations"]["send_draft"];
    assert_eq!((d["read"].as_str(), d["id"].as_str()), (Some("get_draft"), Some("draft_id")));
    for t in tools() {
        assert_eq!(t.input["type"], "object", "{}", t.name);
        assert_eq!(t.output["type"], "object", "{}", t.name);
    }
}

/// A draft is written with only the fields a call names, its text as
/// plain text and its people by address.
#[test]
fn a_draft_is_written_as_graph_takes_it() {
    let b = draft_body(&json!({ "to": ["bo@example.com"], "subject": "Plan", "body": "Hi Bo" }));
    assert_eq!(b, json!({ "subject": "Plan", "body": { "contentType": "Text", "content": "Hi Bo" }, "toRecipients": [{ "emailAddress": { "address": "bo@example.com" } }] }));
    assert_eq!(draft_body(&json!({ "cc": [] })), json!({ "ccRecipients": [] }));
    assert_eq!(folder(&json!({})), "inbox");
    assert_eq!(folder(&json!({ "folder": "AAMk/x" })), "AAMk%2Fx");
}

/// What the gate shows of a draft: every recipient, Bcc too, and its text.
#[test]
fn a_draft_is_read_with_every_recipient() {
    let m = json!({ "id": "d1", "subject": "Plan", "isDraft": true, "toRecipients": [{ "emailAddress": { "name": "Bo", "address": "bo@example.com" } }],
        "bccRecipients": [{ "emailAddress": { "address": "cy@example.com" } }], "body": { "contentType": "html", "content": "<p>Hi</p>" },
        "attachments": [{ "name": "a.pdf", "contentType": "application/pdf", "size": 3 }] });
    let v = message(&m);
    assert_eq!((v["to"].clone(), v["cc"].clone(), v["bcc"].clone()), (json!(["bo@example.com"]), json!([]), json!(["cy@example.com"])));
    assert_eq!((v["body_type"].as_str(), v["body"].as_str(), v["attachments"][0]["name"].as_str()), (Some("html"), Some("<p>Hi</p>"), Some("a.pdf")));
}

/// The digest changes with any part the card does not show whole (the
/// body, who it is from, an attachment's bytes), and not with the order
/// Graph lists attachments in.
#[test]
fn a_draft_s_digest_holds_for_what_the_card_does_not_show() {
    let m = json!({ "body": { "contentType": "html", "content": "<p>Hi</p>" }, "from": { "emailAddress": { "address": "al@example.com" } } });
    let a = json!({ "@odata.type": "#microsoft.graph.fileAttachment", "name": "a.txt", "contentType": "text/plain", "contentBytes": STANDARD.encode("one") });
    let b = json!({ "@odata.type": "#microsoft.graph.fileAttachment", "name": "b.txt", "contentType": "text/plain", "contentBytes": STANDARD.encode("two") });
    let base = digest(&m, &json!([a, b]));
    assert_eq!(base, digest(&m, &json!([b, a])));
    let mut a2 = a.clone();
    a2["contentBytes"] = json!(STANDARD.encode("ONE"));
    assert_ne!(base, digest(&m, &json!([a2, b])));
    let mut m2 = m.clone();
    m2["body"]["content"] = json!("<p>Hi</p><img src=x>");
    assert_ne!(base, digest(&m2, &json!([a, b])));
    let mut m3 = m.clone();
    m3["replyTo"] = json!([{ "emailAddress": { "address": "eve@example.com" } }]);
    assert_ne!(base, digest(&m3, &json!([a, b])));
}

#[test]
fn a_message_in_a_list_is_read_as_a_person_reads_it() {
    let v = summary(
        &json!({ "id": "m1", "subject": "Hi", "from": { "emailAddress": { "address": "bo@example.com" } }, "receivedDateTime": "2026-11-10T09:00:00Z", "isRead": false, "bodyPreview": "Hello" }),
    );
    assert_eq!((v["message_id"].as_str(), v["from"].as_str(), v["read"].as_bool(), v["preview"].as_str()), (Some("m1"), Some("bo@example.com"), Some(false), Some("Hello")));
}
