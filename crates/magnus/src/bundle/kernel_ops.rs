//! Kernel operations on the org context bundle (split from `bundle.rs`).

use super::{place, read_source, render, strip_hidden, user_segment, validate};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::Label;
use serde_json::{json, Value};

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
        let Some(d) = self.read(|s| s.library.bundle().get(path).cloned()) else {
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
        self.emit(&format!("person:{person}"), Ev::BundleShared(crate::events::BundleSharedPayload { path: path.into(), add, everyone, by: person.into() }))
            .await
            .map_err(|e| format!("{e:#}"))?;
        let d = self.read(|s| s.library.bundle().get(path).cloned()).ok_or_else(|| format!("{path}: no such document"))?;
        Ok(serde_json::json!({ "path": path, "version": d.version(), "shared": d.shared, "everyone": d.shared_everyone, "by": person }))
    }

    /// A document for `person`: its frontmatter and body, links to what
    /// they cannot see removed, and the taint it was written under.
    pub async fn bundle_read(&self, person: &str, path: &str) -> anyhow::Result<Option<(Value, String, Label, u32)>> {
        let Some(d) = self.read(|s| s.library.bundle().get(path).cloned()) else { return Ok(None) };
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
        strip_hidden(body, |target| self.read(|s| s.library.bundle().get(target).cloned()).is_some_and(|d| !self.bundle_visible(person, &d)))
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
        let existing = self.read(|s| s.library.bundle().get(path).cloned());
        if existing.as_ref().is_some_and(|d| !self.bundle_visible(person, d)) {
            return Err(format!("{path}: a document you cannot see"));
        }
        // An entity is checked against its type document as it is now.
        let tslug = path.trim_start_matches('/').split('/').nth(1).unwrap_or_default().to_string();
        let tdoc = match (kind, self.read(|s| s.library.bundle().get(&format!("/types/{tslug}.md")).cloned())) {
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
        let ev = crate::events::Ev::BundleWritten(crate::events::BundleWrittenPayload {
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
        });
        let e = self.emit("kernel", ev).await.map_err(|e| format!("{e:#}"))?;
        Ok(e.ev.bundle_version())
    }

    /// Documents `person` can see whose path, title, description or body
    /// has every word of `query`, optionally only some kinds.
    pub async fn bundle_search(&self, person: &str, query: &str, kinds: &[String], limit: usize) -> anyhow::Result<(Vec<Value>, Label)> {
        let docs: Vec<crate::state::BundleDoc> = self.read(|s| s.library.bundle().values().cloned().collect());
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
    use crate::bundle::{actor_ok, links};

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
