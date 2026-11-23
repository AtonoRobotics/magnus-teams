use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Each tool's spec, with the description keys its schema asked for.
fn described() -> Vec<(&'static Decl, String, Value, BTreeSet<String>)> {
    let desc = descriptions();
    all()
        .map(|t| {
            let asked = RefCell::new(BTreeSet::new());
            let d = |n: &str| {
                asked.borrow_mut().insert(n.to_string());
                desc[n].as_str().unwrap_or_default().to_string()
            };
            let schema = (t.schema)(&d);
            (t, d(t.name), schema, asked.into_inner())
        })
        .collect()
}

#[test]
fn every_tool_has_a_description_a_schema_and_words() {
    for (t, description, schema, asked) in described() {
        assert!(!description.trim().is_empty(), "{} has no description in instructions/tools.json", t.name);
        assert_eq!(schema["type"], "object", "{} has no input schema", t.name);
        assert!(schema["properties"].is_object(), "{}'s schema has no properties", t.name);
        for key in asked.iter().filter(|k| *k != t.name) {
            assert!(descriptions()[key.as_str()].as_str().is_some_and(|s| !s.is_empty()), "{} asks for {key}, which tools.json does not have", t.name);
        }
        assert!(!doing_words(t.name).is_empty(), "{} says nothing on a card", t.name);
        assert_ne!(doing_words(t.name), t.name, "{} reads as its name on a card", t.name);
    }
}

/// Every description is a declared tool's, or a field one of their schemas uses.
#[test]
fn tools_json_has_no_orphans() {
    let used: BTreeSet<String> = described().into_iter().flat_map(|(_, _, _, asked)| asked).collect();
    for key in descriptions().as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        assert!(used.contains(&key), "instructions/tools.json describes {key}, which no declared tool uses");
    }
}

#[test]
fn a_tool_is_declared_once() {
    let mut seen = BTreeSet::new();
    for t in all() {
        assert!(seen.insert(t.name), "{} is declared twice", t.name);
    }
}

/// A record never passes the boundary, and everything else does, with what
/// carries it out.
#[test]
fn a_record_has_no_class_and_a_tool_has_one() {
    for t in all() {
        assert_eq!(t.class.is_none(), t.exec == Exec::Record, "{}", t.name);
        assert!(!(t.exec == Exec::Record && t.offer == Offer::Team), "{} is a record of a team thread", t.name);
        if let Exec::Team(_) = t.exec {
            assert_eq!(t.offer, Offer::Team, "{} is the workspace's but offered elsewhere", t.name);
        }
    }
}

/// Every op carries out a declared tool: none is left over, reached only
/// by a name matched somewhere else.
#[test]
fn every_op_is_reached_from_a_declaration() {
    for op in crate::tools_core::ALL {
        assert!(all().any(|t| t.exec == Exec::Core(op)), "{op:?} carries out no declared tool");
    }
    for t in all() {
        if let Exec::Core(op) = t.exec {
            assert!(crate::tools_core::ALL.contains(&op), "{op:?} is not in tools_core::ALL");
            assert_eq!(crate::tools_core::op(t.name), Some(op), "{}", t.name);
        }
    }
}

/// The files whose dispatch and wording come from the registry name no
/// declared tool themselves (their tests aside): a tool is added in one place.
#[test]
fn dispatch_does_not_match_a_tool_by_name() {
    let files = [
        ("tools.rs", include_str!("../tools.rs")),
        ("tools_core.rs", include_str!("../tools_core.rs")),
        ("families.rs", include_str!("../families.rs")),
        ("evidence.rs", include_str!("../evidence.rs")),
        ("web_reads.rs", include_str!("../web_reads.rs")),
        ("knowledge.rs", include_str!("../knowledge.rs")),
        ("team/agent.rs", include_str!("../team/agent.rs")),
    ];
    let names: BTreeSet<String> = all().map(|t| format!("\"{}\"", t.name)).collect();
    for (file, src) in files {
        let code = src.split("#[cfg(test)]").next().unwrap_or_default();
        for (n, line) in code.lines().enumerate() {
            let line = line.split("//").next().unwrap_or_default();
            let named: Vec<&String> = names.iter().filter(|name| line.contains(name.as_str())).collect();
            assert!(named.is_empty(), "{file}:{} names {named:?}; declare what it decides in tool_registry instead", n + 1);
        }
    }
    let boundary = include_str!("../boundary.rs");
    for name in &names {
        assert!(!boundary.contains(&format!("a.tool == {name}")), "boundary.rs matches {name} by name");
    }
}

/// Stored calls of a retired record still read as a record.
#[test]
fn a_retired_record_still_reads_as_one() {
    assert!(crate::tools::RECORDS.contains(&"task_contract"));
    assert!(decl("task_contract").is_none(), "a retired record is offered again");
    for r in records() {
        assert!(crate::tools::RECORDS.contains(&r), "{r}");
    }
}

/// A card and a team thread's work say what a call does in the same words.
#[test]
fn doing_words_come_from_the_declaration() {
    assert_eq!(doing_words("task_complete"), "Finishing up");
    assert_eq!(doing_words("web_search"), "Searching the web");
    assert_eq!(doing_words("browser_navigate"), "Reading a page");
    assert_eq!(doing_words("computer_click"), "Working on its computer");
    assert_eq!(doing_words("team_read_file"), "Reading a team file");
    assert_eq!(doing_words("memory_write"), "Memory write");
    assert_eq!(doing_words("gmail__send_message"), "Send message in gmail");
}

/// The web reads' facts, as the provenance rule reads them.
#[test]
fn web_reads_are_declared() {
    assert_eq!(web_read("web_search"), WebRead::Query);
    for t in ["web_fetch", "browser_navigate"] {
        assert_eq!(web_read(t), WebRead::OpensUrl, "{t}");
    }
    assert!(navigates("browser_navigate") && !navigates("web_fetch") && !navigates("browser_as_navigate"));
    assert_eq!(web_read("browser_as_snapshot"), WebRead::No, "a page read through a person's sign-in is a source");
    for t in all().filter(|t| t.web != WebRead::No) {
        let c = t.class.map(|c| (c.app, c.risk));
        assert!(matches!(c, Some(("web" | "browser", Risk::Read))), "{} is a web read but not one", t.name);
    }
}

/// The team agent's tools are the same declarations, filtered.
#[test]
fn a_team_thread_is_offered_a_view_of_the_same_tools() {
    let team: Vec<&str> = all().filter(|t| t.in_team).map(|t| t.name).collect();
    for t in ["web_fetch", "sandbox_exec", "computer_click", "find_tools", "team_post", "team_read_knowledge"] {
        assert!(team.contains(&t), "{t} does not serve a team thread");
    }
    for t in ["memory_search", "browser_as_click", "browser_sign_in", "project_read", "open_attachment", "relationship_note"] {
        assert!(!team.contains(&t), "{t}, personal, serves a team thread");
    }
    assert!(all().filter(|t| t.person_only).all(|t| !t.in_team));
}

/// The person's computer reaches registries, app hosts and pages it opened,
/// so what its shell and screen return taints the run like any page.
#[test]
fn what_the_computer_returns_is_untrusted() {
    for name in [crate::computer::EXEC_TOOL, "computer_screenshot", "computer_click", "computer_type", "computer_key", "computer_scroll"] {
        let class = decl(name).and_then(|d| d.class.as_ref()).unwrap_or_else(|| panic!("{name} has no class"));
        assert!(class.label.untrusted, "{name} returns principal text");
    }
}
