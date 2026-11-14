//! Computers: a person's or workspace's own VM (spec, the agent computer).
//! Magnus decides who may do what and records it; `magnus-computerd` is the
//! only thing that touches a VM (`decisions/2026-11-08-computerd.md`).

mod agent_tools;
mod apps;
mod client;
mod deliver;
mod desktop;
mod devview;
mod display;
mod driving;
mod egress;
mod export;
mod halt;
mod identity;
mod idle;
mod leftovers;
mod lifecycle;
mod made;
mod one;
pub(crate) use made::{make_for_person, tell_server};
mod routes;
mod service;
mod service_version;
pub(crate) use service_version::report as service_report;
mod sources;
mod takeover;
mod wake;

pub(crate) use agent_tools::specs as agent_specs;
pub(crate) use deliver::MAX_DELIVER_BYTES;
pub(crate) use driving::request_spec;
pub use egress::migrate_sql;
use egress::surrogate as egress_surrogate;
pub(crate) use one::ONE_AT_A_TIME;
pub use routes::router;
pub use service::schedule;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Enough to tell a run what went; the log has every one.
const MAX_DELETED_FILES: usize = 200;
/// As long as a deleted file can still be in a backup.
const TELL_DAYS: i64 = 30;

/// `[computer]`: where computerd listens.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub socket: PathBuf,
    /// The connected app that holds logins. It answers `get_login` with
    /// `{ username, password }`, and the kernel fills them in itself.
    #[serde(default)]
    pub password_manager: Option<String>,
    /// Package registries, for downloads only. `*.name` is any subdomain.
    #[serde(default = "default_registries")]
    pub registries: Vec<String>,
    /// For a test's fake sites: names answered here, in order, with the last
    /// answer repeating. These are taken as public.
    #[serde(default)]
    pub public_hosts: BTreeMap<String, Vec<String>>,
    /// The same, but the private-address check still applies.
    #[serde(default)]
    pub hosts: BTreeMap<String, Vec<String>>,
    /// A PEM file of certificate authorities the egress proxy trusts beside
    /// the public ones, for a company's own sites.
    #[serde(default)]
    pub upstream_ca: Option<std::path::PathBuf>,
    /// A computer unused this long hibernates; 0 keeps it running.
    #[serde(default = "default_idle_hibernate_minutes")]
    pub idle_hibernate_minutes: u64,
    /// A computer running this long without a stop hibernates, busy or not,
    /// unless a person holds it; 0 sets no limit.
    #[serde(default = "default_max_run_minutes")]
    pub max_run_minutes: u64,
}

fn default_max_run_minutes() -> u64 {
    24 * 60
}

fn default_idle_hibernate_minutes() -> u64 {
    30
}

fn default_registries() -> Vec<String> {
    ["pypi.org", "files.pythonhosted.org", "registry.npmjs.org", "deb.debian.org", "security.debian.org", "crates.io", "static.crates.io", "index.crates.io"]
        .map(String::from)
        .to_vec()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    Person(String),
    Workspace(String),
}

/// What a VM is for. A browser VM is reached only through the broker.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Role {
    #[default]
    Computer,
    /// One account at one site, with its own profile.
    Identity { site: String },
    /// A run's browser for untrusted reading: no profile to speak of.
    Throwaway { run: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KillMode {
    Pause,
    Destroy,
}

/// What happened to a computer, as the log keeps it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Created {
        id: String,
        owner: Owner,
    },
    Granted {
        id: String,
        grant: String,
        label: String,
    },
    Queued {
        id: String,
    },
    Started {
        id: String,
    },
    BrowserCreated {
        id: String,
        owner: Owner,
        role: Role,
    },
    /// The VM stopped, and an identity's profile is being sealed.
    BrowserStopped {
        id: String,
    },
    /// A secure form filled a page. The value is never written anywhere.
    SecureFormFilled {
        id: String,
        by: String,
        saved: bool,
        at: DateTime<Utc>,
    },
    /// The hash only: the token itself is held by the computer.
    SurrogateIssued {
        id: String,
        hash: String,
        name: String,
        host: String,
    },
    /// One step with the `Revoked` of each of its grants, in the same write.
    Killed {
        id: String,
        mode: KillMode,
        delete_volume: bool,
    },
    Revoked {
        id: String,
        grant: String,
    },
    Hibernated {
        id: String,
    },
    /// The machine found it no longer running though Magnus had it running
    /// (its host restarted, or its VM ended without Magnus): it is stopped,
    /// or asleep when a snapshot was kept, and the next use wakes it.
    FoundStopped {
        id: String,
        hibernated: bool,
    },
    /// Reachable by this computer from now on; a boundary action.
    DestinationAdded {
        id: String,
        host: String,
    },
    /// Reachable by this computer while one task is live; the person
    /// granted the source to that task alone.
    DestinationGranted {
        id: String,
        host: String,
        task: String,
    },
    PageOpened {
        id: String,
        page: String,
        host: String,
    },
    PageClosed {
        id: String,
        page: String,
    },
    /// Written before an install counts as done, and on an update of it.
    AppInstalled {
        id: String,
        app: App,
    },
    AppRemoved {
        id: String,
        name: String,
    },
    /// The manifest could not be replayed onto a new overlay.
    ReplayFailed {
        id: String,
        app: String,
        error: String,
    },
    /// The path only: nothing of the file's content is kept here.
    FileDeleted {
        id: String,
        path: String,
        at: DateTime<Utc>,
    },
    /// Free space was zeroed, so earlier versions of deleted files are gone.
    FreeSpaceZeroed {
        id: String,
        at: DateTime<Utc>,
    },
    /// The volume was put back from a backup taken at `at`.
    Restored {
        id: String,
        at: u64,
    },
    /// One person holds the screen; the agent is paused on it.
    TakeoverStarted {
        id: String,
        by: String,
        at: DateTime<Utc>,
        /// Why, as far as the log can tell: what the agent was waiting on,
        /// or that the person stepped in. Absent from logs before it.
        #[serde(default)]
        cause: Option<String>,
        /// The runs using the computer then.
        #[serde(default)]
        runs: Vec<String>,
    },
    TakeoverEnded {
        id: String,
        by: String,
        at: DateTime<Utc>,
    },
    /// At handback the guest stopped what the takeover left running and
    /// undid the autostart entries it made, named here; written with the
    /// `TakeoverEnded` it allowed (computer/leftovers.rs).
    TakeoverCleared {
        id: String,
        by: String,
        stopped: Vec<String>,
        stopped_count: u64,
        removed: Vec<String>,
        at: DateTime<Utc>,
    },
    /// The guest could not confirm that: the person keeps the screen and
    /// the agent stays paused.
    TakeoverUnclean {
        id: String,
        by: String,
        why: String,
        at: DateTime<Utc>,
    },
    /// The computer's user let an operator look, until `until`.
    OperatorGranted {
        id: String,
        session: String,
        operator: String,
        until: DateTime<Utc>,
        at: DateTime<Utc>,
    },
    OperatorEnded {
        id: String,
        session: String,
        at: DateTime<Utc>,
    },
    /// Written before an operator is shown anything.
    OperatorLooked {
        id: String,
        session: String,
        operator: String,
        what: String,
        at: DateTime<Utc>,
    },
    /// An owner had several computers and keeps this one, the one used
    /// last (spec rev 142, One computer, always there). The others are
    /// deleted by their own `Killed` in the same write.
    Kept {
        id: String,
        owner: Owner,
        removed: Vec<String>,
    },
    /// A reset that erased the person's files: the same computer, rebuilt
    /// from the base image and its manifest, with a new, empty volume.
    VolumeErased {
        id: String,
    },
    /// A kill switch paused it (its `Killed` with `Pause` in the same
    /// write); releasing that switch, `reason`, resumes it.
    HeldBySwitch {
        id: String,
        reason: String,
    },
    /// The look of its desktop, following its person's app theme (spec
    /// rev 145, The desktop).
    DesktopTheme {
        id: String,
        theme: Theme,
    },
}

/// A desktop's look: light or dark, as the person's app is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// One installed app, enough to install it again on a fresh overlay.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub version: String,
    pub source: String,
    /// Of the installer in the cache.
    pub sha256: String,
    /// Shell commands, with the installer's path in `$INSTALLER`.
    pub steps: Vec<String>,
    /// Destinations the app needs; `*.name` is any subdomain.
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenPage {
    pub id: String,
    pub host: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DeletedFile {
    pub path: String,
    pub at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stopped,
    Queued,
    Running,
    Paused,
    Hibernated,
    Killed,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    pub label: String,
    pub revoked: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Computer {
    pub id: String,
    pub owner: Owner,
    pub status: Status,
    pub grants: Vec<Grant>,
    /// Outside the VM, so any VM can be rebuilt from it.
    pub apps: Vec<App>,
    pub replay_error: Option<String>,
    pub deleted_files: Vec<DeletedFile>,
    #[serde(skip)]
    pub free_space_zeroed: Option<DateTime<Utc>>,
    /// Reachable because a person or the boundary said so.
    pub destinations: Vec<String>,
    /// Sources granted to one task: (host, task).
    pub task_destinations: Vec<(String, String)>,
    pub pages: Vec<OpenPage>,
    /// Its place among waiting wakes, oldest first.
    #[serde(skip)]
    queued: Option<u64>,
    /// Who holds the screen, if a person does.
    pub controller: Option<String>,
    pub operators: Vec<OperatorSession>,
    /// What only its user reads: takeovers and operator sessions.
    pub activity: Vec<Activity>,
    pub role: Role,
    pub surrogates: Vec<Surrogate>,
    /// When the log last recorded anything of it: which one an owner with
    /// several keeps.
    pub last_used: Option<DateTime<Utc>>,
    /// The kill switch whose release resumes it, while one holds it paused.
    pub held_by: Option<String>,
    /// Its desktop's look.
    pub theme: Theme,
}

/// Stands in for a credential the vault holds, at one host only.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Surrogate {
    #[serde(skip)]
    pub hash: String,
    pub name: String,
    pub host: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OperatorSession {
    pub session: String,
    pub operator: String,
    pub until: DateTime<Utc>,
    pub ended: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Activity {
    pub what: String,
    pub who: String,
    pub at: DateTime<Utc>,
}

/// The log's computers, rebuilt by replay.
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    computers: BTreeMap<String, Computer>,
    next_in_queue: u64,
    /// Each time a person took control: its cause and the runs it stopped.
    takeovers: Vec<(Option<String>, Vec<String>)>,
}

impl Ledger {
    pub fn takeovers(&self) -> &[(Option<String>, Vec<String>)] {
        &self.takeovers
    }

    pub fn get(&self, id: &str) -> Option<&Computer> {
        self.computers.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Computer> {
        self.computers.values()
    }

    /// Waiting wakes, oldest first.
    pub fn queue(&self) -> Vec<&Computer> {
        let mut waiting: Vec<&Computer> = self.computers.values().filter(|c| c.status == Status::Queued).collect();
        waiting.sort_by_key(|c| c.queued);
        waiting
    }

    /// What a run starting at `at` must be told is gone from its person's
    /// computers. Fixed by the run's start, so a replay says the same.
    pub fn deletion_section(&self, person: &str, at: DateTime<Utc>) -> String {
        let oldest = at - Duration::days(TELL_DAYS);
        let gone: Vec<String> = self
            .computers
            .values()
            .filter(|c| c.owner == Owner::Person(person.to_string()))
            .flat_map(|c| &c.deleted_files)
            .filter(|f| f.at < at && f.at >= oldest)
            .map(|f| format!("- {}\n", f.path))
            .collect();
        if gone.is_empty() {
            return String::new();
        }
        format!("\n\nThese files were deleted from the person's computer. They are gone: do not rely on them or recreate them.\n{}", gone.concat())
    }

    fn add(&mut self, id: &str, owner: &Owner, role: Role) {
        self.computers.insert(
            id.to_string(),
            Computer {
                id: id.to_string(),
                owner: owner.clone(),
                status: Status::Stopped,
                grants: vec![],
                apps: vec![],
                replay_error: None,
                deleted_files: vec![],
                free_space_zeroed: None,
                destinations: vec![],
                task_destinations: vec![],
                pages: vec![],
                queued: None,
                controller: None,
                operators: vec![],
                activity: vec![],
                role,
                surrogates: vec![],
                last_used: None,
                held_by: None,
                theme: Theme::default(),
            },
        );
    }

    /// `apply`, for an event written at `at`.
    pub fn apply_at(&mut self, change: &Change, at: DateTime<Utc>) {
        self.apply(change);
        if let Some(c) = self.computers.get_mut(change.computer()) {
            c.last_used = Some(at);
        }
    }

    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::TakeoverStarted { .. }
            | Change::TakeoverEnded { .. }
            | Change::TakeoverCleared { .. }
            | Change::TakeoverUnclean { .. }
            | Change::OperatorGranted { .. }
            | Change::OperatorEnded { .. }
            | Change::OperatorLooked { .. } => takeover::apply(self, change),
            Change::Created { id, owner } => self.add(id, owner, Role::Computer),
            Change::BrowserCreated { id, owner, role } => self.add(id, owner, role.clone()),
            Change::BrowserStopped { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Stopped;
                }
            }
            Change::SecureFormFilled { id, by, saved, at } => {
                if let Some(c) = self.computers.get_mut(id) {
                    let what = if *saved { "filled a secure form and saved it to the vault" } else { "filled a secure form" };
                    c.activity.push(Activity { what: what.to_string(), who: by.clone(), at: *at });
                }
            }
            Change::SurrogateIssued { id, hash, name, host } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.surrogates.push(Surrogate { hash: hash.clone(), name: name.clone(), host: host.clone() });
                }
            }
            Change::Granted { id, grant, label } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.grants.push(Grant { id: grant.clone(), label: label.clone(), revoked: false });
                }
            }
            Change::Queued { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Queued;
                    c.queued = Some(self.next_in_queue);
                    self.next_in_queue += 1;
                }
            }
            Change::Started { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Running;
                    c.queued = None;
                    c.replay_error = None;
                    c.held_by = None;
                }
            }
            Change::Killed { id, mode, delete_volume } => {
                if *delete_volume {
                    self.computers.remove(id);
                } else if let Some(c) = self.computers.get_mut(id) {
                    c.status = if *mode == KillMode::Pause { Status::Paused } else { Status::Killed };
                    c.queued = None;
                    c.controller = None;
                }
            }
            Change::Revoked { id, grant } => {
                if let Some(g) = self.computers.get_mut(id).and_then(|c| c.grants.iter_mut().find(|g| g.id == *grant)) {
                    g.revoked = true;
                }
            }
            Change::Hibernated { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = Status::Hibernated;
                    c.controller = None;
                }
            }
            Change::FoundStopped { id, hibernated } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.status = if *hibernated { Status::Hibernated } else { Status::Stopped };
                    c.controller = None;
                }
            }
            Change::AppInstalled { id, app } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.apps.retain(|a| a.name != app.name);
                    c.apps.push(app.clone());
                }
            }
            Change::AppRemoved { id, name } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.apps.retain(|a| a.name != *name);
                }
            }
            Change::ReplayFailed { id, app, error } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.replay_error = Some(format!("{app}: {error}"));
                }
            }
            Change::FileDeleted { id, path, at } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.deleted_files.push(DeletedFile { path: path.clone(), at: *at });
                    let over = c.deleted_files.len().saturating_sub(MAX_DELETED_FILES);
                    c.deleted_files.drain(..over);
                }
            }
            Change::FreeSpaceZeroed { id, at } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.free_space_zeroed = Some(*at);
                }
            }
            Change::Restored { .. } | Change::Kept { .. } => {}
            // What was on the old volume is gone, and so is what stood in
            // for a credential there; the apps stay, to be installed again.
            Change::DesktopTheme { id, theme } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.theme = *theme;
                }
            }
            Change::HeldBySwitch { id, reason } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.held_by = Some(reason.clone());
                }
            }
            Change::VolumeErased { id } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.deleted_files.clear();
                    c.free_space_zeroed = None;
                    c.surrogates.clear();
                }
            }
            Change::DestinationAdded { id, host } => {
                if let Some(c) = self.computers.get_mut(id) {
                    if !c.destinations.contains(host) {
                        c.destinations.push(host.clone());
                    }
                }
            }
            Change::DestinationGranted { id, host, task } => {
                if let Some(c) = self.computers.get_mut(id) {
                    let grant = (host.clone(), task.clone());
                    if !c.task_destinations.contains(&grant) {
                        c.task_destinations.push(grant);
                    }
                }
            }
            Change::PageOpened { id, page, host } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.pages.push(OpenPage { id: page.clone(), host: host.clone() });
                }
            }
            Change::PageClosed { id, page } => {
                if let Some(c) = self.computers.get_mut(id) {
                    c.pages.retain(|p| p.id != *page);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(l: &mut Ledger, id: &str) {
        l.apply(&Change::Created { id: id.into(), owner: Owner::Person("alice".into()) });
    }

    #[test]
    fn a_run_is_told_only_of_what_was_gone_when_it_started() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        let t = Utc::now();
        l.apply(&Change::FileDeleted { id: "a".into(), path: "/workspace/x".into(), at: t });
        assert!(l.deletion_section("alice", t + Duration::seconds(1)).contains("/workspace/x"));
        assert_eq!(l.deletion_section("alice", t - Duration::seconds(1)), "", "a run was told of a later deletion");
        assert_eq!(l.deletion_section("bob", t + Duration::seconds(1)), "", "another person was told");
    }

    #[test]
    fn a_manifest_follows_installs_updates_and_removals() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        let app = |v: &str| App { name: "n".into(), version: v.into(), source: "s".into(), sha256: "h".into(), steps: vec!["x".into()], hosts: vec![] };
        l.apply(&Change::AppInstalled { id: "a".into(), app: app("1") });
        l.apply(&Change::AppInstalled { id: "a".into(), app: app("2") });
        assert_eq!(l.get("a").unwrap().apps.iter().map(|a| a.version.as_str()).collect::<Vec<_>>(), ["2"]);
        l.apply(&Change::AppRemoved { id: "a".into(), name: "n".into() });
        assert!(l.get("a").unwrap().apps.is_empty());
    }

    #[test]
    fn a_change_survives_the_log_in_its_own_shape() {
        let ev = crate::events::Ev::Computer(Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        let text = serde_json::to_value(&ev).unwrap();
        assert_eq!(text["kind"], "computer");
        assert_eq!(text["change"], "killed");
        let back: crate::events::Ev = serde_json::from_value(text).unwrap();
        assert!(matches!(back, crate::events::Ev::Computer(Change::Killed { mode: KillMode::Destroy, .. })));
    }

    #[test]
    fn a_wake_waits_its_turn_and_a_kill_takes_it_out_of_the_queue() {
        let mut l = Ledger::default();
        for id in ["b", "a", "c"] {
            created(&mut l, id);
        }
        for id in ["b", "a", "c"] {
            l.apply(&Change::Queued { id: id.into() });
        }
        assert_eq!(l.queue().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        assert_eq!(l.queue().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    }

    #[test]
    fn a_rebuild_does_not_give_back_a_revoked_grant() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        l.apply(&Change::Granted { id: "a".into(), grant: "g".into(), label: "x".into() });
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: false });
        l.apply(&Change::Revoked { id: "a".into(), grant: "g".into() });
        l.apply(&Change::Started { id: "a".into() });
        let c = l.get("a").unwrap();
        assert_eq!(c.status, Status::Running);
        assert!(c.grants[0].revoked);
    }

    #[test]
    fn a_desktop_is_dark_until_its_person_asks_for_light() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        assert_eq!(l.get("a").unwrap().theme, Theme::Dark);
        l.apply(&Change::DesktopTheme { id: "a".into(), theme: Theme::Light });
        assert_eq!(l.get("a").unwrap().theme, Theme::Light);
        let text = serde_json::to_value(crate::events::Ev::Computer(Change::DesktopTheme { id: "a".into(), theme: Theme::Light })).unwrap();
        assert_eq!((text["change"].as_str(), text["theme"].as_str()), (Some("desktop_theme"), Some("light")));
    }

    #[test]
    fn deleting_the_volume_forgets_the_computer() {
        let mut l = Ledger::default();
        created(&mut l, "a");
        l.apply(&Change::Killed { id: "a".into(), mode: KillMode::Destroy, delete_volume: true });
        assert!(l.get("a").is_none());
    }
}
