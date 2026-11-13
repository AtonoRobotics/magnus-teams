/// Every kind and field the content list names is one `Ev` has, so a
/// misspelling cannot leave content in the log; and each has a default,
/// so an event whose content was deleted still reads.
#[test]
fn the_content_list_names_real_kinds_and_fields() {
    let src = include_str!("events.rs");
    let src = &src[src.find("pub enum Ev {").unwrap()..src.find("pub const CONTENT").unwrap()];
    for (kind, fields) in super::CONTENT.iter().filter(|(k, _)| !["team", "rule", "spend", "evidence"].contains(k)) {
        let variant: String = kind.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect();
        let start = src.find(&format!("\n    {variant} {{")).unwrap_or_else(|| panic!("no event kind {kind}"));
        let body = &src[start..start + src[start..].find("\n    },").unwrap()];
        for f in *fields {
            let top = f.split('.').next().unwrap();
            assert!(body.contains(&format!(" {top}: ")), "{kind} has no field {top}");
            if !f.contains('.') {
                assert!(body.contains(&format!("#[serde(default)]\n        {top}: ")), "{kind}.{top} has no default");
            }
        }
    }
}

/// The team's events are one kind with many shapes: every use of a content
/// field in any of them has a default.
#[test]
fn the_teams_content_fields_all_read_once_deleted() {
    let src = include_str!("team/mod.rs");
    let src = &src[src.find("pub enum Change {").unwrap()..src.find("/// Scratch for one request").unwrap()];
    let (_, fields) = super::CONTENT.iter().find(|(k, _)| *k == "team").unwrap();
    for f in *fields {
        let field = format!("\n        {f}: ");
        assert!(src.contains(&field), "no team event has {f}");
        assert_eq!(src.matches(&field).count(), src.matches(&format!("#[serde(default)]{field}")).count(), "a team event's {f} has no default");
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
