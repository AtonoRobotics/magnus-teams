//! Each person's library and the organisation's: files uploaded and
//! delivered, projects and what was moved into them, the knowledge base,
//! remembered facts, the agent's playbook (I2) and the org context bundle.
//! Their content is in the content store; this is their record.

use crate::events::Ev;
use crate::forget::Erased;
use crate::log::Event;
use crate::state::Run;
use crate::tools::Label;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// A file in a person's library: one they uploaded, or one a task delivered.
#[derive(Clone, Debug)]
pub struct LibFile {
    pub id: String,
    pub person: String,
    pub name: String,
    pub media_type: String,
    pub size: usize,
    pub at: DateTime<Utc>,
    /// An upload (it can be given to a task or message), not a file a run delivered.
    pub uploaded: bool,
    /// The event that holds its content (the upload, or the run's output):
    /// its bytes are recalled from there for its person when a request
    /// needs them, never kept here (recall only, step 5).
    pub seq: i64,
    /// The run that delivered it.
    pub run: Option<String>,
    /// The chat thread it was uploaded in.
    pub thread: Option<String>,
    /// Its content's taint: an upload is private and untrusted; a delivered
    /// file has the taint its run had when it delivered it.
    pub label: Label,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub person: String,
    pub name: String,
    pub instructions: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A knowledge entry's record. What it says (its text, source, evidence
/// and value) is not kept here: it is read from the content store when the
/// entry is indexed, and every reader reads it from the index, as the
/// readers its row security allows (recall only, step 6). The title stays
/// until step 3 settles the titles shown in lists.
#[derive(Clone, Debug, Serialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub readers: Vec<Vec<String>>,
    pub integrity: String,
    pub author: String,
    pub run: Option<String>,
    pub endorsed: Option<Value>,
    /// The last share, by whom and when: who widened the readers.
    pub shared: Option<Value>,
    pub at: DateTime<Utc>,
    /// The last event that changed it, so the index knows it is current.
    pub version: i64,
    /// The event that added it: where its content is.
    pub seq: i64,
}

#[derive(Clone, Debug)]
/// A remembered fact's record. What it says (the fact and its topic) is
/// not kept here: it is recalled from the content store, for its person,
/// when a request needs it (recall only, step 4).
pub struct MemoryEntry {
    pub id: String,
    pub person: String,
    pub run: String,
    pub label: Label,
    pub sources: Vec<String>,
    /// The memory pass, or None for the run's own model.
    pub by: Option<String>,
    pub at: DateTime<Utc>,
    /// The event that wrote it: where its content is recalled from.
    pub seq: i64,
}

/// A document in the org context bundle, as the kernel guards it: where it
/// is, whose, its versions, and what it lists as sources. Its frontmatter
/// and body are in the content store (`bundle.rs`).
#[derive(Clone, Debug)]
pub struct BundleDoc {
    pub path: String,
    pub kind: String,
    pub scope: String,
    pub owner_user: Option<String>,
    pub label: Label,
    pub sources: Vec<String>,
    /// Each version: its number, the event holding it, when and by whom.
    pub versions: Vec<(u32, i64, DateTime<Utc>, String)>,
    /// The person whose run wrote the current version, under whose grant
    /// its sources were read.
    pub writer: Option<String>,
    /// Principals (`person:<id>`, `group:<id>`) the writer shared the
    /// current version with, besides; cleared by every new version, so a
    /// share covers only the text seen (#299).
    pub shared: Vec<String>,
    /// Shared with everyone (as `shared`, for the current version).
    pub shared_everyone: bool,
    /// Who shared the current version.
    pub shared_by: Option<String>,
}

impl BundleDoc {
    pub fn version(&self) -> u32 {
        self.versions.last().map_or(0, |v| v.0)
    }
    pub fn seq(&self) -> i64 {
        self.versions.last().map_or(0, |v| v.1)
    }
}

/// One item of the agent's playbook (I2).
#[derive(Clone, Debug)]
pub struct PlaybookItem {
    pub id: String,
    pub person: String,
    pub text: String,
    pub addresses: Option<String>,
    /// The run that added it, and the runs that edited it.
    pub runs: Vec<String>,
    pub label: Label,
    /// Playbook version at which the item was added, and retired.
    pub added: u64,
    pub retired: Option<u64>,
    pub added_at: DateTime<Utc>,
}

#[derive(Default, Debug)]
pub struct Library {
    /// Every person's library, by file id.
    files: BTreeMap<String, LibFile>,
    projects: BTreeMap<String, Project>,
    /// Items the person moved into a project (Some) or out of one (None).
    moved: HashMap<String, Option<String>>,
    /// The company knowledge base; its searchable index is in Postgres.
    knowledge: HashMap<String, KnowledgeEntry>,
    memory: Vec<MemoryEntry>,
    playbook: BTreeMap<String, PlaybookItem>,
    /// Increments with every playbook change (I3).
    playbook_version: u64,
    /// The org context bundle, by path.
    bundle: BTreeMap<String, BundleDoc>,
    /// The last version number each path was written with, kept through a
    /// removal: a path written again counts on, so (path, version) names
    /// one version in the log.
    bundle_last: HashMap<String, u32>,
}

/// Their files, projects, memories and own documents (their relationship
/// memory among them, with its versions: org-context.md, the owner's
/// ruling on erasure), and what they moved. The knowledge base, the
/// playbook and the org's documents are the organisation's; a path counts
/// its versions on through any removal.
impl crate::forget::Forget for Library {
    fn forget(&mut self, who: &Erased) {
        let p = who.person.as_str();
        self.memory.retain(|m| m.person != p);
        self.bundle.retain(|_, d| d.owner_user.as_deref() != Some(p));
        // Their own documents' version counts too: their id is never given
        // again, so no one writes under their folder after them.
        let theirs = format!("/users/{}/", crate::bundle::user_segment(p));
        self.bundle_last.retain(|path, _| !path.starts_with(&theirs));
        self.files.retain(|_, f| f.person != p);
        self.projects.retain(|_, x| x.person != p);
        self.moved.retain(|id, _| !who.tasks.contains(id) && !who.files.contains(id) && !who.threads.contains(id));
    }
}

impl Library {
    /// The file, project, move, knowledge, memory, playbook and bundle
    /// kinds; `runs` names a bundle writer's person. `State::apply` routes
    /// no other kind here.
    pub(crate) fn fold(&mut self, e: &Event, runs: &HashMap<String, Run>) {
        match &e.ev {
            Ev::FileUploaded { id, person, name, media_type, data_b64, thread } => {
                let f = LibFile {
                    id: id.clone(),
                    person: person.clone(),
                    name: name.clone(),
                    media_type: media_type.clone(),
                    size: data_b64.len() * 3 / 4,
                    at: e.at,
                    uploaded: true,
                    seq: e.seq,
                    run: None,
                    thread: thread.clone(),
                    label: Label { untrusted: true, private: true },
                };
                self.files.insert(id.clone(), f);
            }
            Ev::ProjectSet { id, person, name, instructions } => {
                let created_at = self.projects.get(id).map_or(e.at, |p| p.created_at);
                self.projects.insert(id.clone(), Project { id: id.clone(), person: person.clone(), name: name.clone(), instructions: instructions.clone(), created_at });
            }
            Ev::Moved { item, project } => {
                self.moved.insert(item.clone(), project.clone());
            }
            Ev::BundleWritten { path, version, doc_kind: kind, scope, owner_user, actor, run, label, sources, writer: named, .. } => {
                let writer = named.clone().or_else(|| run.as_ref().and_then(|r| runs.get(r)).map(|r| r.person.clone()));
                let d = self.bundle.entry(path.clone()).or_insert_with(|| BundleDoc {
                    path: path.clone(),
                    kind: kind.clone(),
                    scope: scope.clone(),
                    owner_user: owner_user.clone(),
                    label: *label,
                    sources: vec![],
                    versions: vec![],
                    writer: None,
                    shared: vec![],
                    shared_everyone: false,
                    shared_by: None,
                });
                // An admin's edit (no run, no writer named) keeps the writer
                // whose run read its sources (K2).
                if run.is_some() || named.is_some() {
                    d.writer = writer;
                }
                d.kind = kind.clone();
                d.scope = scope.clone();
                d.owner_user = owner_user.clone();
                d.label = *label;
                d.sources = sources.clone();
                d.versions.push((*version, e.seq, e.at, actor.clone()));
                // A new version is new text: an earlier share does not cover it.
                d.shared.clear();
                d.shared_everyone = false;
                d.shared_by = None;
                let last = self.bundle_last.entry(path.clone()).or_default();
                *last = (*last).max(*version);
            }
            Ev::BundleShared { path, add, everyone, by } => {
                if let Some(d) = self.bundle.get_mut(path) {
                    for p in add {
                        if !d.shared.contains(p) {
                            d.shared.push(p.clone());
                        }
                    }
                    d.shared_everyone |= *everyone;
                    d.shared_by = Some(by.clone());
                }
            }
            Ev::BundleRemoved { path } => {
                self.bundle.remove(path);
            }
            Ev::Memory { id, person, run, label, sources, by, .. } => self.memory.push(MemoryEntry {
                id: id.clone(),
                person: person.clone(),
                run: run.clone(),
                label: *label,
                sources: sources.clone(),
                by: by.clone(),
                at: e.at,
                seq: e.seq,
            }),
            Ev::KnowledgeAdded { id, title, readers, integrity, author, run, .. } => {
                self.knowledge.insert(
                    id.clone(),
                    KnowledgeEntry {
                        id: id.clone(),
                        title: title.clone(),
                        readers: readers.clone(),
                        integrity: integrity.clone(),
                        author: author.clone(),
                        run: run.clone(),
                        endorsed: None,
                        shared: None,
                        at: e.at,
                        version: e.seq,
                        seq: e.seq,
                    },
                );
            }
            Ev::KnowledgeShared { id, readers, by } => {
                if let Some(k) = self.knowledge.get_mut(id) {
                    k.readers = readers.clone();
                    k.shared = Some(serde_json::json!({ "by": by, "at": e.at }));
                    k.version = e.seq;
                }
            }
            Ev::KnowledgeEndorsed { id, by, item, note } => {
                if let Some(k) = self.knowledge.get_mut(id) {
                    k.integrity = "company".into();
                    k.endorsed = Some(serde_json::json!({ "by": by, "item": item, "note": note, "at": e.at }));
                    k.version = e.seq;
                }
            }
            Ev::Playbook { item, op, person, run, text, addresses, label } => {
                self.playbook_version += 1;
                let v = self.playbook_version;
                match op.as_str() {
                    "add" => {
                        self.playbook.insert(
                            item.clone(),
                            PlaybookItem {
                                id: item.clone(),
                                person: person.clone(),
                                text: text.clone().unwrap_or_default(),
                                addresses: addresses.clone(),
                                runs: vec![run.clone()],
                                label: *label,
                                added: v,
                                retired: None,
                                added_at: e.at,
                            },
                        );
                    }
                    "edit" => {
                        if let Some(i) = self.playbook.get_mut(item) {
                            if let Some(t) = text {
                                i.text = t.clone();
                            }
                            if addresses.is_some() {
                                i.addresses = addresses.clone();
                            }
                            i.runs.push(run.clone());
                            i.label = i.label.join(*label);
                        }
                    }
                    "retire" => {
                        if let Some(i) = self.playbook.get_mut(item) {
                            i.retired = Some(v);
                        }
                    }
                    _ => {}
                }
            }
            Ev::MemoryForgotten { id } => self.memory.retain(|m| &m.id != id),
            _ => {}
        }
    }

    /// A file a run delivered, or one back from the trash.
    pub(crate) fn put_file(&mut self, f: LibFile) {
        self.files.insert(f.id.clone(), f);
    }

    pub(crate) fn put_project(&mut self, p: Project) {
        self.projects.insert(p.id.clone(), p);
    }

    /// A memory back from the trash, in its place by time.
    pub(crate) fn put_memory(&mut self, m: MemoryEntry) {
        let at = self.memory.partition_point(|x| x.at <= m.at);
        self.memory.insert(at, m);
    }

    pub(crate) fn put_knowledge(&mut self, k: KnowledgeEntry) {
        self.knowledge.insert(k.id.clone(), k);
    }

    /// What goes to the trash: a file, a project, a memory or a knowledge entry.
    pub(crate) fn take_file(&mut self, id: &str) -> Option<LibFile> {
        self.files.remove(id)
    }

    pub(crate) fn take_project(&mut self, id: &str) -> Option<Project> {
        self.projects.remove(id)
    }

    pub(crate) fn take_memory(&mut self, id: &str) -> Option<MemoryEntry> {
        let i = self.memory.iter().position(|m| m.id == id)?;
        Some(self.memory.remove(i))
    }

    pub(crate) fn take_knowledge(&mut self, id: &str) -> Option<KnowledgeEntry> {
        self.knowledge.remove(id)
    }

    pub fn files(&self) -> &BTreeMap<String, LibFile> {
        &self.files
    }

    pub fn projects(&self) -> &BTreeMap<String, Project> {
        &self.projects
    }

    pub fn moved(&self) -> &HashMap<String, Option<String>> {
        &self.moved
    }

    pub fn knowledge(&self) -> &HashMap<String, KnowledgeEntry> {
        &self.knowledge
    }

    /// Remembered facts, oldest first.
    pub fn memory(&self) -> &Vec<MemoryEntry> {
        &self.memory
    }

    pub fn playbook(&self) -> &BTreeMap<String, PlaybookItem> {
        &self.playbook
    }

    pub fn playbook_version(&self) -> u64 {
        self.playbook_version
    }

    /// The person's playbook items that were live at playbook version `v`.
    pub fn playbook_at(&self, person: &str, v: u64) -> Vec<&PlaybookItem> {
        self.playbook.values().filter(|i| i.person == person && i.added <= v && i.retired.is_none_or(|r| r > v)).collect()
    }

    pub fn bundle(&self) -> &BTreeMap<String, BundleDoc> {
        &self.bundle
    }

    pub fn bundle_last(&self) -> &HashMap<String, u32> {
        &self.bundle_last
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("files", format!("{:?}", self.files)),
            ("projects", format!("{:?}", self.projects)),
            ("moved", format!("{:?}", self.moved)),
            ("knowledge", format!("{:?}", self.knowledge)),
            ("memory", format!("{:?}", self.memory)),
            ("playbook", format!("{:?}", self.playbook)),
            ("playbook_version", format!("{:?}", self.playbook_version)),
            ("bundle", format!("{:?}", self.bundle)),
            ("bundle_last", format!("{:?}", self.bundle_last)),
        ]
    }
}
