//! The knowledge base of a project, and of the workspace itself. The agent
//! writes to it directly and curators keep it right afterwards, so a wrong
//! entry costs a correction and not a review step.
use super::{Role, Workspace};
use serde::{Deserialize, Serialize};

/// How much a reader should lean on an entry, from where it came.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    #[default]
    Member,
    /// From a guest, inbound email or the web.
    Outside,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Relation {
    pub kind: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Flag {
    pub by: String,
    pub why: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Retired {
    pub by: String,
    pub at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub superseded_by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Entry {
    pub id: String,
    /// `None` is the workspace's own base, facts about the team itself.
    #[serde(default)]
    pub project: Option<String>,
    pub kind: String,
    pub statement: String,
    pub source: String,
    pub weight: Weight,
    pub author: String,
    pub owner: String,
    #[serde(default)]
    pub thread: Option<String>,
    pub reviewed_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub flags: Vec<Flag>,
    #[serde(default)]
    pub retired: Option<Retired>,
}

impl Entry {
    /// What a reader is given. A retired entry is never read as current.
    pub fn line(&self) -> String {
        let outside = if self.weight == Weight::Outside { "; from outside the workspace" } else { "" };
        let flagged = if self.flags.is_empty() { "" } else { "; flagged as possibly wrong" };
        format!("- [{}] ({}; source: {}{outside}{flagged}) {}\n", self.id, self.kind, self.source, self.statement)
    }
}

/// A project in a workspace: its own members and knowledge base.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub owner: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub curators: Vec<String>,
    /// The types of entry this project uses. Empty allows any.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// The relations this project uses. Empty allows any.
    #[serde(default)]
    pub relations: Vec<String>,
}

impl Project {
    pub fn may_read(&self, person: &str) -> bool {
        self.owner == person || self.members.iter().any(|m| m == person) || self.curators.iter().any(|c| c == person)
    }

    pub fn may_curate(&self, person: &str) -> bool {
        self.owner == person || self.curators.iter().any(|c| c == person)
    }
}

impl Workspace {
    /// Whether a person, or an agent acting for them, may read this base.
    pub fn may_read(&self, project: Option<&str>, person: &str) -> bool {
        match project {
            None => self.members.contains_key(person),
            Some(p) => self.projects.get(p).is_some_and(|p| p.may_read(person)),
        }
    }

    /// Whether they may correct, retire or delete in this base.
    pub fn may_curate(&self, project: Option<&str>, person: &str) -> bool {
        match project {
            None => self.members.get(person).is_some_and(|r| *r >= Role::Admin),
            Some(p) => self.projects.get(p).is_some_and(|p| p.may_curate(person)),
        }
    }

    /// The entries of a base that are current.
    pub fn entries(&self, project: Option<&str>) -> Vec<&Entry> {
        self.knowledge.values().filter(|e| e.project.as_deref() == project && e.retired.is_none()).collect()
    }

    /// Whether this kind of entry fits a base. The project sets its own.
    pub fn allows_kind(&self, project: Option<&str>, kind: &str) -> bool {
        project.and_then(|p| self.projects.get(p)).is_none_or(|p| p.kinds.is_empty() || p.kinds.iter().any(|k| k == kind))
    }

    pub fn allows_relation(&self, project: Option<&str>, relation: &str) -> bool {
        project.and_then(|p| self.projects.get(p)).is_none_or(|p| p.relations.is_empty() || p.relations.iter().any(|k| k == relation))
    }

    /// One ordinary file per base, its relations and history in plain view.
    pub fn export(&self) -> Vec<(String, String)> {
        let file = |title: &str, project: Option<&str>| {
            let mut text = format!("# {title}\n");
            for e in self.knowledge.values().filter(|e| e.project.as_deref() == project) {
                text.push_str(&format!(
                    "\n## {}\n\n{}\n\n- type: {}\n- source: {}\n- weight: {:?}\n- author: {}\n- owner: {}\n- last reviewed: {}\n",
                    e.id,
                    e.statement,
                    e.kind,
                    e.source,
                    e.weight,
                    e.author,
                    e.owner,
                    e.reviewed_at.to_rfc3339()
                ));
                for r in &e.relations {
                    text.push_str(&format!("- {}: {}\n", r.kind, r.to));
                }
                for f in &e.flags {
                    text.push_str(&format!("- flagged by {}: {}\n", f.by, f.why));
                }
                if let Some(r) = &e.retired {
                    text.push_str(&format!(
                        "- retired by {} at {}{}\n",
                        r.by,
                        r.at.to_rfc3339(),
                        r.superseded_by.as_ref().map(|s| format!(", superseded by {s}")).unwrap_or_default()
                    ));
                }
            }
            text
        };
        let mut files = vec![("workspace.md".to_string(), file(&self.name, None))];
        files.extend(self.projects.iter().map(|(id, p)| (format!("project-{id}.md"), file(&p.name, Some(id)))));
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(weight: Weight) -> Entry {
        Entry {
            id: "e".into(),
            project: None,
            kind: "decision".into(),
            statement: "We ship on Friday".into(),
            source: "thread".into(),
            weight,
            author: "agent:alice".into(),
            owner: "alice".into(),
            thread: None,
            reviewed_at: chrono::Utc::now(),
            relations: vec![],
            flags: vec![],
            retired: None,
        }
    }

    #[test]
    fn where_an_entry_came_from_is_on_its_line() {
        assert!(entry(Weight::Outside).line().contains("from outside"));
        assert!(!entry(Weight::Member).line().contains("from outside"));
    }

    #[test]
    fn a_project_is_open_only_to_its_people() {
        let p = Project { id: "p".into(), name: "P".into(), owner: "a".into(), members: vec!["b".into()], curators: vec!["c".into()], ..Default::default() };
        assert!(p.may_read("a") && p.may_read("b") && p.may_read("c"));
        assert!(!p.may_read("d"));
        assert!(p.may_curate("c") && !p.may_curate("b"));
    }

    #[test]
    fn a_retired_entry_is_never_read_as_current() {
        let mut w = Workspace::default();
        let mut e = entry(Weight::Member);
        w.knowledge.insert("e".into(), e.clone());
        assert_eq!(w.entries(None).len(), 1);
        e.retired = Some(Retired { by: "alice".into(), at: chrono::Utc::now(), superseded_by: None });
        w.knowledge.insert("e".into(), e);
        assert!(w.entries(None).is_empty());
        assert!(w.export()[0].1.contains("retired by alice"), "the history stays in the export");
    }

    #[test]
    fn a_project_is_read_only_by_its_people_even_an_admin() {
        let mut w = Workspace::default();
        w.members.insert("admin".into(), Role::Admin);
        w.members.insert("in".into(), Role::Member);
        w.projects.insert("p".into(), Project { owner: "in".into(), ..Default::default() });
        assert!(w.may_read(Some("p"), "in"));
        assert!(!w.may_read(Some("p"), "admin"));
        assert!(w.may_read(None, "admin"));
        assert!(w.may_curate(None, "admin") && !w.may_curate(None, "in"));
        assert!(w.may_curate(Some("p"), "in") && !w.may_curate(Some("p"), "admin"));
    }

    #[test]
    fn a_project_sets_its_own_types() {
        let mut w = Workspace::default();
        w.projects.insert("p".into(), Project { kinds: vec!["decision".into()], relations: vec!["supersedes".into()], ..Default::default() });
        assert!(w.allows_kind(Some("p"), "decision") && !w.allows_kind(Some("p"), "gossip"));
        assert!(w.allows_kind(None, "gossip"), "the workspace's own base has no list");
        assert!(w.allows_relation(Some("p"), "supersedes") && !w.allows_relation(Some("p"), "likes"));
    }
}
