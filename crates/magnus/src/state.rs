//! The projection of the event log. It holds nothing the log does not;
//! `State::apply` is the only writer, and a boot replays every event
//! through it. `apply` routes each kind to the domain that folds it (the
//! modules below) and orders the folds that span domains; the feature
//! ledgers fold their own kinds (decisions/2026-11-14-state-folds-per-domain.md).

use crate::events::Ev;
use crate::log::Event;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub mod apps;
pub mod arrangements;
pub mod erasure;
pub mod identity;
pub mod inbox;
pub mod library;
pub mod org;
pub mod server_ops;
pub mod thread_extras;
pub mod trash;
pub mod vault;
pub mod watch;
pub mod work;
pub use apps::ConnectorSetting;
pub use arrangements::{Answer, CardState, Commitment, CommitmentStep, Grant, GrantAction, PauseCause, Schedule};
pub use erasure::Erasure;
pub use inbox::{Approval, Item};
pub use library::{BundleDoc, KnowledgeEntry, LibFile, MemoryEntry, PlaybookItem, Project};
pub use org::PolicyVersion;
pub use server_ops::{ScrubPending, Update};
pub use thread_extras::{Branch, Disagreement};
pub use trash::{NotTrashable, TitleFrom, Trashed};
pub use vault::Revoked;
pub use work::{Cut, DecisionRec, DraftSources, Intent, Reply, ResultRec, Run, Status, Task, Tokens, Verdict, BACKGROUND_OUTPUTS, VERDICTS};

/// Debug, so a test can look through all of it for what erasing a person
/// left behind (`forget_tests.rs`).
#[derive(Default, Debug)]
pub struct State {
    pub computers: crate::computer::Ledger,
    pub payments: crate::payments::Ledger,
    pub rules: crate::rules::Ledger,
    pub evidence: crate::evidence::Ledger,
    pub sub_agents: crate::sub_agents::Ledger,
    pub compartments: crate::compartment::Ledger,
    pub envelopes: crate::envelope::Ledger,
    pub staged: crate::staging::Ledger,
    pub security: crate::security_ledger::SecurityLedger,
    pub missions: crate::team::missions::MissionLedger,
    pub prefetches: HashMap<String, (String, i64, crate::tools::Label)>,
    pub memory_superseded: HashMap<String, String>,
    pub memory_archived: HashSet<String>,
    pub outcomes: crate::outcomes::Ledger,
    pub local: crate::local::Ledger,
    pub team: crate::team::Ledger,
    pub outside_agents: crate::a2a::Ledger,
    pub account_links: crate::account_links::Ledger,
    pub legal: crate::legal::Ledger,
    pub push: crate::push::Ledger,
    pub halts: crate::halts::Ledger,
    /// The trash, and the conversations removed from it for good.
    pub trash: trash::Trash,
    pub(crate) tasks: BTreeMap<String, Task>,
    pub(crate) runs: HashMap<String, Run>,
    /// Items, approvals and the proactive loop's marks.
    pub inbox: inbox::Inbox,
    /// The organisation's settings, the admin audit and version notices.
    pub org: org::OrgSettings,
    /// Secrets' names and the kill switch.
    pub vault: vault::Ledger,
    /// Sign-ins, subjects, rights, roles, privacy, preferences and the
    /// home mark: the facts `people::rights` reads.
    pub identity: identity::Identity,
    /// Watched reads' baselines and poll clocks (#369, #406).
    pub watch: watch::Watch,
    /// The conversations Magnus opened, by person then task: each starts
    /// once ever, so deleting one does not start it again (review of #274).
    pub(crate) openers: std::collections::BTreeSet<(String, String)>,
    /// The server's updates and backups.
    pub server_ops: server_ops::ServerOps,
    /// Each person erased with content, by their id then.
    pub erasures: erasure::Erasures,
    /// The apps the owner added, their settings, credentials and registrations.
    pub apps: apps::Apps,
    /// Hidden entries, reactions, disagreements and branches.
    pub thread_extras: thread_extras::ThreadExtras,
    /// Schedules, grants, commitments and answered cards.
    pub arrangements: arrangements::Arrangements,
    /// Runs a person stopped, as against the system.
    pub(crate) stopped_by_person: HashSet<String>,
    /// Drafts written by scoped steps, by draft id: the threads whose
    /// content they may hold and who is entitled to it (#178).
    pub(crate) draft_sources: HashMap<String, DraftSources>,
    /// App tool definitions as screened, those held back, and who connected which app through its connect screen (spec rev 140).
    pub tools: crate::tool_gate::Ledger,
    pub spend: crate::spend::Ledger,
    pub catalog: crate::catalog_withdrawal::Ledger,
    /// Files, projects, knowledge, memory, the playbook and the bundle.
    pub library: library::Library,
    /// Files attached to each task, by task id.
    pub(crate) attachments: HashMap<String, Vec<Value>>,
    pub last_seq: i64,
}

/// A text file's content in lower case for library search, up to as much as
/// a page shows of a file; other files are found by name.
pub fn search_text(media_type: &str, data_b64: &str) -> String {
    use base64::Engine;
    let textual = media_type.starts_with("text/") || ["application/json", "application/xml", "application/csv"].contains(&media_type);
    let Some(bytes) = textual.then(|| base64::engine::general_purpose::STANDARD.decode(data_b64).ok()).flatten() else { return String::new() };
    String::from_utf8_lossy(&bytes).chars().take(crate::compose::PREVIEW_CHARS).collect::<String>().to_lowercase()
}

impl State {
    /// `person`'s current memory, oldest first: in the library (not trashed
    /// or forgotten), not archived, not superseded. What recall searches.
    pub fn current_memory(&self, person: &str) -> Vec<MemoryEntry> {
        self.library.memory().iter().filter(|m| m.person == person && !self.memory_archived.contains(&m.id) && !self.memory_superseded.contains_key(&m.id)).cloned().collect()
    }

    pub fn apply(&mut self, e: &Event) {
        self.last_seq = e.seq;
        if let Some(r) = e.ev.run().and_then(|id| self.runs.get_mut(id)) {
            r.last_seq = e.seq;
            r.last_at = e.at;
        }
        match &e.ev {
            Ev::KernelBoot { .. }
            | Ev::PolicySet { .. }
            | Ev::ModuleSet { .. }
            | Ev::ProviderSet { .. }
            | Ev::DesktopChannelSet { .. }
            | Ev::ConsoleSettings(_)
            | Ev::PolicyDrafted { .. }
            | Ev::AdminAction(_)
            | Ev::ModelSet { .. }
            | Ev::AdapterVerified(_) => self.org.fold(e),
            Ev::SecretSet { .. } | Ev::KillSwitch { .. } | Ev::VaultSealed { .. } | Ev::SecretRevoked { .. } => self.vault.fold(&e.ev, &e.actor, e.at),
            Ev::Computer(c) => self.computers.apply_at(c, e.at),
            Ev::Payment(c) => self.payments.apply(c),
            Ev::Rule(c) => self.rules.apply(c),
            Ev::SubAgent(c) => self.sub_agents.apply(c),
            Ev::Compartment(c) => self.compartments.apply(c),
            Ev::Envelope(c) => self.envelopes.apply(c),
            Ev::Staged(c) => self.staged.apply(c),
            Ev::LocalComputer(c) => self.local.apply(c),
            Ev::LocalFile(c) => self.local.files.apply(c),
            Ev::Desktop(c) => self.local.desktop.apply(c),
            Ev::OutsideAgent(c) => self.outside_agents.apply(e.at, c),
            Ev::Team(c) => {
                if let crate::team::Change::Deleted { workspace } = c {
                    self.missions.drop_workspace(workspace);
                }
                self.team.apply_at(c, e.seq)
            }
            Ev::AccountLink(c) => self.account_links.apply(c),
            Ev::Legal(c) => self.legal.apply(c),
            Ev::Push(c) => self.push.apply(c),
            Ev::Halt(c) => self.halts.apply(c, &e.actor, e.at),
            Ev::AdminSetting(c) => self.org.fold_admin(c),
            Ev::Handback { run, step, image_b64 } => self.add_frame(run, *step, image_b64),
            Ev::BackupMade { .. }
            | Ev::UpdateStarted { .. }
            | Ev::UpdateFinished { .. }
            | Ev::UpdateRolledBack { .. }
            | Ev::UpdateFailed { .. }
            | Ev::BackupChecked { .. }
            | Ev::BackupCopied { .. } => self.server_ops.fold(&e.ev, e.at),
            // Which erasures a backup still holds follows the scrub.
            Ev::BackupScrubPending { .. } | Ev::BackupsScrubbed { .. } => {
                self.server_ops.fold(&e.ev, e.at);
                self.erasures_pending(e);
            }
            Ev::ConnectorAdded { .. }
            | Ev::ConnectorSettings(_)
            | Ev::AppConnected { .. }
            | Ev::AppDisconnected { .. }
            | Ev::CredentialSet { .. }
            | Ev::RegistrationSet { .. }
            | Ev::ToolClassified { .. } => self.apps.fold(&e.ev, e.at),
            // Its tools' definitions and its people's acceptance of its
            // connect screen go with it: another server added later under
            // its name inherits none of them (`connectors::gone`).
            Ev::ConnectorRemoved { name } => {
                self.apps.fold(&e.ev, e.at);
                self.tools.forget_app(name);
            }
            Ev::ContextReset(_)
            | Ev::Escalated { .. }
            | Ev::ModelCall { .. }
            | Ev::Notice { .. }
            | Ev::Intent(_)
            | Ev::Decision(_)
            | Ev::ToolResult { .. }
            | Ev::Contract(_)
            | Ev::WorkStarted { .. }
            | Ev::ContractFrozen { .. }
            | Ev::Plan { .. }
            | Ev::Graded(_)
            | Ev::Verification(_)
            | Ev::Suspended { .. }
            | Ev::Resumed { .. }
            | Ev::Wait { .. }
            | Ev::BudgetExtended { .. } => work::fold_run(&mut self.runs, e),
            Ev::SubjectBound { .. }
            | Ev::RightsFromProvider(_)
            | Ev::DataAccessSet { .. }
            | Ev::PersonDataRead(_)
            | Ev::RolesSet { .. }
            | Ev::SignedIn { .. }
            | Ev::SignedOut { .. }
            | Ev::PasswordChanged { .. }
            | Ev::DevicePaired { .. }
            | Ev::DeviceRevoked { .. }
            | Ev::HomeOpened { .. }
            | Ev::PersonTurnedOff { .. } => self.identity.fold(&e.ev, e.at),
            // Every per-person part forgets them (`forget.rs`).
            Ev::PersonErased { person } => self.forget_person(person),
            Ev::PreferencesSet { person, .. } => {
                self.identity.fold(&e.ev, e.at);
                // Daily times follow the person's zone (owner, 2026-11-14).
                self.arrangements.set_zone(person, self.identity.zone(person));
            }
            Ev::ChatHidden { .. } | Ev::Reacted(_) | Ev::DisagreementRecorded(_) | Ev::DisagreementOutcome(_) | Ev::ThreadBranched(_) => self.thread_extras.fold(&e.ev, e.at),
            Ev::WatchedSeen(crate::events::WatchedSeenPayload { person, app, tool, ids, full }) => self.watch.fold_seen(person, app, tool, ids, *full),
            Ev::WatchPolled { person, app, tool } => self.watch.fold_polled(person, app, tool, e.at),
            Ev::ScheduleSet { .. }
            | Ev::ScheduleRemoved(_)
            | Ev::ScheduleEnded(_)
            | Ev::ScheduleSkipped { .. }
            | Ev::SchedulePaused(_)
            | Ev::ScheduleChanged(_)
            | Ev::ScheduleResumed(_)
            | Ev::ScheduleFailed { .. }
            | Ev::GrantSet(_)
            | Ev::GrantRevoked { .. }
            | Ev::GrantUsed(_)
            | Ev::CommitmentRecorded(_)
            | Ev::CommitmentChanged(_)
            | Ev::CommitmentRemoved { .. }
            | Ev::CardDeclined(_)
            | Ev::CardDeferred { .. }
            | Ev::CardAccepted { .. } => self.arrangements.fold(e, self.identity.preferences()),
            Ev::Trashed { item, person } => {
                if let Some((title, title_from, held)) = self.take(item) {
                    self.trash.put(item.clone(), Trashed { person: person.clone(), title, title_from, at: e.at, held });
                }
            }
            Ev::Restored { item } => {
                if let Some(t) = self.trash.take(item) {
                    self.put_back(t.held);
                }
            }
            Ev::Removed { item, seqs } => {
                // What the removed events had copied leaves with them, and
                // the results they were hold nothing (`read_copies`).
                self.drop_copies(seqs);
                self.drop_read_results(seqs);
                // A person's erasure: their subject is still bound here.
                if let Some(p) = item.strip_prefix("person:") {
                    self.erasures.record(p, e.seq, self.identity.subject(p).map(str::to_string));
                }
                if let Some(task) = item.strip_prefix(crate::team::upkeep::NOTES) {
                    self.team.drop_notes(task);
                }
                if let Some(thread) = self.trash.remove(item) {
                    self.thread_extras.hide(thread, item.clone());
                }
            }
            Ev::FileUploaded(_)
            | Ev::ProjectSet(_)
            | Ev::Moved { .. }
            | Ev::KnowledgeAdded { .. }
            | Ev::KnowledgeShared { .. }
            | Ev::KnowledgeEndorsed { .. }
            | Ev::Memory(_)
            | Ev::MemoryForgotten { .. }
            | Ev::BundleWritten(_)
            | Ev::BundleShared(_)
            | Ev::BundleRemoved { .. } => self.library.fold(e, &self.runs),
            // Each playbook change is a new version, noticed (U5).
            Ev::Playbook(_) => {
                self.library.fold(e, &self.runs);
                self.org.note_version(e, "playbook", format!("v{}", self.library.playbook_version()));
            }
            Ev::ScheduleFired { id, task } => {
                // A run its schedule created was counted when it was created
                // (below), so a firing lost after it never lets one more run.
                let counted = self.tasks.get(task).is_some_and(|t| t.schedule.as_deref() == Some(id.as_str()));
                self.arrangements.fired(id, counted, e.at);
                if let Some(t) = self.tasks.get_mut(task) {
                    t.schedule = Some(id.clone());
                }
            }
            Ev::ToolPinned(crate::events::ToolPinnedPayload { tool, hash, description, input_schema, approved }) => {
                self.tools.pinned(tool, hash, description, input_schema, *approved)
            }
            Ev::ToolChanged(crate::events::ToolChangedPayload { tool, hash, .. }) => self.tools.changed(tool, hash),
            Ev::Tools(c) => self.tools.apply(e.at, c),
            Ev::Evidence(c) => self.evidence.apply(e.at, c),
            Ev::Spend(c) => self.fold_spend(e, c),
            Ev::Catalog(c) => self.catalog.apply(c),
            Ev::TaskCreated { task, person, config, attachments, chat, label, explicit_label, .. } => {
                // New tasks record their full clean starting label explicitly (M1);
                // older events replay with the legacy playbook + commitment fold.
                let pb = if *explicit_label {
                    label.unwrap_or_default()
                } else {
                    let pb = self.playbook_at(person, config.playbook).into_iter().fold(label.unwrap_or_default(), |l, i| l.join(i.label));
                    self.arrangements.commitments().values().filter(|c| &c.person == person && c.status == "open").fold(pb, |l, c| l.join(c.label))
                };
                // A schedule's run counts against it as it is created.
                if let Some(id) = e.actor.strip_prefix("schedule:") {
                    self.arrangements.started(id, e.at);
                }
                if !attachments.is_empty() {
                    self.attachments.insert(task.clone(), attachments.clone());
                }
                if chat.as_ref().is_some_and(|c| c.opener.is_some()) {
                    self.openers.insert((person.clone(), task.clone()));
                }
                work::created(&mut self.tasks, &mut self.runs, e, pb);
            }
            Ev::ModelReply(crate::events::ModelReplyPayload { run, usage, .. }) => {
                self.charge_grant(e, run, usage.cost_usd);
                work::fold_run(&mut self.runs, e);
            }
            Ev::VerifierReply(crate::events::VerifierReplyPayload { run, usage, .. })
            | Ev::ReviewerReply(crate::events::ReviewerReplyPayload { run, usage, .. })
            | Ev::MemoryPass(crate::events::MemoryPassPayload { run, usage, .. })
            | Ev::SpokenWritten(crate::events::SpokenWrittenPayload { run, usage, .. })
            | Ev::KnowledgeExtraction(crate::events::KnowledgeExtractionPayload { run, usage, .. })
            | Ev::DraftScoped(crate::events::DraftScopedPayload { run, usage, .. }) => {
                self.charge_grant(e, run, usage.cost_usd);
                if let Ev::DraftScoped(crate::events::DraftScopedPayload { draft, person, threads, participants, .. }) = &e.ev {
                    self.draft_sources.insert(draft.clone(), DraftSources { person: person.clone(), threads: threads.clone(), participants: participants.clone() });
                }
                work::fold_run(&mut self.runs, e);
            }
            Ev::ModelError(crate::events::ModelErrorPayload { run, usage, .. }) => {
                if let Some(u) = usage {
                    self.charge_grant(e, run, u.cost_usd);
                }
                work::fold_run(&mut self.runs, e);
            }
            Ev::Verdict(_) => work::verdict(&mut self.tasks, &self.runs, e),
            // A run's end frees its review and lets its run's record end once.
            Ev::Completed { run, .. } | Ev::Failed { run, .. } => {
                self.end_cards(run, e.at);
                work::fold_run(&mut self.runs, e);
            }
            Ev::Cancelled { run, .. } => {
                self.end_cards(run, e.at);
                if e.actor.starts_with("person:") && self.runs.get(run).is_some_and(|r| !r.status.terminal()) {
                    self.stopped_by_person.insert(run.clone());
                }
                work::fold_run(&mut self.runs, e);
            }
            Ev::Item { .. } | Ev::ItemOpened { .. } | Ev::Approval(_) | Ev::ApprovalUsed { .. } => self.inbox.fold(e, self.identity.preferences()),
            Ev::ReviewWoken { person, reason, .. } => {
                self.inbox.fold(e, self.identity.preferences());
                if let Some(id) = reason.strip_prefix("commitment ").and_then(|r| r.strip_suffix(" due")) {
                    let by = self.arrangements.commitments().get(id).and_then(|c| c.by.clone());
                    self.inbox.mark_commitment(person, id, by.as_deref());
                }
            }
            // A stopped run's notice put away: the run waits on no card
            // now, so its own Resume (or Cancel) is what is left.
            Ev::ItemResolved { id, .. } => {
                if let Some(r) = self.inbox.resolve(e).and_then(|r| self.runs.get_mut(&r)).filter(|r| r.waiting_item.as_deref() == Some(id.as_str())) {
                    r.waiting_item = None;
                }
            }
            Ev::Output(_) => {
                // The run delivered it: a file in its person's library.
                if let Some(f) = work::output(&mut self.runs, e) {
                    self.library.put_file(f);
                }
            }
            Ev::QueryGateRecorded(p) => self.security.apply_query_gate(p),
            Ev::SafetyBlocked(p) => self.security.apply_safety_blocked(p),
            Ev::TeamMission(ev) => self.missions.apply_at(ev, e.seq),
            Ev::MemorySuperseded(p) => {
                self.memory_superseded.insert(p.old_id.clone(), p.new_id.clone());
            }
            Ev::MemoryArchived(p) => {
                self.memory_archived.insert(p.id.clone());
            }
            Ev::PrefetchRecorded(crate::events::PrefetchRecordedPayload { run, person, label, .. }) => {
                self.prefetches.insert(run.clone(), (person.clone(), e.seq, *label));
            }
        }
        crate::outcomes::observe(self, e);
    }

    /// A person's file, or a delivered file of a task they may see.
    pub fn file_for(&self, person: &str, owner: bool, id: &str) -> Option<&LibFile> {
        self.library.files().get(id).filter(|f| f.person == person || (owner && f.run.is_some()))
    }

    /// The person's playbook items that were live at playbook version `v`.
    pub fn playbook_at(&self, person: &str, v: u64) -> Vec<&PlaybookItem> {
        self.library.playbook_at(person, v)
    }

    /// A run's end (its first: a later one changes nothing): a review it was
    /// stops, and each card it waited on to go on is closed.
    fn end_cards(&mut self, run: &str, at: chrono::DateTime<chrono::Utc>) {
        self.inbox.run_ended(run);
        if let Some(r) = self.runs.get(run).filter(|r| !r.status.terminal()) {
            self.inbox.close_waits_of(run, r.waiting_item.as_deref(), at);
        }
    }

    pub fn open_item_for_run(&self, run: &str, kind: &str) -> Option<&Item> {
        self.inbox.items().values().find(|i| i.run.as_deref() == Some(run) && i.kind == kind && i.resolution.is_none())
    }
}

#[cfg(test)]
mod tests;
