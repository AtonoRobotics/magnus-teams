//! The organisation's knowledge as an admin oversees it (admin console spec,
//! Knowledge): the org context bundle that agents curate (OKF v0.2,
//! `bundle.rs`), its entity types (Ontology), what every agent receives
//! (Context), its entries (Memory), and every change agents make (Activity).
//! An admin edits, removes and rolls back through the kernel's own checks,
//! as `human:<admin>`; every change is a new version, and nothing is lost
//! from the log.

use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::Label;
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub type Refusal = (StatusCode, String);

fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("knowledge admin: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

fn bad(m: impl Into<String>) -> Refusal {
    (StatusCode::BAD_REQUEST, m.into())
}

/// The relations a type declares, each as `{type, to, cardinality}`.
pub fn relations(fm: &Value) -> Vec<Value> {
    fm["relations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| match r.as_str() {
            Some(name) => json!({ "type": name, "to": null, "cardinality": null }),
            None => json!({ "type": r["type"].as_str().or(r["name"].as_str()), "to": r["to"], "cardinality": r["cardinality"] }),
        })
        .collect()
}

/// A type's slug from its path: `/types/customer.md` is `customer`.
/// Whether a path is a person's own document (`/users/<user>/`), which no
/// admin reads, writes or rolls back: theirs alone (K2), their relationship
/// memory among them.
fn personal(path: &str) -> bool {
    crate::bundle::place(path).map_or(path.starts_with("/users/"), |(_, user)| user.is_some())
}

fn slug(path: &str) -> Option<&str> {
    path.strip_prefix("/types/")?.strip_suffix(".md")
}

impl Kernel {
    /// Every org document now (people's own are theirs, under Users), with
    /// its current frontmatter: newest version, who wrote it and when.
    pub async fn admin_bundle(&self, kinds: &[&str]) -> Result<Vec<Value>, Refusal> {
        let docs: Vec<crate::state::BundleDoc> =
            self.read(|s| s.bundle.values().filter(|d| d.scope == "org" && (kinds.is_empty() || kinds.contains(&d.kind.as_str()))).cloned().collect());
        let contents = self.log.contents(&docs.iter().map(|d| d.seq()).collect::<Vec<_>>()).await.map_err(failed)?;
        Ok(docs
            .iter()
            .map(|d| {
                let c = contents.get(&d.seq()).cloned().unwrap_or_default();
                let fm = &c["frontmatter"];
                let first = d.versions.first().map(|v| v.2);
                let last = d.versions.last();
                json!({
                    "path": d.path, "kind": d.kind, "title": fm["title"], "description": fm["description"], "status": fm["status"],
                    "type": fm["type"], "relations": if d.kind == "type" { json!(relations(fm)) } else { fm["relations"].clone() },
                    "identity_keys": fm["identity_keys"], "sources": d.sources, "version": d.version(), "created": first,
                    "changed_at": last.map(|v| v.2), "changed_by": last.map(|v| v.3.clone()), "label": d.label, "writer": d.writer,
                    "last_used": self.last_read(&d.path),
                })
            })
            .collect())
    }

    /// When a run last read a document through the bundle tools.
    fn last_read(&self, path: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.read(|s| s.runs.values().flat_map(|r| r.intents.values()).filter(|i| i.tool == "bundle_read" && i.input["path"].as_str() == Some(path)).map(|i| i.at).max())
    }

    /// One document: its text now, every version (who, when, from which
    /// run), what refers to it, and when a run last read it.
    pub async fn admin_bundle_doc(&self, path: &str) -> Result<Value, Refusal> {
        let Some(d) = self.read(|s| s.bundle.get(path).cloned()).filter(|d| d.scope == "org") else { return Err((StatusCode::NOT_FOUND, "no such document".into())) };
        let c = self.log.contents(&[d.seq()]).await.map_err(failed)?.remove(&d.seq()).unwrap_or_default();
        let runs = self.bundle_runs(&d).await;
        let versions: Vec<Value> = d.versions.iter().rev().map(|(v, seq, at, actor)| json!({ "version": v, "seq": seq, "at": at, "by": actor, "run": runs.get(seq) })).collect();
        // What refers to it: the documents linking to it, and for a type,
        // its entities.
        let others: Vec<crate::state::BundleDoc> = self.read(|s| s.bundle.values().filter(|o| o.path != path && o.scope == "org").cloned().collect());
        let oc = self.log.contents(&others.iter().map(|o| o.seq()).collect::<Vec<_>>()).await.map_err(failed)?;
        let mut referenced_by: Vec<String> = others
            .iter()
            .filter(|o| {
                oc.get(&o.seq())
                    .is_some_and(|c| c.to_string().contains(&format!("\"{path}\"")) || crate::bundle::links(c["body"].as_str().unwrap_or_default()).iter().any(|l| l == path))
            })
            .map(|o| o.path.clone())
            .collect();
        let entities: Vec<String> = match slug(path) {
            Some(s) => others.iter().filter(|o| o.path.starts_with(&format!("/entities/{s}/"))).map(|o| o.path.clone()).collect(),
            None => vec![],
        };
        referenced_by.retain(|p| !entities.contains(p));
        Ok(json!({
            "path": d.path, "kind": d.kind, "frontmatter": c["frontmatter"], "body": c["body"], "version": d.version(), "label": d.label,
            "sources": d.sources, "writer": d.writer, "shared": d.shared, "shared_everyone": d.shared_everyone, "shared_by": d.shared_by,
            "versions": versions, "entities": entities, "referenced_by": referenced_by,
            "relations": if d.kind == "type" { json!(relations(&c["frontmatter"])) } else { c["frontmatter"]["relations"].clone() },
            "last_used": self.last_read(path),
        }))
    }

    /// The run of each version, from the log.
    async fn bundle_runs(&self, d: &crate::state::BundleDoc) -> BTreeMap<i64, String> {
        let rows = self.log.of_kinds(&["bundle_written"], 100_000).await.unwrap_or_default();
        let seqs: std::collections::HashSet<i64> = d.versions.iter().map(|v| v.1).collect();
        rows.into_iter().filter(|(seq, ..)| seqs.contains(seq)).filter_map(|(seq, _, _, b)| b["run"].as_str().map(|r| (seq, r.to_string()))).collect()
    }

    /// One version of a document, as it was written.
    pub async fn admin_bundle_version(&self, path: &str, version: u32) -> Result<Value, Refusal> {
        if personal(path) {
            return Err((StatusCode::NOT_FOUND, "no such version".into()));
        }
        let seq = self.read(|s| s.bundle.get(path).and_then(|d| d.versions.iter().find(|v| v.0 == version).map(|v| v.1)));
        let seq = match seq {
            Some(s) => s,
            None => self.removed_version(path, version).await?.ok_or((StatusCode::NOT_FOUND, "no such version".into()))?,
        };
        let c = self.log.contents(&[seq]).await.map_err(failed)?.remove(&seq).unwrap_or_default();
        Ok(json!({ "path": path, "version": version, "seq": seq, "frontmatter": c["frontmatter"], "body": c["body"] }))
    }

    /// A version of a document no longer in the bundle, from the log.
    async fn removed_version(&self, path: &str, version: u32) -> Result<Option<i64>, Refusal> {
        let rows = self.log.of_kinds(&["bundle_written"], 100_000).await.map_err(failed)?;
        Ok(rows.into_iter().find(|(_, _, _, b)| b["path"] == path && b["version"].as_u64() == Some(version as u64)).map(|r| r.0))
    }

    /// Write a version as an admin (`human:<sub>`): the kernel's checks, as
    /// for an agent's write, but an admin may make a type stable. The
    /// document keeps the taint and the sources it was written from.
    pub async fn admin_bundle_write(&self, sub: &str, path: &str, fm: &Value, body: &str) -> Result<u32, Refusal> {
        let kind = self.admin_bundle_check(path, fm, body).await?;
        let existing = self.read(|s| s.bundle.get(path).cloned());
        // The private content its writer's run read stays among its
        // sources: an admin's edit does not widen who may see it.
        let read: Vec<String> = existing.iter().flat_map(|d| d.sources.iter()).filter(|s| s.starts_with("private:")).cloned().collect();
        let label = existing.as_ref().map(|d| d.label).unwrap_or(Label::PRINCIPAL);
        self.admin_put(sub, path, kind, fm, body, label, read, None).await
    }

    /// The kernel's checks on an admin's version, as for an agent's write:
    /// its kind.
    async fn admin_bundle_check(&self, path: &str, fm: &Value, body: &str) -> Result<&'static str, Refusal> {
        let (kind, user) = crate::bundle::place(path).map_err(bad)?;
        if user.is_some() {
            return Err(bad(format!("{path}: a person's own document is theirs; it is not the organisation's knowledge")));
        }
        let tslug = path.trim_start_matches('/').split('/').nth(1).unwrap_or_default().to_string();
        let tdoc = match (kind, self.read(|s| s.bundle.get(&format!("/types/{tslug}.md")).cloned())) {
            ("entity", Some(d)) => self.log.contents(&[d.seq()]).await.map_err(failed)?.remove(&d.seq()).map(|c| c["frontmatter"].clone()),
            _ => None,
        };
        crate::bundle::validate(path, fm, body, |_| tdoc.clone()).map_err(bad)?;
        Ok(kind)
    }

    /// Put the version at `seq` back, as a new version, as it was written:
    /// with its own taint, the private content its run read and that run's
    /// person, not the current version's or none if the document was
    /// removed (M1, K2).
    async fn admin_bundle_put_back(&self, sub: &str, path: &str, seq: i64) -> Result<u32, Refusal> {
        let history = self.log.bundle_history(path, seq).await.map_err(failed)?;
        let Some(b) = history.last().filter(|(s, b)| *s == seq && b["kind"] == "bundle_written").map(|r| r.1.clone()) else {
            return Err((StatusCode::NOT_FOUND, "no such version".into()));
        };
        let c = self.log.contents(&[seq]).await.map_err(failed)?.remove(&seq).unwrap_or_default();
        let (fm, body) = (&c["frontmatter"], c["body"].as_str().unwrap_or_default());
        let kind = self.admin_bundle_check(path, fm, body).await?;
        // Unreadable, it counts as both: never put back cleaner than written.
        let label = serde_json::from_value(b["label"].clone()).unwrap_or(Label { untrusted: true, private: true });
        let sources: Vec<String> = b["sources"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        let read: Vec<String> = sources.iter().filter(|s| s.starts_with("private:")).cloned().collect();
        // Whose run read them: the bundle's writer at that version, from
        // the path's writes and removals up to it, as the state had it.
        let mut writer = None;
        for (_, e) in &history {
            if e["kind"] == "bundle_removed" {
                writer = None;
            } else if let Some(w) = e["writer"].as_str() {
                writer = Some(w.to_string());
            } else if let Some(r) = e["run"].as_str() {
                writer = self.run_person(r).await?;
            }
        }
        // Seen only by its writer, and that person is not known: never
        // the current writer's instead.
        let public = sources.iter().all(|s| s.starts_with('/') || s.starts_with("human:"));
        if writer.is_none() && !public {
            return Err((StatusCode::CONFLICT, "Whose run wrote this version is no longer known, so it cannot be put back.".into()));
        }
        self.admin_put(sub, path, kind, fm, body, label, read, writer).await
    }

    /// The person whose run `run` is: from the state, or from the log once
    /// its task is in the trash.
    async fn run_person(&self, run: &str) -> Result<Option<String>, Refusal> {
        if let Some(p) = self.read(|s| s.runs.get(run).map(|r| r.person.clone())) {
            return Ok(Some(p));
        }
        let evs = self.log.run_events(run).await.map_err(failed)?;
        Ok(evs.into_iter().find_map(|e| match e.ev {
            Ev::TaskCreated { person, .. } => Some(person),
            _ => None,
        }))
    }

    /// Write an admin's version with the taint, the private content read
    /// and the writer it carries over (K2).
    #[allow(clippy::too_many_arguments)]
    async fn admin_put(&self, sub: &str, path: &str, kind: &str, fm: &Value, body: &str, label: Label, read: Vec<String>, writer: Option<String>) -> Result<u32, Refusal> {
        let mut sources: Vec<String> = fm["sources"].as_array().into_iter().flatten().filter_map(|x| x["resource"].as_str().map(String::from)).collect();
        for s in read {
            if !sources.contains(&s) {
                sources.push(s);
            }
        }
        let ev = Ev::BundleWritten {
            path: path.into(),
            // Numbered by `emit`, under the write lock.
            version: 0,
            doc_kind: kind.into(),
            scope: "org".into(),
            owner_user: None,
            actor: format!("human:{sub}"),
            run: None,
            label,
            sources,
            writer,
            frontmatter: fm.clone(),
            body: body.into(),
        };
        let e = self.emit(&format!("admin:{sub}"), ev).await.map_err(failed)?;
        // The writer stays the person whose run read its sources (K2).
        Ok(e.ev.bundle_version())
    }

    /// Remove an org document. A type in use (it has entities, or another
    /// document refers to it) is merged or emptied first.
    pub async fn admin_bundle_remove(&self, sub: &str, path: &str) -> Result<Value, Refusal> {
        let d = self.admin_bundle_doc(path).await?;
        let in_use = d["entities"].as_array().is_some_and(|e| !e.is_empty()) || (d["kind"] == "type" && d["referenced_by"].as_array().is_some_and(|r| !r.is_empty()));
        if in_use {
            return Err((StatusCode::CONFLICT, "This type is in use: merge it into another, or move its entries, first.".into()));
        }
        self.emit(&format!("admin:{sub}"), Ev::BundleRemoved { path: path.into() }).await.map_err(failed)?;
        Ok(json!({ "removed": path }))
    }

    /// Put an earlier version back, as a new version: through the kernel's
    /// checks, as it was written.
    pub async fn admin_bundle_rollback(&self, sub: &str, path: &str, version: u32) -> Result<u32, Refusal> {
        let v = self.admin_bundle_version(path, version).await?;
        self.admin_bundle_put_back(sub, path, v["seq"].as_i64().unwrap_or_default()).await
    }

    /// Merge one type into another: each of its entities moves to the other
    /// type (re-typed), every relation that pointed to it points to the
    /// other, and it is removed. Each step is a version, so each can be
    /// rolled back.
    pub async fn admin_merge_types(&self, sub: &str, from: &str, into: &str) -> Result<Value, Refusal> {
        if from == into {
            return Err(bad("merge a type into another one"));
        }
        let (Some(fslug), Some(islug)) = (slug(from), slug(into)) else { return Err(bad("merge one type document (/types/<name>.md) into another")) };
        let target = self.admin_bundle_doc(into).await?;
        let source = self.admin_bundle_doc(from).await?;
        let title = target["frontmatter"]["title"].clone();
        let mut moved = 0;
        for e in source["entities"].as_array().cloned().unwrap_or_default() {
            let old = e.as_str().unwrap_or_default().to_string();
            let doc = self.admin_bundle_doc(&old).await?;
            let mut fm = doc["frontmatter"].clone();
            fm["type"] = title.clone();
            let new_path = old.replacen(&format!("/entities/{fslug}/"), &format!("/entities/{islug}/"), 1);
            if self.read(|s| s.bundle.contains_key(&new_path)) {
                return Err((StatusCode::CONFLICT, format!("{new_path} is there already: rename one of them first")));
            }
            // It moves with what it was written from: its taint, the
            // private content its run read and whose run that was (K2).
            let Some(was) = self.read(|s| s.bundle.get(&old).cloned()) else { return Err((StatusCode::CONFLICT, format!("{old} changed during the merge; try again"))) };
            let body = doc["body"].as_str().unwrap_or_default();
            let kind = self.admin_bundle_check(&new_path, &fm, body).await?;
            let read: Vec<String> = was.sources.iter().filter(|s| s.starts_with("private:")).cloned().collect();
            self.admin_put(sub, &new_path, kind, &fm, body, was.label, read, was.writer.clone()).await?;
            self.emit(&format!("admin:{sub}"), Ev::BundleRemoved { path: old }).await.map_err(failed)?;
            moved += 1;
        }
        // Relations and links that named the old type now name the new one.
        let mut repointed = 0;
        for r in source["referenced_by"].as_array().cloned().unwrap_or_default() {
            let p = r.as_str().unwrap_or_default().to_string();
            let doc = self.admin_bundle_doc(&p).await?;
            let fm: Value = serde_json::from_str(&doc["frontmatter"].to_string().replace(&format!("\"{from}\""), &format!("\"{into}\""))).unwrap_or(doc["frontmatter"].clone());
            let body = doc["body"].as_str().unwrap_or_default().replace(&format!("]({from})"), &format!("]({into})"));
            self.admin_bundle_write(sub, &p, &fm, &body).await?;
            repointed += 1;
        }
        self.emit(&format!("admin:{sub}"), Ev::BundleRemoved { path: from.into() }).await.map_err(failed)?;
        Ok(json!({ "merged": from, "into": into, "entities_moved": moved, "documents_repointed": repointed }))
    }

    /// Every change to the organisation's knowledge, newest first: who made
    /// it (an agent, `magnus/<version>`, or an admin, `human:<id>`), what
    /// changed, from which run and sources. `agents_only` leaves out
    /// admins' own changes.
    pub async fn admin_activity(&self, agents_only: bool, limit: i64) -> Result<Vec<Value>, Refusal> {
        let rows = self.log.of_kinds(&["bundle_written", "bundle_removed"], limit.clamp(1, 2000)).await.map_err(failed)?;
        Ok(rows
            .into_iter()
            .filter(|(_, _, _, b)| b["kind"] == "bundle_removed" || b["scope"] == "org")
            .map(|(seq, at, actor, b)| {
                let removed = b["kind"] == "bundle_removed";
                let by = if removed { actor.clone() } else { b["actor"].as_str().unwrap_or(&actor).to_string() };
                let change = if removed { "removed" } else if b["version"] == 1 { "created" } else { "updated" };
                json!({
                    "seq": seq, "at": at, "by": by, "path": b["path"], "area": crate::bundle::place(b["path"].as_str().unwrap_or_default()).map(|p| p.0).unwrap_or_default(), "change": change,
                    "version": b["version"], "run": b["run"], "sources": b["sources"],
                })
            })
            .filter(|c| !agents_only || !c["by"].as_str().is_some_and(|a| a.starts_with("human:") || a.starts_with("admin:")))
            .collect())
    }

    /// One change, with what was there before and after.
    pub async fn admin_change(&self, seq: i64) -> Result<Value, Refusal> {
        let rows = self.log.of_kinds(&["bundle_written", "bundle_removed"], 100_000).await.map_err(failed)?;
        // A person's own documents are theirs alone, never the organisation's knowledge (K2).
        let Some((_, at, actor, b)) = rows.iter().find(|r| r.0 == seq && !personal(r.3["path"].as_str().unwrap_or_default())).cloned() else {
            return Err((StatusCode::NOT_FOUND, "no such change".into()));
        };
        let path = b["path"].as_str().unwrap_or_default().to_string();
        // The version before it: the newest write of the path before seq.
        let before = rows.iter().filter(|r| r.0 < seq && r.3["path"] == path.as_str() && r.3["kind"] != "bundle_removed").map(|r| r.0).max();
        let after = (b["kind"] != "bundle_removed").then_some(seq);
        let ids: Vec<i64> = before.into_iter().chain(after).collect();
        let c = self.log.contents(&ids).await.map_err(failed)?;
        let text = |s: Option<i64>| s.and_then(|s| c.get(&s)).map(|c| crate::bundle::render(&c["frontmatter"], c["body"].as_str().unwrap_or_default()));
        Ok(
            json!({ "seq": seq, "at": at, "by": b["actor"].as_str().unwrap_or(&actor), "path": path, "before": text(before), "after": text(after), "before_seq": before, "run": b["run"], "sources": b["sources"] }),
        )
    }

    /// Undo changes: each document they touched goes back to how it was
    /// before the earliest of them (a new version, or removed if it did not
    /// exist then).
    pub async fn admin_revert(&self, sub: &str, seqs: &[i64]) -> Result<Value, Refusal> {
        if seqs.is_empty() {
            return Err(bad("name the changes to undo"));
        }
        let mut earliest: BTreeMap<String, i64> = BTreeMap::new();
        for s in seqs {
            let c = self.admin_change(*s).await?;
            let p = c["path"].as_str().unwrap_or_default().to_string();
            earliest.entry(p).and_modify(|e| *e = (*e).min(*s)).or_insert(*s);
        }
        let mut restored = vec![];
        for (path, seq) in earliest {
            let c = self.admin_change(seq).await?;
            match c["before_seq"].as_i64() {
                Some(b) => {
                    self.admin_bundle_put_back(sub, &path, b).await?;
                    restored.push(json!({ "path": path, "to": "before" }));
                }
                None => {
                    if self.read(|s| s.bundle.contains_key(&path)) {
                        self.emit(&format!("admin:{sub}"), Ev::BundleRemoved { path: path.clone() }).await.map_err(failed)?;
                    }
                    restored.push(json!({ "path": path, "to": "removed" }));
                }
            }
        }
        Ok(json!({ "restored": restored }))
    }

    /// The root index as one person's agent receives it: links to what they
    /// cannot see removed, bullets that would show nothing left out (K2).
    pub async fn admin_assembled(&self, person: &str) -> Result<Value, Refusal> {
        let Some(d) = self.read(|s| s.bundle.get("/index.md").cloned()) else { return Ok(json!({ "person": person, "text": null })) };
        let c = self.log.contents(&[d.seq()]).await.map_err(failed)?.remove(&d.seq()).unwrap_or_default();
        Ok(json!({ "person": person, "text": self.without_hidden_links(person, c["body"].as_str().unwrap_or_default()), "version": d.version() }))
    }
}
