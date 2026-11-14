//! Only `people` reads a person's roles to decide what they may do (decision
//! `2026-11-14-one-answer-to-may-they.md`). A check written anywhere else
//! fails here, naming its file and line, so the rules cannot spread again.

use std::path::Path;

/// What a decision from a role looks like outside `people`.
const FORBIDDEN: &[&str] = &[".is_owner()", ".has_role(", ".is_developer()", "fn is_owner(", "fn has_role(", "fn is_guest("];

fn scan(dir: &Path, people: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.starts_with(people) {
            continue;
        }
        if path.is_dir() {
            scan(&path, people, found);
        } else if path.extension().is_some_and(|x| x == "rs") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for (n, line) in text.lines().enumerate() {
                if FORBIDDEN.iter().any(|f| line.contains(f)) && !line.trim_start().starts_with("//") {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn no_role_is_read_to_decide_outside_people() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = vec![];
    scan(&src, &src.join("people"), &mut found);
    assert!(found.is_empty(), "ask `people::rights` or `Kernel::authority` instead:\n{}", found.join("\n"));
}

#[test]
fn the_guard_sees_what_it_forbids() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("people")).unwrap();
    std::fs::write(d.path().join("a.rs"), "fn f(p: &Person) -> bool {\n    p.is_owner()\n}\n// p.has_role(\"x\") in a comment\n").unwrap();
    std::fs::write(d.path().join("people").join("b.rs"), "p.is_owner()").unwrap();
    let mut found = vec![];
    scan(d.path(), &d.path().join("people"), &mut found);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].ends_with("a.rs:2: p.is_owner()"), "{found:?}");
}
