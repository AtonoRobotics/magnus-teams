//! Erasing a person from the state. The `PersonErased` arm of
//! `State::apply` calls `State::forget_person` and nothing else.
//!
//! `forget_person` takes the state apart field by field, with no `..`, so
//! a field added to the state does not compile until it is listed here:
//! either forgotten (a ledger's own `Forget`, or a line here) or kept, with
//! the reason. Each feature ledger owns what erasing means for it, beside
//! the records it keeps (decisions/2026-11-13-every-ledger-forgets.md).
//!
//! What stays, and why: the audit and the erasures themselves (the record
//! that it happened, #195); approvals, kill switches and spend decisions
//! the person gave others (each was valid when given and is about the
//! other person's work, #191); the organisation's own config, policies,
//! apps and knowledge. Their computer and its volume are deleted by the
//! erasure itself, before this runs (`person_admin::erase_person`), and
//! their team messages deleted for everyone (`team/forget.rs`).

use crate::state::State;
use std::collections::HashSet;

/// Who is forgotten, with what the state held of theirs just before: the
/// ledgers keyed by run, task, item or thread forget those.
#[derive(Debug, Default)]
pub struct Erased {
    pub person: String,
    pub runs: HashSet<String>,
    pub tasks: HashSet<String>,
    pub items: HashSet<String>,
    /// Their conversations: their tasks' threads and their branches.
    pub threads: HashSet<String>,
    pub files: HashSet<String>,
}

impl Erased {
    pub fn is(&self, p: &str) -> bool {
        p == self.person
    }

    /// Their own agent, as kill switches and pauses name it.
    pub fn agent(&self) -> String {
        crate::halts::agent_of_person(&self.person)
    }

    fn of(s: &State, person: &str) -> Erased {
        let tasks: HashSet<String> = s.tasks.values().filter(|t| t.person == person).map(|t| t.task_id.clone()).collect();
        let mut threads: HashSet<String> = s.tasks.values().filter(|t| t.person == person).filter_map(|t| t.chat.as_ref().map(|c| c.thread.clone())).collect();
        threads.extend(s.thread_extras.branches().iter().filter(|(_, b)| b.person == person).map(|(t, _)| t.clone()));
        threads.extend(tasks.iter().cloned());
        Erased {
            person: person.to_string(),
            runs: s.runs.values().filter(|r| r.person == person).map(|r| r.id.clone()).collect(),
            items: s.inbox.items().values().filter(|i| i.person == person).map(|i| i.id.clone()).collect(),
            files: s.library.files().iter().filter(|(_, f)| f.person == person).map(|(id, _)| id.clone()).collect(),
            tasks,
            threads,
        }
    }
}

/// A part of the state that may hold something of a person's. A part
/// that holds nothing personal says so in its `forget`.
pub trait Forget {
    fn forget(&mut self, who: &Erased);
}

/// Their own sign-in to one of the organisation's apps:
/// `connector_<app>__<person>`, where an app's name holds no `__` (it may
/// end in `_`), so not anyone's whose name ends in theirs.
pub fn their_sign_in(name: &str, person: &str) -> bool {
    let tail = format!("__{person}");
    name.strip_prefix("connector_").and_then(|r| r.strip_suffix(&tail)).is_some_and(|app| !app.is_empty() && !app.contains("__"))
}

/// A vault secret of theirs alone: a sign-in, their card or Link wallet.
pub fn their_secret(name: &str, person: &str) -> bool {
    their_sign_in(name, person) || name == format!("payments/card/{person}") || name == format!("payments/link/{person}")
}

impl State {
    /// Everything of the person's leaves the state, so it holds live as it
    /// does after a replay.
    pub(crate) fn forget_person(&mut self, person: &str) {
        let who = Erased::of(self, person);
        let p = person;
        let State {
            // Feature ledgers: each owns what erasing means for it.
            computers,
            payments,
            rules,
            evidence,
            sub_agents,
            compartments,
            envelopes,
            staged,
            security,
            prefetches,
            memory_superseded,
            memory_archived,
            outcomes,
            local,
            team,
            account_links,
            legal,
            push,
            halts,
            watch,
            server_ops,
            vault,
            org,
            thread_extras,
            apps,
            arrangements,
            library,
            inbox,
            identity,
            trash,
            erasures,
            tools,
            spend,
            catalog,
            // The person's own records.
            tasks,
            runs,
            openers,
            stopped_by_person,
            draft_sources,
            attachments,
            // Kept: the record of what was done and of the erasure itself
            // (#195), the organisation's own settings, apps, policies and
            // knowledge, and the server's own condition. None of it is the
            // person's: a policy layer naming them is the admins' rule, and
            // their apps' records are harmless once the id is never given
            // again (2026-11-11-an-erased-persons-id-is-not-given-again.md).
            last_seq: _,
        } = self;
        let parts: [&mut dyn Forget; 31] = [
            computers,
            payments,
            rules,
            evidence,
            sub_agents,
            compartments,
            envelopes,
            staged,
            security,
            outcomes,
            local,
            team,
            account_links,
            legal,
            push,
            halts,
            watch,
            server_ops,
            vault,
            org,
            thread_extras,
            apps,
            arrangements,
            library,
            inbox,
            identity,
            trash,
            erasures,
            tools,
            spend,
            catalog,
        ];
        for part in parts {
            part.forget(&who);
        }
        tasks.retain(|_, t| t.person != p);
        runs.retain(|_, r| r.person != p);
        stopped_by_person.retain(|r| !who.runs.contains(r));
        // What their drafts were written from.
        draft_sources.retain(|_, d| d.person != p);
        // What their tasks had attached goes too.
        attachments.retain(|t, _| !who.tasks.contains(t));
        openers.retain(|(x, _)| x != p);
        prefetches.retain(|_, (owner, _, _)| owner != p);
        let live_mem: HashSet<&str> = library.memory().iter().map(|m| m.id.as_str()).collect();
        memory_superseded.retain(|old, _| live_mem.contains(old.as_str()));
        memory_archived.retain(|id| live_mem.contains(id.as_str()));
    }
}

#[cfg(test)]
#[path = "forget_tests.rs"]
mod tests;
