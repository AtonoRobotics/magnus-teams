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
        threads.extend(s.branches.iter().filter(|(_, b)| b.person == person).map(|(t, _)| t.clone()));
        threads.extend(tasks.iter().cloned());
        Erased {
            person: person.to_string(),
            runs: s.runs.values().filter(|r| r.person == person).map(|r| r.id.clone()).collect(),
            items: s.items.values().filter(|i| i.person == person).map(|i| i.id.clone()).collect(),
            files: s.files.iter().filter(|(_, f)| f.person == person).map(|(id, _)| id.clone()).collect(),
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
            outcomes,
            local,
            team,
            account_links,
            legal,
            push,
            halts,
            tools,
            spend,
            catalog,
            // The person's own records.
            trash,
            removed_threads,
            tasks,
            runs,
            items,
            approvals,
            signed_in_before,
            last_sign_in,
            home_last_open,
            watched_seen,
            watch_last_polled,
            items_opened,
            items_resolved_at,
            app_connections,
            openers,
            turned_off,
            provider_rights,
            subjects,
            roles_set,
            preferences,
            hidden,
            reactions,
            disagreements,
            branches,
            schedules,
            grants,
            commitments,
            cards,
            stopped_by_person,
            draft_sources,
            reviews_last,
            reviews_overnight,
            interrupts,
            review_runs,
            wake_marks,
            secrets,
            revoked,
            memory,
            attachments,
            files,
            projects,
            moved,
            bundle,
            // Kept: the record of what was done and of the erasure itself
            // (#195), the organisation's own settings, apps, policies and
            // knowledge, and the server's own condition. None of it is the
            // person's: a policy layer naming them is the admins' rule, and
            // their apps' records are harmless once the id is never given
            // again (2026-11-11-an-erased-persons-id-is-not-given-again.md).
            audit: _,
            erasures: _,
            kill_switch: _,
            kill_switch_by: _,
            policies: _,
            policy_history: _,
            policy_drafts: _,
            last_backup: _,
            last_copy: _,
            scrub_pending: _,
            updating: _,
            last_update: _,
            skipped_releases: _,
            connectors: _,
            registrations: _,
            connector_settings: _,
            credential_changed: _,
            sealed: _,
            data_private: _,
            model: _,
            admin_settings: _,
            knowledge: _,
            conformance: _,
            playbook: _,
            // A path counts its versions on through any removal.
            bundle_last: _,
            module_settings: _,
            providers_off: _,
            desktop_channel: _,
            console_settings: _,
            version_changes: _,
            playbook_version: _,
            last_seq: _,
        } = self;
        let parts: [&mut dyn Forget; 15] = [computers, payments, rules, evidence, sub_agents, outcomes, local, team, account_links, legal, push, halts, tools, spend, catalog];
        for part in parts {
            part.forget(&who);
        }
        tasks.retain(|_, t| t.person != p);
        runs.retain(|_, r| r.person != p);
        items.retain(|_, i| i.person != p && i.run.as_ref().is_none_or(|r| !who.runs.contains(r)));
        // Their items' opened and resolved marks go with the items (#412).
        items_opened.retain(|id, _| !who.items.contains(id));
        items_resolved_at.retain(|id, _| !who.items.contains(id));
        stopped_by_person.retain(|r| !who.runs.contains(r));
        schedules.retain(|_, x| x.person != p);
        // Their grants: none covers whoever is next known by their name.
        grants.retain(|_, g| g.person != p);
        // Their commitments, whose words went with theirs (#236).
        commitments.retain(|_, c| c.person != p);
        memory.retain(|m| m.person != p);
        // Their own documents (their relationship memory among them) and
        // their versions, whose content went with theirs (org-context.md,
        // the owner's ruling on erasure).
        bundle.retain(|_, d| d.owner_user.as_deref() != Some(p));
        turned_off.remove(p);
        files.retain(|_, f| f.person != p);
        projects.retain(|_, x| x.person != p);
        trash.retain(|_, t| t.person != p);
        // What they moved, hid and removed of their own conversations.
        moved.retain(|id, _| !who.tasks.contains(id) && !who.files.contains(id) && !who.threads.contains(id));
        removed_threads.retain(|t| !who.threads.contains(t));
        hidden.remove(p);
        hidden.retain(|t, _| !who.threads.contains(t));
        preferences.remove(p);
        // Their reactions and disagreements, whose words went with theirs.
        reactions.remove(p);
        disagreements.retain(|_, d| d.person != p);
        // Their answers to cards, a decline's reason among them.
        cards.retain(|_, c| c.person.as_deref() != Some(p));
        // What their drafts were written from.
        draft_sources.retain(|_, d| d.person != p);
        // The proactive loop's scheduling for them.
        reviews_last.remove(p);
        reviews_overnight.remove(p);
        interrupts.remove(p);
        review_runs.retain(|_, x| x != p);
        wake_marks.retain(|k, _| k.split_once(':').is_some_and(|(x, _)| x != p));
        // Their branched threads (the entries as shown then) and what their
        // tasks had attached go too.
        branches.retain(|_, b| b.person != p);
        attachments.retain(|t, _| !who.tasks.contains(t));
        subjects.remove(p);
        last_sign_in.remove(p);
        signed_in_before.remove(p);
        app_connections.retain(|(x, _), _| x != p);
        openers.retain(|(x, _)| x != p);
        // Their watched baselines and home mark: whoever is next known by
        // their name starts fresh (#412). So do their poll clocks (#406).
        watched_seen.retain(|(x, _, _), _| x != p);
        home_last_open.remove(p);
        watch_last_polled.retain(|(x, _, _), _| x != p);
        // Their secrets' names, which the erasure took from the vault.
        secrets.retain(|n| !their_secret(n, p));
        revoked.retain(|n, _| !their_secret(n, p));
        // The rights they held go with them: whoever is next known by their
        // name starts from the organisation's config.
        roles_set.remove(p);
        provider_rights.remove(p);
        // So do the approvals given for their actions: none covers whoever
        // is next known by their name (#191). Those they gave others stay,
        // as given; the gate still asks whether their approver may give a
        // lasting one.
        approvals.retain(|a| a.person != p);
    }
}

#[cfg(test)]
#[path = "forget_tests.rs"]
mod tests;
