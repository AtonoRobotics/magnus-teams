//! Removing a person, as one kernel operation: the Admin section's removal,
//! the console's, and a person deleting their own account (decision
//! `2026-11-23-one-reader-for-server-roles.md`). Everything that could
//! refuse it is asked first, under the account lock, so a refused removal
//! changes nothing; then their team work is passed on or deleted, their
//! data erased and their account removed, or, for someone the config names,
//! turned off.

use crate::account_change::{not_found, store_refusal};
use crate::accounts::{Guard, Status};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::people::{After, Refusal};
use serde_json::{json, Value};

/// What becomes of the team work of someone removed: the projects and
/// routines they hold for workspaces, and the workspaces they own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamWork {
    /// Left as it is: the console's removal, which never said.
    Untouched,
    /// Passed to this person.
    Transfer(String),
    /// Deleted; a workspace with others in it passes to one of them.
    Delete,
}

impl Kernel {
    /// Remove `id`, as `actor`: refused, with nothing changed, when the
    /// account rule refuses or it would leave no enabled owner or admin.
    /// What the erasure did, and whether a local account was removed.
    pub async fn remove_account(&self, actor: &str, id: &str, team: TeamWork) -> Result<Value, Refusal> {
        if self.person(id).is_none() {
            return Err(not_found());
        }
        let _one = self.account_changes.lock().await;
        self.would_strand(id, After::Gone).await?;
        self.may_change_account(actor, id).await?;
        if team != TeamWork::Untouched {
            self.pass_on_team_work(actor, id, &team).await?;
        }
        // Turned off first, so a failed erasure leaves a listed, disabled
        // account whose removal retries cleanly (#529).
        let local = self.accounts.is_local(id);
        let ended = self.accounts.set_status_as(id, Status::Disabled, Guard::Kernel).await.map_err(store_refusal)?;
        if !ended.is_empty() {
            let evs: Vec<(&str, Ev)> = ended.iter().map(|session| (actor, Ev::SignedOut { person: id.to_string(), session: session.clone() })).collect();
            self.emit_all(evs).await.map_err(store_refusal)?;
        }
        let mut erased = self.erase_person(actor, id).await.map_err(|(code, m)| (code, "failed", m))?;
        erased["sessions_ended"] = json!(ended.len() as u64 + erased["sessions_ended"].as_u64().unwrap_or(0));
        if local {
            self.remove_local_person(id).await.map_err(store_refusal)?;
        } else {
            // Someone the config names stays listed, turned off.
            self.turn_off(id, &format!("{actor} removed {id}"), "Your account here was removed").await;
        }
        Ok(json!({ "erased": erased, "account_removed": local }))
    }

    /// Delete the workspaces `id` alone is in, then pass on or delete what
    /// they hold for the rest, and take them out of every workspace.
    async fn pass_on_team_work(&self, actor: &str, id: &str, team: &TeamWork) -> Result<(), Refusal> {
        let sole = self.read(|s| crate::admin_app::remove::sole_workspaces(&s.team, id));
        for ws in &sole {
            self.delete_workspace(actor, ws).await.map_err(store_refusal)?;
        }
        let given = match team {
            TeamWork::Transfer(to) => Some(to.as_str()),
            _ => None,
        };
        let evs = self.read(|s| {
            let h = crate::admin_app::remove::held(&s.team, id);
            crate::admin_app::remove::changes(&s.team, id, &h, given)
        });
        let evs: Vec<(&str, Ev)> = evs.into_iter().map(|c| (actor, Ev::Team(c))).collect();
        if !evs.is_empty() {
            self.emit_all(evs).await.map_err(store_refusal)?;
        }
        Ok(())
    }
}
