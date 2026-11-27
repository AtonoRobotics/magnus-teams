/// Every kind and field the content list names is one `Ev` has, so a
/// misspelling cannot leave content in the log; and each has a default,
/// so an event whose content was deleted still reads.
#[test]
fn the_content_list_names_real_kinds_and_fields() {
    let src = include_str!("events.rs");
    let payloads = include_str!("events/payloads.rs");
    let end = src.find("pub const CONTENT").or_else(|| src.find("#[path = \"events_content.rs\"]")).unwrap();
    let src = &src[src.find("pub enum Ev {").unwrap()..end];
    for (kind, fields) in super::CONTENT.iter().filter(|(k, _)| !["team", "team_mission", "rule", "spend", "evidence", "staged", "outside_agent"].contains(k)) {
        let variant: String = kind.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect();
        let (body, field_prefix) = if let Some(start) = src.find(&format!("\n    {variant} {{")) {
            (&src[start..start + src[start..].find("\n    },").unwrap()], "        ")
        } else {
            let needle = format!("\n    {variant}({variant}Payload),");
            assert!(src.contains(&needle), "no event kind {kind}");
            let pstart = payloads.find(&format!("pub struct {variant}Payload {{")).unwrap_or_else(|| panic!("no payload struct for {kind}"));
            (&payloads[pstart..pstart + payloads[pstart..].find("\n}").unwrap()], "    pub ")
        };
        for f in *fields {
            let top = f.split('.').next().unwrap();
            assert!(body.contains(&format!(" {top}: ")), "{kind} has no field {top}");
            if !f.contains('.') {
                let indent = if field_prefix.starts_with("    pub") { "    " } else { "        " };
                assert!(
                    body.contains(&format!("#[serde(default)]\n{field_prefix}{top}: ")) || body.contains(&format!("#[serde(default)]\n{indent}{top}: ")),
                    "{kind}.{top} has no default"
                );
            }
        }
    }
}

/// The team's events are one kind with many shapes: every use of a content
/// field in any of them has a default.
#[test]
fn the_teams_content_fields_all_read_once_deleted() {
    let src = include_str!("team/change.rs");
    let src = &src[src.find("pub enum Change {").unwrap()..src.find("/// Scratch for one request").unwrap()];
    let (_, fields) = super::CONTENT.iter().find(|(k, _)| *k == "team").unwrap();
    for f in *fields {
        let field = format!("\n        {f}: ");
        assert!(src.contains(&field), "no team event has {f}");
        assert_eq!(src.matches(&field).count(), src.matches(&format!("#[serde(default)]{field}")).count(), "a team event's {f} has no default");
    }
}

/// A mission's words (goal, objectives, drafts, releases, deliverable) are
/// content: each is a field of a mission event, and each reads once deleted.
#[test]
fn a_missions_words_are_content_and_read_once_deleted() {
    let src = include_str!("events/mission.rs");
    let (_, fields) = super::CONTENT.iter().find(|(k, _)| *k == "team_mission").unwrap();
    for f in *fields {
        let field = format!("\n        {f}: ");
        assert!(src.contains(&field), "no mission event has {f}");
        assert_eq!(src.matches(&field).count(), src.matches(&format!("#[serde(default)]{field}")).count(), "a mission event's {f} has no default");
    }
}

/// A rule's words are the person's content: they leave the log's record,
/// and a rule whose words were erased still reads.
#[test]
fn a_rules_words_are_content_and_read_once_deleted() {
    let src = include_str!("rules.rs");
    let src = &src[src.find("pub enum Change {").unwrap()..src.find("#[derive(Serialize, Clone, Debug)]\npub struct Rule").unwrap()];
    let (_, fields) = super::CONTENT.iter().find(|(k, _)| *k == "rule").unwrap();
    for f in *fields {
        let field = format!("\n        {f}: ");
        assert!(src.contains(&field), "no rule event has {f}");
        assert_eq!(src.matches(&field).count(), src.matches(&format!("#[serde(default)]{field}")).count(), "a rule event's {f} has no default");
    }
    let ev = super::Ev::Rule(crate::rules::Change::Proposed { id: "r".into(), person: "alice".into(), compiled: None, words: Some("ask me first".into()) });
    let (record, content) = crate::log::split(serde_json::to_value(&ev).unwrap());
    assert!(record.get("words").is_none(), "the words stayed in the record");
    assert_eq!(content.unwrap().body["words"], "ask me first");
}

/// Evidence is a tagged family too; every content field must remain readable
/// after deletion, rather than matching only the outer Ev variant's spelling.
#[test]
fn evidence_content_fields_have_defaults() {
    let src = include_str!("evidence.rs");
    let src = &src[src.find("pub enum Change {").unwrap()..src.find("impl Change").unwrap()];
    for field in super::content_fields("evidence") {
        let declaration = format!("\n        {field}: ");
        assert!(src.contains(&declaration), "missing evidence field {field}");
        assert_eq!(src.matches(&declaration).count(), src.matches(&format!("#[serde(default)]{declaration}")).count());
    }
}

/// An outside agent's words (a request, a held draft and why, a reply) are
/// content: each is a field of an outside-agent change, and each change
/// still reads once its words are deleted.
#[test]
fn an_outside_agents_words_are_content_and_read_once_deleted() {
    let src = include_str!("a2a/ledger.rs");
    let src = &src[src.find("pub enum Change {").unwrap()..src.find("/// A task Magnus sent").unwrap()];
    let (_, fields) = super::CONTENT.iter().find(|(k, _)| *k == "outside_agent").unwrap();
    for f in *fields {
        let field = format!("\n        {f}: ");
        assert!(src.contains(&field), "no outside-agent change has {f}");
        assert_eq!(src.matches(&field).count(), src.matches(&format!("#[serde(default)]{field}")).count(), "an outside-agent change's {f} has no default");
    }
    for deleted in [
        serde_json::json!({ "change": "request_received", "agent": "oa", "task": "in-1", "message_id": "m-1", "to": { "person": "mo" } }),
        serde_json::json!({ "change": "reply_held", "task": "in-1", "item": "i1" }),
        serde_json::json!({ "change": "replied", "task": "in-1", "by": "kernel" }),
    ] {
        assert!(serde_json::from_value::<crate::a2a::Change>(deleted.clone()).is_ok(), "{deleted} does not read once its words are deleted");
    }
}
