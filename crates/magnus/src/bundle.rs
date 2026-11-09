//! The org context bundle (the owner's org context build spec, Bundle
//! format; `docs/proposals/org-context.md`, step 1): an OKF v0.2 bundle with
//! the Magnus profile. Seven kinds of document, each with the fields the
//! kernel checks on every write; a write that breaks a rule is refused with
//! the reason and nothing is stored (K8); `/index.md` is at most 16KB (K9);
//! every write is a new version. Documents are events (`BundleWritten`):
//! the log keeps where each is and whose, its frontmatter and body are in
//! the content store, which the kernel reads itself, deciding who may see
//! what (K2).

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::Label;
use serde_json::{json, Map, Value};
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
    let file = *segs.last().unwrap();
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
/// hidden is dropped (K2).
pub fn strip_hidden(body: &str, hidden: impl Fn(&str) -> bool) -> String {
    let defs: HashMap<String, String> = body.lines().filter_map(definition).collect();
    let gone = |t: &str| t.starts_with('/') && hidden(t);
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

impl Kernel {
    /// Whether `person` may see a document (K2): their own, or an org
    /// document whose every source they can read. Until the kernel asks
    /// each source (K3, step 4), an org document drawn from items in the
    /// org's systems, or written by a run that read private content (the
    /// sources the kernel adds, whatever the document cites), is seen only
    /// by the person whose run wrote it, under whose grant the items were
    /// read; one drawn only from the bundle, or from people's answers, by
    /// everyone.
    pub fn bundle_visible(&self, person: &str, d: &crate::state::BundleDoc) -> bool {
        if d.scope == "user" {
            return d.owner_user.as_deref() == Some(person);
        }
        if d.sources.iter().all(|s| s.starts_with('/') || s.starts_with("human:")) {
            return true;
        }
        if d.writer.as_deref() == Some(person) {
            return true;
        }
        // Shared (#299): everyone, or one of the person's principals.
        if d.shared_everyone {
            return true;
        }
        let mine = self.person(person).map(|p| crate::knowledge::principals(&p)).unwrap_or_default();
        d.shared.iter().any(|r| mine.contains(r))
    }

    /// The writer shares the current version of an org document with groups
    /// or everyone (#299): widening only, recorded as theirs. A new version
    /// clears it, so a share covers only the text seen.
    pub async fn bundle_share(&self, person: &str, path: &str, groups: &[String], everyone: bool) -> Result<serde_json::Value, String> {
        let Some(d) = self.read(|s| s.bundle.get(path).cloned()) else {
            return Err(format!("{path}: no such document, or not one the person can see"));
        };
        if d.scope != "org" {
            return Err(format!("{path}: only an org document can be shared"));
        }
        if d.writer.as_deref() != Some(person) {
            return Err(format!("{path}: no such document, or not one the person can see"));
        }
        if d.shared_everyone || (!everyone && groups.iter().all(|g| d.shared.contains(&format!("group:{g}")))) {
            return Err(format!("{path}: they may already read it"));
        }
        let add: Vec<String> = groups.iter().map(|g| format!("group:{g}")).collect();
        self.emit(&format!("person:{person}"), Ev::BundleShared { path: path.into(), add, everyone, by: person.into() }).await.map_err(|e| format!("{e:#}"))?;
        let d = self.read(|s| s.bundle.get(path).cloned()).ok_or_else(|| format!("{path}: no such document"))?;
        Ok(serde_json::json!({ "path": path, "version": d.version(), "shared": d.shared, "everyone": d.shared_everyone, "by": person }))
    }

    /// A document for `person`: its frontmatter and body, links to what
    /// they cannot see removed, and the taint it was written under.
    pub async fn bundle_read(&self, person: &str, path: &str) -> anyhow::Result<Option<(Value, String, Label, u32)>> {
        let Some(d) = self.read(|s| s.bundle.get(path).cloned()) else { return Ok(None) };
        if !self.bundle_visible(person, &d) {
            return Ok(None);
        }
        let c = self.log.contents(&[d.seq()]).await?.remove(&d.seq()).unwrap_or_default();
        let body = self.without_hidden_links(person, c["body"].as_str().unwrap_or_default());
        Ok(Some((c["frontmatter"].clone(), body, d.label, d.version())))
    }

    /// A body with every link to a document `person` cannot see removed
    /// (`strip_hidden`, K2).
    pub fn without_hidden_links(&self, person: &str, body: &str) -> String {
        strip_hidden(body, |target| self.read(|s| s.bundle.get(target).cloned()).is_some_and(|d| !self.bundle_visible(person, &d)))
    }

    /// Write a version of a document for `person`'s run: checked first, and
    /// refused with the reason when it breaks a rule (K8, K9).
    pub async fn bundle_write(&self, person: &str, run: &str, path: &str, fm: &Value, body: &str) -> Result<u32, String> {
        // A run that writes its raw id names a directory that is not
        // theirs when the id maps: the refusal names the one that is.
        let dir = format!("/users/{}/", user_segment(person));
        let (kind, user) = place(path).map_err(|e| if path.starts_with("/users/") { format!("{e}; your documents are under {dir}") } else { e })?;
        if user.as_deref().is_some_and(|u| u != user_segment(person)) {
            return Err(format!("{path}: another person's document"));
        }
        let existing = self.read(|s| s.bundle.get(path).cloned());
        if existing.as_ref().is_some_and(|d| !self.bundle_visible(person, d)) {
            return Err(format!("{path}: a document you cannot see"));
        }
        // An entity is checked against its type document as it is now.
        let tslug = path.trim_start_matches('/').split('/').nth(1).unwrap_or_default().to_string();
        let tdoc = match (kind, self.read(|s| s.bundle.get(&format!("/types/{tslug}.md")).cloned())) {
            ("entity", Some(d)) => self.log.contents(&[d.seq()]).await.map_err(|e| format!("{e:#}"))?.remove(&d.seq()).map(|c| c["frontmatter"].clone()),
            _ => None,
        };
        let before = match &existing {
            Some(d) => self.log.contents(&[d.seq()]).await.map_err(|e| format!("{e:#}"))?.remove(&d.seq()).map(|c| c["frontmatter"].clone()),
            None => None,
        };
        // How a document was checked is the kernel's record, kept from
        // version to version: a write carries it back unchanged or leaves it
        // out, and never adds, changes or drops an entry.
        let kept = before.as_ref().map(|b| b["verified"].clone()).filter(|v| !v.is_null());
        let mut fm = fm.clone();
        if let Some(asked) = fm.get("verified").filter(|v| !v.is_null()) {
            if Some(asked) != kept.as_ref() && !(kept.is_none() && asked.as_array().is_some_and(Vec::is_empty)) {
                return Err(format!("{path}: verified is the kernel's record of how this document was checked; a write keeps it as it is, and never adds or changes an entry"));
            }
        }
        if let Some(m) = fm.as_object_mut() {
            match kept {
                Some(v) => {
                    m.insert("verified".into(), v);
                }
                None if kind == "convention" => {
                    m.insert("verified".into(), json!([]));
                }
                None => {
                    m.remove("verified");
                }
            }
        }
        let fm = &fm;
        validate(path, fm, body, |_| tdoc.clone())?;
        // A type becomes a Cedar entity: only an admin makes one stable, and
        // once stable only an admin changes it, to stay or to go back.
        let was_stable = kind == "type" && before.as_ref().is_some_and(|b| b["status"] == "stable");
        if was_stable {
            return Err(format!("{path}: a stable type changes only with an admin's approval in the console; it stays as it is"));
        }
        if kind == "type" && fm["status"] == "stable" {
            return Err(format!("{path}: a type becomes stable only with an admin's approval in the console; write it as draft"));
        }
        let (label, read, writer) =
            self.read(|s| s.runs.get(run).map(|r| (Label { untrusted: r.untrusted, private: r.private }, r.private_from.clone(), Some(r.person.clone())))).unwrap_or_default();
        let mut sources: Vec<String> = fm["sources"].as_array().into_iter().flatten().filter_map(|x| x["resource"].as_str().map(String::from)).collect();
        // What the run read, whatever the document cites: each place whose
        // private content it read is a source the kernel names itself (K2).
        // A shared document written by a run that carried its person's
        // relationship memory may carry it: drawn from their memory (#258).
        let personal = match self.read(|s| s.runs.get(run).cloned()) {
            Some(r) if user.is_none() => self.carries_person(&r).await.map_err(|e| format!("{e:#}"))?,
            _ => false,
        };
        let memory = personal.then(|| "memory".to_string());
        for s in read.iter().chain(memory.iter()).map(|app| read_source(app)) {
            if !sources.contains(&s) {
                sources.push(s);
            }
        }
        let ev = crate::events::Ev::BundleWritten {
            path: path.into(),
            // Numbered by `emit`, under the write lock.
            version: 0,
            doc_kind: kind.into(),
            scope: if user.is_some() { "user".into() } else { "org".into() },
            // The person's id, not the path's segment: the two differ
            // when the id maps (#244), and the id is what the readers,
            // the erasure and the content owners compare.
            owner_user: if user.is_some() { Some(person.to_string()) } else { None },
            actor: format!("magnus/{}", crate::update::version()),
            run: Some(run.into()),
            label,
            sources,
            writer,
            frontmatter: fm.clone(),
            body: body.into(),
        };
        let e = self.emit("kernel", ev).await.map_err(|e| format!("{e:#}"))?;
        Ok(e.ev.bundle_version())
    }

    /// Documents `person` can see whose path, title, description or body
    /// has every word of `query`, optionally only some kinds.
    pub async fn bundle_search(&self, person: &str, query: &str, kinds: &[String], limit: usize) -> anyhow::Result<(Vec<Value>, Label)> {
        let docs: Vec<crate::state::BundleDoc> = self.read(|s| s.bundle.values().cloned().collect());
        let seen: Vec<crate::state::BundleDoc> = docs.into_iter().filter(|d| (kinds.is_empty() || kinds.contains(&d.kind)) && self.bundle_visible(person, d)).collect();
        let contents = self.log.contents(&seen.iter().map(|d| d.seq()).collect::<Vec<_>>()).await?;
        let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
        let mut label = Label::PRINCIPAL;
        let mut hits = vec![];
        for d in &seen {
            let c = contents.get(&d.seq()).cloned().unwrap_or_default();
            let hay = format!(
                "{} {} {} {}",
                d.path,
                c["frontmatter"]["title"].as_str().unwrap_or_default(),
                c["frontmatter"]["description"].as_str().unwrap_or_default(),
                c["body"].as_str().unwrap_or_default()
            )
            .to_lowercase();
            if words.iter().all(|w| hay.contains(w)) {
                label.untrusted |= d.label.untrusted;
                label.private |= d.label.private;
                hits.push(json!({ "path": d.path, "kind": d.kind, "title": c["frontmatter"]["title"], "description": c["frontmatter"]["description"], "status": c["frontmatter"]["status"], "version": d.version() }));
            }
            if hits.len() >= limit {
                break;
            }
        }
        Ok((hits, label))
    }
}

impl Kernel {
    /// One of the bundle tools, for the run's person: what it says back,
    /// whether it is an error, and the taint of what it returned (read by
    /// the boundary, as for a knowledge search).
    pub async fn bundle_tool(&self, run: &str, tool: &str, input: &Value) -> anyhow::Result<(String, bool, Option<Value>)> {
        let person = self.read(|s| s.runs.get(run).map(|r| r.person.clone())).unwrap_or_default();
        let s = |k: &str| input[k].as_str().unwrap_or_default().to_string();
        Ok(match tool {
            "bundle_read" => match self.bundle_read(&person, &s("path")).await? {
                Some((fm, body, label, version)) => {
                    (render(&fm, &body), false, Some(json!({ "path": s("path"), "version": version, "untrusted": label.untrusted, "private": label.private })))
                }
                None => (format!("{}: no such document, or not one the person can see", s("path")), true, Some(json!({ "untrusted": false, "private": false }))),
            },
            "bundle_search" => {
                let kinds: Vec<String> = input["kinds"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
                let limit = input["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
                let (hits, label) = self.bundle_search(&person, &s("query"), &kinds, limit).await?;
                (json!({ "hits": hits }).to_string(), false, Some(json!({ "hits": hits, "untrusted": label.untrusted, "private": label.private })))
            }
            "bundle_share" => {
                let groups: Vec<String> = input["groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect();
                if groups.is_empty() && input["everyone"] != true {
                    (format!("{}: share with groups or everyone", s("path")), true, None)
                } else {
                    match self.bundle_share(&person, &s("path"), &groups, input["everyone"] == true).await {
                        Ok(v) => (format!("shared {} (version {})", s("path"), v["version"]), false, Some(v)),
                        Err(why) => (format!("Not shared: {why}"), true, None),
                    }
                }
            }
            // The person's relationship memory enters every run of theirs
            // as their own words: it changes only through relationship_note,
            // which refuses a run that read outside content.
            _ if s("path") == crate::relationship::path(&person) => (format!("Not written: {} changes only with relationship_note", s("path")), true, None),
            _ => match self.bundle_write(&person, run, &s("path"), &input["frontmatter"], &s("body")).await {
                Ok(v) => (format!("wrote {} (version {v})", s("path")), false, Some(json!({ "path": s("path"), "version": v }))),
                Err(why) => (format!("Not written: {why}"), true, None),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gen() -> Value {
        json!({ "by": "magnus/0.2.4", "at": "2026-11-02T10:00:00+00:00" })
    }

    fn customer_type(_: &str) -> Option<Value> {
        Some(json!({ "title": "Customer", "identity_keys": ["hubspot_id", "domain"], "relations": ["owned_by", { "type": "parent" }] }))
    }

    #[test]
    fn every_place_has_its_kind() {
        assert_eq!(place("/index.md").unwrap().0, "index");
        assert_eq!(place("/sales/log.md").unwrap().0, "log");
        assert_eq!(place("/sources/hubspot.md").unwrap().0, "source");
        assert_eq!(place("/types/customer.md").unwrap().0, "type");
        assert_eq!(place("/entities/customer/acme.md").unwrap().0, "entity");
        assert_eq!(place("/conventions/sales/discounts.md").unwrap().0, "convention");
        assert_eq!(place("/people/finance.md").unwrap().0, "people");
        assert_eq!(place("/users/alice/notes.md").unwrap(), ("user", Some("alice".into())));
        for bad in ["index.md", "/a/../b.md", "/x.txt", "/random.md", "/log.md", "/users/notes.md", "/types/a b.md", "/entities/customer.md"] {
            assert!(place(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_malformed_document_is_refused_with_the_reason() {
        let no_type = validate("/sources/hubspot.md", &json!({ "title": "HubSpot" }), "", customer_type).unwrap_err();
        assert!(no_type.contains("no type"), "{no_type}");
        let fm = json!({ "type": "Source", "title": "HubSpot", "description": "CRM", "resource": "hubspot://acct/1", "sources": [{ "resource": "hubspot://acct/1" }], "generated": { "by": "magnus/0.2.4", "at": "2026-11-02 10:00" }, "scope": "org" });
        let bad_time = validate("/sources/hubspot.md", &fm, "", customer_type).unwrap_err();
        assert!(bad_time.contains("generated.at"), "{bad_time}");
        let mut ok = fm.clone();
        ok["generated"] = gen();
        validate("/sources/hubspot.md", &ok, "Search by company domain.", customer_type).unwrap();
        let mut actor = ok.clone();
        actor["generated"]["by"] = json!("Magnus");
        assert!(validate("/sources/hubspot.md", &actor, "", customer_type).unwrap_err().contains("actor"));
        let entity = |identity: Value, relations: Value| json!({ "type": "Customer", "title": "Acme", "resource": "hubspot://company/42", "sources": [{ "resource": "hubspot://company/42" }], "generated": gen(), "identity": identity, "relations": relations, "scope": "org" });
        validate("/entities/customer/acme.md", &entity(json!({ "domain": "acme.example" }), json!([{ "type": "owned_by", "to": "/people/sales.md" }])), "", customer_type).unwrap();
        let key = validate("/entities/customer/acme.md", &entity(json!({ "vat": "1" }), json!([])), "", customer_type).unwrap_err();
        assert!(key.contains("identity key \"vat\""), "{key}");
        let rel = validate(
            "/entities/customer/acme.md",
            &entity(json!({ "domain": "acme.example" }), json!([{ "type": "competes_with", "to": "/entities/customer/b.md" }])),
            "",
            customer_type,
        )
        .unwrap_err();
        assert!(rel.contains("competes_with"), "{rel}");
        assert!(validate("/entities/customer/acme.md", &entity(json!({}), json!([])), "", |_| None).unwrap_err().contains("not in the bundle"));
        // Relative links are not the profile's.
        assert!(validate("/sources/hubspot.md", &ok, "See [types](../types/customer.md).", customer_type).unwrap_err().contains("link"));
        // Scope follows the path.
        let mut user = ok.clone();
        user["scope"] = json!("user");
        assert!(validate("/sources/hubspot.md", &user, "", customer_type).unwrap_err().contains("scope: org"));
        assert!(validate("/users/alice/n.md", &json!({ "type": "Note", "scope": "user", "owner_user": "bob" }), "", customer_type).is_err());
        validate("/users/alice/n.md", &json!({ "type": "Note", "scope": "user", "owner_user": "alice" }), "", customer_type).unwrap();
    }

    #[test]
    fn every_person_id_maps_to_one_valid_user_segment() {
        for (id, seg) in [
            ("alice", "alice"),
            ("sam@example.com", "sam-40example.com"),
            ("a-b", "a--b"),
            ("-40", "--40"),
            ("sam three", "sam-20three"),
            ("Zoë", "Zo-C3-AB"),
            (".", "-2E"),
            ("..", "-2E."),
            ("", "-"),
        ] {
            assert_eq!(user_segment(id), seg, "{id}");
            assert_eq!(place(&format!("/users/{seg}/n.md")).unwrap(), ("user", Some(seg.to_string())));
        }
        // Raw and mapped never collide: the writer's directory is one only.
        assert_ne!(user_segment("sam@example.com"), user_segment("sam-40example.com"));
        // The frontmatter still names the raw id; the directory its mapping.
        validate("/users/sam-40example.com/n.md", &json!({ "type": "Note", "scope": "user", "owner_user": "sam@example.com" }), "", customer_type).unwrap();
        assert!(validate("/users/sam-40example.com/n.md", &json!({ "type": "Note", "scope": "user", "owner_user": "sam-40example.com" }), "", customer_type).is_err());
    }

    #[test]
    fn indexes_and_logs_keep_their_shape_and_the_root_index_its_size() {
        validate("/index.md", &json!({ "okf_version": "0.2" }), "# Sales\n* [HubSpot](/sources/hubspot.md) - the CRM\n", customer_type).unwrap();
        assert!(validate("/sales/index.md", &json!({ "okf_version": "0.2" }), "# Sales\n", customer_type).is_err());
        assert!(validate("/index.md", &json!({}), "Some prose.\n", customer_type).unwrap_err().contains("headings and bulleted links"));
        let big = format!("# All\n{}", "* [x](/sources/x.md) - a source\n".repeat(600));
        let e = validate("/index.md", &json!({}), &big, customer_type).unwrap_err();
        assert!(e.contains("16KB") && e.contains(&big.len().to_string()), "{e}");
        validate("/sales/log.md", &json!({}), "## 2026-11-02\nAdded HubSpot.\n## 2026-11-01\nStarted.\n", customer_type).unwrap();
        assert!(validate("/sales/log.md", &json!({}), "## 2026-11-01\nA.\n## 2026-11-02\nB.\n", customer_type).unwrap_err().contains("newest first"));
    }

    #[test]
    fn actors_are_okf_forms() {
        for ok in ["human:alice", "process:curator", "magnus/0.2.4"] {
            assert!(actor_ok(ok), "{ok}");
        }
        for bad in ["alice", "human:", "robot:x", "magnus/", "/0.2", "magnus 0.2/x"] {
            assert!(!actor_ok(bad), "{bad}");
        }
    }

    /// A hidden link is found whatever its label holds, wherever its
    /// destination begins, and a line is a reference definition only when
    /// it starts with one.
    #[test]
    fn a_hidden_link_leaves_whatever_its_label_or_line_holds() {
        let hidden = |t: &str| t == "/people/finance.md";
        assert_eq!(strip_hidden("Pay: [Carol [finance]](/people/finance.md).", hidden), "Pay: .");
        let l = strip_hidden("[Carol](/people/finance.md) and [owner]: human:alice", hidden);
        assert!(!l.contains("Carol") && !l.contains("finance"), "{l}");
        assert_eq!(strip_hidden("See [pay](\n/people/finance.md) now.", hidden), "See now.");
        assert_eq!(strip_hidden("Bonuses are in [the payroll][pay].\n\n[pay]: /people/finance.md\n", hidden), "Bonuses are in .\n\n");
        assert_eq!(links("[relative](\nrelative.md)"), vec!["relative.md"]);
        assert!(validate(
            "/people/x.md",
            &json!({ "type": "Area", "title": "X", "owner": "human:a", "ask_channel": "#x", "scope": "org" }),
            "[relative](\nrelative.md)",
            customer_type
        )
        .is_err());
    }
}
