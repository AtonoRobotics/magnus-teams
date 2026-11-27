//! The org context bundle (the owner's org context build spec, Bundle
//! format; `docs/proposals/org-context.md`, step 1): an OKF v0.2 bundle with
//! the Magnus profile. Seven kinds of document, each with the fields the
//! kernel checks on every write; a write that breaks a rule is refused with
//! the reason and nothing is stored (K8); `/index.md` is at most 16KB (K9);
//! every write is a new version. Documents are events (`BundleWritten`):
//! the log keeps where each is and whose, its frontmatter and body are in
//! the content store, which the kernel reads itself, deciding who may see
//! what (K2).

mod kernel_ops;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The root index's size limit, before filtering, so every person's view
/// of it fits (K9).
pub const INDEX_CAP: usize = 16 * 1024;

/// Where a path puts a document: its kind, and whose it is when it is a
/// person's own (under `/users/<user>/`).
pub fn place(path: &str) -> Result<(&'static str, Option<String>), String> {
    let p = path.strip_prefix('/').ok_or_else(|| format!("{path}: a bundle path starts with /"))?;
    if !p.ends_with(".md") {
        return Err(format!("{path}: a bundle document is a .md file"));
    }
    let segs: Vec<&str> = p.split('/').collect();
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == ".." || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))) {
        return Err(format!("{path}: each part of a path is letters, digits, -, _ or ."));
    }
    let file = *segs.last().ok_or_else(|| format!("{path}: a bundle path starts with /"))?;
    let user = (segs[0] == "users" && segs.len() >= 3).then(|| segs[1].to_string());
    if segs[0] == "users" && user.is_none() {
        return Err(format!("{path}: a person's documents are under /users/<user>/"));
    }
    let kind = match (file, segs.as_slice()) {
        ("index.md", _) => "index",
        ("log.md", s) if s.len() >= 2 => "log",
        ("log.md", _) => return Err(format!("{path}: a log is /<area>/log.md")),
        _ if user.is_some() => "user",
        (_, ["sources", _]) => "source",
        (_, ["types", _]) => "type",
        (_, ["entities", _, _]) => "entity",
        (_, ["conventions", _, _]) => "convention",
        (_, ["people", _]) => "people",
        _ => return Err(format!("{path}: not a place in the bundle (sources/, types/, entities/<type>/, conventions/<area>/, people/, users/<user>/, an index.md or a log.md)")),
    };
    Ok((kind, user))
}

/// A person id as a `/users/` path segment: every id the appliance
/// accepts maps to a valid one, and two ids never map to the same one
/// (#244). Bytes the path rule takes stay (`-` doubles, so it cannot
/// start an escape); every other byte is `-` with its uppercase hex.
/// The only post-pass: an id of `.` or `..` would refuse as a segment,
/// and an empty id would vanish, so those escape their first byte.
pub fn user_segment(person: &str) -> String {
    let mut out = String::new();
    for b in person.bytes() {
        if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
            out.push(b as char);
        } else if b == b'-' {
            out.push_str("--");
        } else {
            out.push_str(&format!("-{b:02X}"));
        }
    }
    if out.is_empty() {
        return "-".into();
    }
    if out == "." || out == ".." {
        out.replace_range(0..1, "-2E");
    }
    out
}

/// The fields each kind needs beyond OKF's `type`, and the `type` it must
/// carry (an entity's is its type document's title).
fn required(kind: &str) -> (&'static [&'static str], Option<&'static str>) {
    match kind {
        "source" => (&["title", "description", "resource", "sources", "generated"], Some("Source")),
        "type" => (&["title", "description", "generated", "status", "system_of_record", "identity_keys", "relations", "cedar_entity"], Some("Entity Type")),
        "entity" => (&["title", "resource", "sources", "generated", "identity", "relations"], None),
        "convention" => (&["title", "description", "sources", "generated", "verified", "status"], Some("Convention")),
        "people" => (&["title", "owner", "ask_channel"], Some("Area")),
        _ => (&[], None),
    }
}

const VERIFICATIONS: [&str; 6] = ["owner_review", "downstream_result", "correction", "rejection", "acceptance", "recheck"];

/// An OKF actor: `human:<id>`, `process:<id>`, or `<producer>/<version>`.
pub fn actor_ok(a: &str) -> bool {
    let plain = |s: &str| !s.is_empty() && !s.chars().any(char::is_whitespace);
    match a.split_once(':') {
        Some(("human" | "process", id)) => plain(id),
        Some(_) => false,
        None => a.split_once('/').is_some_and(|(p, v)| plain(p) && plain(v)),
    }
}

fn time_ok(v: &Value) -> bool {
    v.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
}

/// A link the profile accepts: bundle-absolute, or a source's URI.
fn link_ok(l: &str) -> bool {
    l.starts_with('/')
        || l.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
}

/// Every Markdown link target in a body: inline, and each reference
/// definition's.
pub fn links(body: &str) -> Vec<String> {
    let mut out: Vec<String> = body.lines().filter_map(definition).map(|(_, t)| t).collect();
    out.extend(spans(body).into_iter().filter_map(|(_, _, l)| match l {
        Link::Inline(t) => Some(t),
        Link::Ref(_) => None,
    }));
    out
}

/// A link written in a body: to its target, or by a reference's label.
enum Link {
    Inline(String),
    Ref(String),
}

/// Each link in a body with where it is: `[text](target "title")`,
/// `[text][label]`, `[label][]` and `[label]` (a reference only where the
/// label is defined). Brackets nest, so a label's own brackets do not end
/// it, and a destination may begin on the next line.
fn spans(body: &str) -> Vec<(usize, usize, Link)> {
    let b = body.as_bytes();
    let mut out = vec![];
    let mut open: Vec<usize> = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => open.push(i),
            b']' => {
                if let Some(start) = open.pop() {
                    let label = &body[start + 1..i];
                    match b.get(i + 1) {
                        Some(b'(') => {
                            if let Some(close) = closing(b, i + 1) {
                                let target = body[i + 2..close].split_whitespace().next().unwrap_or_default().trim_start_matches('<').trim_end_matches('>');
                                out.push((start, close + 1, Link::Inline(target.into())));
                                i = close + 1;
                                continue;
                            }
                        }
                        Some(b'[') => {
                            if let Some(j) = body[i + 2..].find(']').filter(|&j| !body[i + 2..i + 2 + j].contains('[')) {
                                let r = &body[i + 2..i + 2 + j];
                                out.push((start, i + 3 + j, Link::Ref(if r.is_empty() { label } else { r }.to_lowercase())));
                                i += 3 + j;
                                continue;
                            }
                        }
                        Some(b':') => {}
                        _ => out.push((start, i + 1, Link::Ref(label.to_lowercase()))),
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The `)` that closes the `(` at `at`, parentheses nesting.
fn closing(b: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0;
    for (k, &c) in b.iter().enumerate().skip(at) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// A reference definition, `[label]: target`: the line starts with it,
/// and the label holds no brackets.
fn definition(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix('[')?;
    let end = rest.find(']')?;
    let label = &rest[..end];
    if label.is_empty() || label.contains('[') {
        return None;
    }
    let target = rest[end + 1..].strip_prefix(':')?.split_whitespace().next()?.trim_start_matches('<').trim_end_matches('>');
    Some((label.to_lowercase(), target.into()))
}

/// A body with every link to a bundle path `hidden` names removed, its text
/// with it, and each definition of one; a bullet whose links are all
/// hidden is dropped (K2): `hidden` is asked the path each link names
/// (`bundle_link::link_path`), whatever section or query follows it.
pub fn strip_hidden(body: &str, hidden: impl Fn(&str) -> bool) -> String {
    let defs: HashMap<String, String> = body.lines().filter_map(definition).collect();
    let gone = |t: &str| crate::bundle_link::link_path(t).is_some_and(|p| hidden(&p));
    // Each link to a bundle path: where it is, and whether it is hidden.
    let found: Vec<(usize, usize, bool)> = spans(body)
        .into_iter()
        .filter_map(|(s, e, l)| {
            match l {
                Link::Inline(t) => Some(t),
                Link::Ref(r) => defs.get(&r).cloned(),
            }
            .filter(|t| t.starts_with('/'))
            .map(|t| (s, e, gone(&t)))
        })
        .collect();
    let mut cut: Vec<(usize, usize)> = found.iter().filter(|f| f.2).map(|f| (f.0, f.1)).collect();
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        let (s, e) = (at, at + line.len());
        at = e;
        let here: Vec<_> = found.iter().filter(|f| f.0 >= s && f.0 < e).collect();
        let bullet = line.trim_start().starts_with("* ") || line.trim_start().starts_with("- ");
        if definition(line).is_some_and(|(_, t)| gone(&t)) || (bullet && !here.is_empty() && here.iter().all(|f| f.2)) {
            cut.push((s, here.iter().map(|f| f.1).fold(e, usize::max)));
        }
    }
    cut.sort();
    let (mut out, mut from) = (String::new(), 0);
    for (s, e) in cut {
        if e <= from {
            continue;
        }
        out.push_str(&body[from..s.max(from)]);
        from = e;
        if out.ends_with(' ') && body[from..].starts_with(' ') {
            from += 1;
        }
    }
    out.push_str(&body[from..]);
    out
}

/// Check a document before it is stored. `type_doc` gives an entity's type
/// document's frontmatter (the bundle's current one).
pub fn validate(path: &str, fm: &Value, body: &str, type_doc: impl Fn(&str) -> Option<Value>) -> Result<(), String> {
    let (kind, user) = place(path)?;
    let fm = fm.as_object().cloned().unwrap_or_default();
    for l in links(body) {
        if !link_ok(&l) {
            return Err(format!("{path}: the link {l:?} is neither a bundle path (starting with /) nor a source's URI"));
        }
    }
    if kind == "index" || kind == "log" {
        let allowed = path == "/index.md" && fm.keys().all(|k| k == "okf_version") && fm.get("okf_version").is_none_or(|v| v == "0.2");
        if !fm.is_empty() && !allowed {
            return Err(format!("{path}: an {kind} carries no frontmatter (the root index may carry okf_version: \"0.2\")"));
        }
        if path == "/index.md" && body.len() > INDEX_CAP {
            return Err(format!("/index.md is {} bytes; it may be at most {INDEX_CAP} (16KB), so every person's view of it fits", body.len()));
        }
        return if kind == "index" { index_ok(path, body) } else { log_ok(path, body) };
    }
    let ty = fm.get("type").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if ty.is_empty() {
        return Err(format!("{path}: no type"));
    }
    let (fields, expect) = required(kind);
    for f in fields {
        if fm.get(*f).is_none_or(|v| v.is_null() || v == "" || v.as_array().is_some_and(|a| a.is_empty() && !["relations", "verified"].contains(f))) {
            return Err(format!("{path}: a {kind} document needs {f}"));
        }
    }
    if let Some(e) = expect {
        if ty != e {
            return Err(format!("{path}: a {kind} document's type is {e:?}, not {ty:?}"));
        }
    }
    // Scope: a person's own documents are theirs alone; the rest are the org's.
    match (&user, fm.get("scope").and_then(Value::as_str)) {
        (Some(u), Some("user")) if fm.get("owner_user").and_then(Value::as_str).is_some_and(|o| user_segment(o) == *u) => {}
        (Some(u), _) => return Err(format!("{path}: a document under /users/{u}/ has scope: user and its person's id in owner_user")),
        (None, Some("org")) => {}
        (None, _) => return Err(format!("{path}: an org document has scope: org")),
    }
    if let Some(g) = fm.get("generated") {
        if !g["by"].as_str().is_some_and(actor_ok) {
            return Err(format!("{path}: generated.by is an actor: human:<id>, process:<id> or <producer>/<version>"));
        }
        if !time_ok(&g["at"]) {
            return Err(format!("{path}: generated.at is an ISO 8601 time with its offset"));
        }
    }
    if let Some(s) = fm.get("stale_after") {
        if !time_ok(s) {
            return Err(format!("{path}: stale_after is an ISO 8601 time with its offset"));
        }
    }
    if let Some(s) = fm.get("status") {
        if !s.as_str().is_some_and(|s| ["draft", "stable", "deprecated"].contains(&s)) {
            return Err(format!("{path}: status is draft, stable or deprecated"));
        }
    }
    if let Some(v) = fm.get("verified") {
        let list = match v {
            Value::Array(a) => a.clone(),
            Value::Object(_) => vec![v.clone()],
            _ => return Err(format!("{path}: verified is a list of {{by, at, kind, event, note}}")),
        };
        for e in &list {
            if !e["by"].as_str().is_some_and(actor_ok) || !time_ok(&e["at"]) || !e["kind"].as_str().is_some_and(|k| VERIFICATIONS.contains(&k)) {
                return Err(format!("{path}: each verification has by (an actor), at (an ISO 8601 time with its offset) and kind ({})", VERIFICATIONS.join(", ")));
            }
        }
    }
    if let Some(s) = fm.get("sources") {
        let ok = s.as_array().is_some_and(|a| a.iter().all(|x| x["resource"].as_str().is_some_and(link_ok)));
        if !ok {
            return Err(format!("{path}: sources is a list whose every entry has a resource: a bundle path or a source's URI"));
        }
    }
    if let Some(r) = fm.get("resource") {
        if !r.as_str().is_some_and(link_ok) {
            return Err(format!("{path}: resource is a bundle path or a source's URI"));
        }
    }
    if kind == "entity" {
        let tslug = path.trim_start_matches('/').split('/').nth(1).unwrap_or_default();
        let tdoc = type_doc(tslug).ok_or_else(|| format!("{path}: its type /types/{tslug}.md is not in the bundle"))?;
        if tdoc["title"].as_str() != Some(ty) {
            return Err(format!("{path}: an entity's type is its type document's title, {:?}", tdoc["title"].as_str().unwrap_or_default()));
        }
        let keys: Vec<&str> = tdoc["identity_keys"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        for k in fm["identity"].as_object().ok_or_else(|| format!("{path}: identity maps keys to values"))?.keys() {
            if !keys.contains(&k.as_str()) {
                return Err(format!("{path}: identity key {k:?} is not one of its type's identity_keys ({})", keys.join(", ")));
            }
        }
        let rels: Vec<&str> = tdoc["relations"].as_array().into_iter().flatten().filter_map(|r| r.as_str().or(r["type"].as_str()).or(r["name"].as_str())).collect();
        for r in fm["relations"].as_array().ok_or_else(|| format!("{path}: relations is a list of {{type, to}}"))? {
            let t = r["type"].as_str().unwrap_or_default();
            if !rels.contains(&t) || !r["to"].as_str().is_some_and(link_ok) {
                return Err(format!("{path}: relation {t:?} is not one its type declares ({}), or has no to", rels.join(", ")));
            }
        }
    }
    Ok(())
}

/// Headings grouping bulleted links, each with a short description.
fn index_ok(path: &str, body: &str) -> Result<(), String> {
    for l in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let bullet = (l.starts_with("* ") || l.starts_with("- ")) && l.contains("](");
        if !l.starts_with('#') && !bullet {
            return Err(format!("{path}: an index is headings and bulleted links (\"* [Title](/path.md) - description\"); not {l:?}"));
        }
    }
    Ok(())
}

/// Date-grouped entries, newest first.
fn log_ok(path: &str, body: &str) -> Result<(), String> {
    let mut last: Option<chrono::NaiveDate> = None;
    let mut first = true;
    for l in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(d) = l.strip_prefix("## ") {
            let d = chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").map_err(|_| format!("{path}: a log's sections are \"## YYYY-MM-DD\"; not {l:?}"))?;
            if last.is_some_and(|p| d > p) {
                return Err(format!("{path}: a log's dates go newest first"));
            }
            last = Some(d);
        } else if first {
            return Err(format!("{path}: a log starts with a \"## YYYY-MM-DD\" section"));
        }
        first = false;
    }
    Ok(())
}

/// A document as OKF writes it: YAML frontmatter, then the body.
pub fn render(fm: &Value, body: &str) -> String {
    let empty = fm.as_object().is_none_or(Map::is_empty);
    if empty {
        return body.to_string();
    }
    let yaml = serde_yaml_ng::to_string(fm).unwrap_or_default();
    format!("---\n{yaml}---\n\n{body}")
}

/// The source the kernel records for private content a run read from
/// `app` (its files, memory, an app; empty: privacy carried in from where
/// the run started), so a document it writes is never drawn "only from the
/// bundle or people's answers".
pub fn read_source(app: &str) -> String {
    format!("private:{}", if app.is_empty() { "unnamed" } else { app })
}
