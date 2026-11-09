//! Team workspaces: channels, roles, a team volume and one team agent. The
//! agent acts for whoever asked and never for the channel, so its authority
//! is the workspace's grants cut down to what that member's role allows.
mod agent;
mod routes;

pub use agent::BUILTINS;
pub use routes::router;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Lower roles come first. A ceiling is the lower of two.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Guest,
    Member,
    Admin,
    Owner,
}

/// One change to a workspace. A retired form keeps its reader here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Created {
        workspace: String,
        name: String,
        owner: String,
    },
    Deleted {
        workspace: String,
    },
    Member {
        workspace: String,
        person: String,
        role: Role,
    },
    Removed {
        workspace: String,
        person: String,
    },
    Channel {
        workspace: String,
        channel: String,
        name: String,
    },
    Message {
        workspace: String,
        channel: String,
        message: Message,
    },
    /// An app granted to the workspace, for these roles. Empty takes it away.
    Connector {
        workspace: String,
        app: String,
        roles: Vec<Role>,
    },
    /// A request the team agent took on, and the channel as it then stood.
    Run {
        workspace: String,
        channel: String,
        thread: Option<String>,
        task: String,
        requester: String,
        history: String,
    },
    File {
        workspace: String,
        file: File,
    },
    FileRemoved {
        workspace: String,
        name: String,
    },
    /// A file built from the requester's own data, kept back from the volume.
    Held {
        workspace: String,
        person: String,
        file: File,
    },
    /// The person shared it, or threw it away.
    Released {
        workspace: String,
        person: String,
        name: String,
    },
    Fact {
        workspace: String,
        id: String,
        text: String,
        by: String,
    },
    FactRemoved {
        workspace: String,
        id: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub text: String,
    #[serde(default)]
    pub thread: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct File {
    pub name: String,
    pub size: u64,
    /// The member it was made for or by.
    pub by: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TeamRun {
    pub workspace: String,
    pub channel: String,
    pub thread: Option<String>,
    pub task: String,
    pub requester: String,
    pub history: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub members: BTreeMap<String, Role>,
    pub channels: BTreeMap<String, Channel>,
    pub connectors: BTreeMap<String, Vec<Role>>,
    pub files: BTreeMap<String, File>,
    /// Person, then file name.
    pub held: BTreeMap<String, BTreeMap<String, File>>,
    pub facts: BTreeMap<String, String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Channel {
    pub name: String,
    pub messages: Vec<Message>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    workspaces: BTreeMap<String, Workspace>,
    runs: BTreeMap<String, TeamRun>,
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        if let Change::Created { workspace, name, owner } = c {
            let members = BTreeMap::from([(owner.clone(), Role::Owner)]);
            self.workspaces.insert(workspace.clone(), Workspace { id: workspace.clone(), name: name.clone(), members, ..Default::default() });
            return;
        }
        if let Change::Run { workspace, channel, thread, task, requester, history } = c {
            let run = TeamRun {
                workspace: workspace.clone(),
                channel: channel.clone(),
                thread: thread.clone(),
                task: task.clone(),
                requester: requester.clone(),
                history: history.clone(),
            };
            self.runs.insert(task.clone(), run);
            return;
        }
        if let Change::Deleted { workspace } = c {
            self.workspaces.remove(workspace);
            return;
        }
        match c {
            Change::Member { workspace, person, role } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.members.insert(person.clone(), *role);
                }
            }
            Change::Removed { workspace, person } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.members.remove(person);
                    w.held.remove(person);
                }
            }
            Change::Channel { workspace, channel, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.channels.insert(channel.clone(), Channel { name: name.clone(), messages: vec![] });
                }
            }
            Change::Message { workspace, channel, message } => {
                if let Some(ch) = self.workspaces.get_mut(workspace).and_then(|w| w.channels.get_mut(channel)) {
                    ch.messages.push(message.clone());
                }
            }
            Change::Connector { workspace, app, roles } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    if roles.is_empty() {
                        w.connectors.remove(app);
                    } else {
                        w.connectors.insert(app.clone(), roles.clone());
                    }
                }
            }
            Change::File { workspace, file } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.files.insert(file.name.clone(), file.clone());
                }
            }
            Change::FileRemoved { workspace, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.files.remove(name);
                }
            }
            Change::Held { workspace, person, file } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.held.entry(person.clone()).or_default().insert(file.name.clone(), file.clone());
                }
            }
            Change::Released { workspace, person, name } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    if let Some(files) = w.held.get_mut(person) {
                        files.remove(name);
                    }
                }
            }
            Change::Fact { workspace, id, text, .. } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.facts.insert(id.clone(), text.clone());
                }
            }
            Change::FactRemoved { workspace, id } => {
                if let Some(w) = self.workspaces.get_mut(workspace) {
                    w.facts.remove(id);
                }
            }
            Change::Created { .. } | Change::Deleted { .. } | Change::Run { .. } => {}
        }
    }

    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.get(id)
    }

    pub fn of(&self, person: &str) -> Vec<&Workspace> {
        self.workspaces.values().filter(|w| w.members.contains_key(person)).collect()
    }

    pub fn role(&self, workspace: &str, person: &str) -> Option<Role> {
        self.workspaces.get(workspace).and_then(|w| w.members.get(person)).copied()
    }

    /// The team request a task carries out, if it is one.
    pub fn run_of(&self, task: &str) -> Option<&TeamRun> {
        self.runs.get(task)
    }

    pub fn runs_in(&self, workspace: &str, channel: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace && r.channel == channel).collect()
    }

    pub fn runs_by(&self, workspace: &str, person: &str) -> Vec<&TeamRun> {
        self.runs.values().filter(|r| r.workspace == workspace && r.requester == person).collect()
    }
}

/// Whether a name is fit to be a file in a volume.
pub(crate) fn fit_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0') && name.len() <= MAX_FILE_NAME_BYTES
}

const MAX_FILE_NAME_BYTES: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;

    fn made() -> Ledger {
        let mut l = Ledger::default();
        l.apply(&Change::Created { workspace: "w".into(), name: "Team".into(), owner: "alice".into() });
        l
    }

    #[test]
    fn the_creator_owns_it() {
        assert_eq!(made().role("w", "alice"), Some(Role::Owner));
        assert_eq!(made().role("w", "bob"), None);
    }

    #[test]
    fn roles_are_ordered_from_the_least() {
        assert!(Role::Guest < Role::Member && Role::Member < Role::Admin && Role::Admin < Role::Owner);
    }

    #[test]
    fn removing_a_member_forgets_what_was_held_for_them() {
        let mut l = made();
        l.apply(&Change::Member { workspace: "w".into(), person: "bob".into(), role: Role::Member });
        l.apply(&Change::Held { workspace: "w".into(), person: "bob".into(), file: File { name: "f".into(), size: 1, by: "bob".into() } });
        l.apply(&Change::Removed { workspace: "w".into(), person: "bob".into() });
        assert_eq!(l.role("w", "bob"), None);
        assert!(l.workspace("w").unwrap().held.is_empty());
    }

    #[test]
    fn an_empty_grant_takes_the_app_away() {
        let mut l = made();
        l.apply(&Change::Connector { workspace: "w".into(), app: "crm".into(), roles: vec![Role::Admin] });
        assert!(l.workspace("w").unwrap().connectors.contains_key("crm"));
        l.apply(&Change::Connector { workspace: "w".into(), app: "crm".into(), roles: vec![] });
        assert!(l.workspace("w").unwrap().connectors.is_empty());
    }

    #[test]
    fn a_file_name_cannot_climb_out() {
        for bad in ["", ".", "..", "a/b", "../x"] {
            assert!(!fit_name(bad), "{bad}");
        }
        assert!(fit_name("report.pdf"));
    }
}
