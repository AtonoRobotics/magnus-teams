//! Each change to someone's account, as one kernel operation that asks the
//! account rule first (`Kernel::may_change_account`, decision
//! `2026-11-23-one-rule-for-changing-an-account.md`): the Admin section and
//! the console call these, never the account store directly, so no route
//! can change an account the rule would refuse.

use crate::accounts::{Guard, Role, Status};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::people::{After, Refusal};
use axum::http::StatusCode;

/// A failure of the account store, as a refusal: the last admin's own, or
/// the store's.
pub(crate) fn store_refusal(e: anyhow::Error) -> Refusal {
    let m = format!("{e:#}");
    if m.contains("last admin") {
        return (StatusCode::CONFLICT, "last_admin", "This is the organisation's last admin: make someone else an admin first.".into());
    }
    (StatusCode::INTERNAL_SERVER_ERROR, "failed", m)
}

pub(crate) fn not_found() -> Refusal {
    (StatusCode::NOT_FOUND, "not_found", "No such person.".into())
}

fn not_invited() -> Refusal {
    (StatusCode::CONFLICT, "not_invited", "They have already chosen their password: this is an account, not an invitation.".into())
}

/// What turning someone off ended: their sessions and their schedules.
#[derive(Debug, Default, PartialEq)]
pub struct TurnedOff {
    pub sessions: usize,
    pub schedules: usize,
}

impl Kernel {
    fn known(&self, id: &str) -> Result<(), Refusal> {
        self.person(id).map(|_| ()).ok_or_else(not_found)
    }

    /// Set, or with None clear, the address `id`'s recovery emails go to.
    pub async fn change_account_email(&self, actor: &str, id: &str, email: Option<&str>) -> Result<(), Refusal> {
        self.known(id)?;
        if email.is_some_and(|e| !crate::invitations::valid_email(&e.trim().to_lowercase())) {
            return Err((StatusCode::BAD_REQUEST, "invalid", "That is not an email address.".into()));
        }
        self.may_change_account(actor, id).await?;
        let name = self.person(id).map(|p| p.name).unwrap_or_default();
        self.accounts.set_email(id, &name, email).await.map_err(store_refusal)
    }

    /// Make `id` an admin or a member in the server's own directory: never
    /// the last enabled owner's or admin's role taken.
    pub async fn set_directory_role(&self, actor: &str, id: &str, role: Role) -> Result<(), Refusal> {
        let Some(p) = self.person(id) else { return Err(not_found()) };
        let _one = self.account_changes.lock().await;
        self.may_give_role(actor, id, role.as_str()).await?;
        self.would_strand(id, After::Role(self.authority_with(&p, role))).await?;
        self.accounts.set_role_as(id, role, Guard::Kernel).await.map_err(store_refusal)
    }

    /// Set `id`'s roles here to `roles`, as `actor` (the owner's Settings):
    /// never while the identity provider names them, and never the last
    /// enabled owner's or admin's role taken.
    pub async fn set_roles_of(&self, actor: &str, id: &str, roles: Vec<String>) -> Result<(), Refusal> {
        let Some(mut p) = self.person(id) else { return Err(not_found()) };
        let _one = self.account_changes.lock().await;
        self.may_change_account(actor, id).await?;
        if self.read(|s| s.identity.provider_set_roles(id)) {
            return Err((StatusCode::CONFLICT, "provider", "The identity provider names this person's roles: change them there.".into()));
        }
        p.roles = roles.clone();
        let then = self.authority_with(&p, self.accounts.role(id).await);
        self.would_strand(id, After::Role(then)).await?;
        self.emit(actor, Ev::RolesSet { person: id.to_string(), roles }).await.map(|_| ()).map_err(store_refusal)
    }

    /// Turn `id` off, as `actor`: never the last enabled owner or admin.
    /// Their sessions end on the record, and everything turning off does
    /// follows (`Kernel::turn_off`).
    pub async fn turn_off_account(&self, actor: &str, id: &str) -> Result<TurnedOff, Refusal> {
        self.known(id)?;
        let one = self.account_changes.lock().await;
        self.would_strand(id, After::Off).await?;
        self.may_change_account(actor, id).await?;
        let schedules = self.read(|s| s.arrangements.schedules().values().filter(|x| x.person == id && x.next_due().is_some()).count());
        let ended = self.accounts.set_status_as(id, Status::Disabled, Guard::Kernel).await.map_err(store_refusal)?;
        drop(one);
        let sessions = ended.len();
        let evs: Vec<(&str, Ev)> = ended.into_iter().map(|session| (actor, Ev::SignedOut { person: id.to_string(), session })).collect();
        if !evs.is_empty() {
            self.emit_all(evs).await.map_err(store_refusal)?;
        }
        self.turn_off(id, &format!("{actor} turned {id} off"), "An administrator has turned your account off").await;
        Ok(TurnedOff { sessions, schedules })
    }

    /// Turn `id` back on, as `actor`.
    pub async fn turn_on_account(&self, actor: &str, id: &str) -> Result<(), Refusal> {
        self.known(id)?;
        self.may_change_account(actor, id).await?;
        self.accounts.set_status(id, Status::Active).await.map(|_| ()).map_err(store_refusal)
    }

    /// The invitation `id` still waits on, checked as `actor` may change it.
    async fn invitation(&self, actor: &str, id: &str) -> Result<(), Refusal> {
        self.known(id)?;
        self.may_change_account(actor, id).await?;
        if self.accounts.status(id).await != Status::Invited {
            return Err(not_invited());
        }
        Ok(())
    }

    /// Withdraw an invitation sent from here: the local account goes,
    /// never having held anything.
    pub async fn withdraw_invitation(&self, actor: &str, id: &str) -> Result<(), Refusal> {
        self.invitation(actor, id).await?;
        if !self.accounts.is_local(id) {
            return Err((StatusCode::CONFLICT, "not_invited", "Only an invitation sent from here can be withdrawn.".into()));
        }
        self.remove_local_person(id).await.map(|_| ()).map_err(store_refusal)
    }

    /// A fresh invitation token for `id`; the old one dies with it.
    pub async fn renew_invitation(&self, actor: &str, id: &str) -> Result<String, Refusal> {
        self.invitation(actor, id).await?;
        self.accounts.invite(id, None).await.map_err(store_refusal)
    }

    /// Give an invited person a password to choose with once.
    pub async fn set_invitation_password(&self, actor: &str, id: &str, password: &str) -> Result<(), Refusal> {
        self.invitation(actor, id).await?;
        self.accounts.set_temporary_password(id, password).await.map_err(|e| {
            let message = format!("{e:#}");
            if message.contains("not an invitation") {
                return not_invited();
            }
            (StatusCode::BAD_REQUEST, "invalid", message)
        })
    }
}
