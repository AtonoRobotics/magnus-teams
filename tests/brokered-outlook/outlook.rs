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
    // A send is read back from Sent Items by the id it returned.
    let rb = &m["read_back"]["send_draft"];
    assert_eq!(rb["read"].as_str(), Some("get_sent_message"));
    assert_eq!(m["tools"]["get_sent_message"].as_str(), Some("read"));
    assert_eq!(rb["input"]["internet_message_id"].as_str(), Some("/result/internet_message_id"));
    for (from, to) in [("/shown/content/subject", "/subject"), ("/shown/recipients/to", "/to"), ("/shown/recipients/cc", "/cc"), ("/shown/recipients/bcc", "/bcc")] {
        assert_eq!(rb["expect"][from].as_str(), Some(to), "{from}");
    }
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

fn tool_named(name: &str) -> Tool {
    tools().into_iter().find(|t| t.name == name).unwrap()
}

/// A send reads the draft's Internet message id, refuses what is not a
/// draft before anything is sent, and returns the id its schema promises.
#[test]
fn a_send_returns_the_id_it_is_read_back_by() {
    let mut asked: Vec<(String, String)> = vec![];
    let r = sending(&json!({ "draft_id": "AAMk/1" }), &mut |method, path, _| {
        asked.push((method.to_string(), path.to_string()));
        Ok(if method == "GET" { json!({ "isDraft": true, "internetMessageId": "<m1@outlook.example>" }) } else { json!({}) })
    })
    .unwrap();
    assert_eq!(r, json!({ "sent": true, "internet_message_id": "<m1@outlook.example>" }));
    assert_eq!(
        asked,
        vec![("GET".to_string(), "/v1.0/me/messages/AAMk%2F1?$select=isDraft,internetMessageId".to_string()), ("POST".to_string(), "/v1.0/me/messages/AAMk%2F1/send".to_string())]
    );
    let send = tool_named("send_draft");
    assert_eq!(magnus_brokered_kit::schema::check(&send.output, &r), Ok(()));
    assert!(magnus_brokered_kit::schema::check(&send.output, &json!({ "sent": true })).is_err(), "a send without its id passed");

    let mut posted = false;
    let e = sending(&json!({ "draft_id": "m2" }), &mut |method, _, _| {
        posted |= method == "POST";
        Ok(json!({ "isDraft": false, "internetMessageId": "<m2@outlook.example>" }))
    });
    assert!(matches!(e, Err(Failure::NotDone(_))));
    assert!(!posted, "a message that is not a draft was sent");
}

/// The sent copy is found in Sent Items by its Internet message id, the id
/// quoted as OData quotes it, looked for again while Graph files it, and
/// read whole, as its schema says.
#[test]
fn a_sent_message_is_found_in_sent_items_by_its_id() {
    let sent = json!({ "id": "s1", "subject": "Plan", "isDraft": false, "toRecipients": [{ "emailAddress": { "address": "bo@example.com" } }],
        "bccRecipients": [{ "emailAddress": { "address": "cy@example.com" } }], "body": { "contentType": "text", "content": "Hi" }, "attachments": [] });
    let mut paths: Vec<String> = vec![];
    let mut waits = 0;
    let v = find_sent(
        "<o'k@outlook.example>",
        &mut |p| {
            paths.push(p.to_string());
            Ok(if paths.len() < 3 { json!({ "value": [] }) } else { json!({ "value": [sent.clone()] }) })
        },
        &mut || waits += 1,
    )
    .unwrap();
    assert_eq!((paths.len(), waits), (3, 2));
    assert!(paths[0].starts_with("/v1.0/me/mailFolders/sentitems/messages?$filter=internetMessageId%20eq%20%27%3Co%27%27k%40outlook.example%3E%27&"), "{}", paths[0]);
    assert_eq!((v["subject"].as_str(), v["to"].clone(), v["bcc"].clone()), (Some("Plan"), json!(["bo@example.com"]), json!(["cy@example.com"])));
    assert_eq!(magnus_brokered_kit::schema::check(&tool_named("get_sent_message").output, &v), Ok(()));
}

/// Never found, it is looked for a bounded number of times and then said
/// not to be there yet; a lookup with no id asks Graph for nothing.
#[test]
fn a_sent_message_not_filed_yet_is_looked_for_a_bounded_number_of_times() {
    let mut gets = 0;
    let mut waits = 0;
    let e = find_sent(
        "<m@x>",
        &mut |_| {
            gets += 1;
            Ok(json!({ "value": [] }))
        },
        &mut || waits += 1,
    );
    assert!(matches!(e, Err(Failure::NotDone(ref w)) if w.contains("not in Sent Items yet")));
    assert_eq!((gets, waits), (SENT_LOOKUP_ATTEMPTS, SENT_LOOKUP_ATTEMPTS - 1));
    let e = find_sent(" ", &mut |_| panic!("Graph was asked with no id"), &mut || ());
    assert!(e.is_err());
}
