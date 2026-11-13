use super::*;

#[test]
fn content_leaves_the_record_and_joins_back_as_written() {
    let ev = serde_json::json!({ "kind": "memory", "id": "m1", "person": "alice", "run": "r1", "fact": "Salary is 150k.", "topic": null, "label": { "untrusted": false, "private": true } });
    let (record, content) = split(ev.clone());
    let content = content.expect("a memory has content");
    assert!(!record.to_string().contains("150k"), "the fact stayed in the log: {record}");
    assert_eq!(record["topic"], Value::Null, "a field without content stays as it was");
    let bytes = serde_json::to_vec(&content.body).unwrap();
    assert_eq!(record["content"]["sha256"], hash(&content.salt, &bytes));
    assert_ne!(record["content"]["sha256"], hex::encode(Sha256::digest(&bytes)), "the hash is not salted");
    assert_eq!(join(record.clone(), Some(content.body)), ev);
    assert_eq!(join(record.clone(), None), record, "deleted content leaves the record");

    let nested = serde_json::json!({ "kind": "task_created", "text": "Plan the offsite.", "chat": { "thread": "t1", "turn": false, "conversation": ["earlier"] } });
    let (record, content) = split(nested.clone());
    assert_eq!(record["chat"], serde_json::json!({ "thread": "t1", "turn": false }));
    assert_eq!(join(record, content.map(|c| c.body)), nested);

    // Research keeps its sources' quotes: content, deleted with the entry.
    let kb = serde_json::json!({ "kind": "knowledge_added", "id": "kb1", "title": "Headcount", "text": "120 staff.", "source": { "kind": "research", "citations": [{ "source": "https://a.example/", "quote": "QUOTE-120-staff" }] } });
    let (record, _) = split(kb);
    assert!(!record.to_string().contains("QUOTE-120-staff"), "a citation's quote stayed in the log: {record}");

    let plain = serde_json::json!({ "kind": "resumed", "run": "r1" });
    assert!(matches!(split(plain.clone()), (r, None) if r == plain), "an event without content is stored whole");

    // Watched ids are source-derived content: they leave the log
    // for the deletable store, or erasure cannot remove them (#408).
    let seen = serde_json::json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ["Q3 numbers"] });
    let (record, content) = split(seen.clone());
    let content = content.expect("watched ids are content");
    assert!(!record.to_string().contains("Q3 numbers"), "watched ids stayed in the log: {record}");
    assert_eq!(join(record.clone(), Some(content.body)), seen);
    assert_eq!(join(record.clone(), None)["ids"], Value::Null, "deleted ids leave no body behind");
}

#[test]
fn evidence_content_is_deletable_and_replays_after_deletion() {
    for change in [
        crate::evidence::Change::ReadBack {
            run: "r".into(),
            call_id: "c".into(),
            tool: "send".into(),
            read: Some("get".into()),
            found: json!({"body": "PRIVATE"}),
            matches: Some(true),
            note: "PRIVATE error".into(),
        },
        crate::evidence::Change::Sources { run: "r".into(), sources: vec![crate::evidence::Source { claim: "PRIVATE claim".into(), call: "c".into() }] },
    ] {
        let original = serde_json::to_value(Ev::Evidence(change)).unwrap();
        let (record, content) = split(original.clone());
        assert!(!record.to_string().contains("PRIVATE"));
        assert_eq!(join(record.clone(), Some(content.unwrap().body)), original);
        serde_json::from_value::<Ev>(record).expect("erased evidence still replays");
    }
}
