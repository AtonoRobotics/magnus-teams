//! One rule for changing someone's account: their role, whether they are
//! on, their recovery address, their invitation, their removal (decision
//! `2026-11-23-one-rule-for-changing-an-account.md`). Every operation that
//! changes an account asks it itself, so no route can skip it.

use super::{rights, Authority};
use crate::kernel::Kernel;
use axum::http::StatusCode;

/// A change refused: its status, its error code and what it says.
pub type Refusal = (StatusCode, &'static str, String);

/// The person asking, read from the actor the log names: `admin:<id>` from
/// the Admin section, `person:<id>` from their own session or a flow they
/// began. Any other actor is the server itself: the console on the machine,
/// which only the server's own user reaches, or the server's own upkeep.
pub fn asker(actor: &str) -> Option<&str> {
    actor.strip_prefix("admin:").or_else(|| actor.strip_prefix("person:"))
}

fn owner_only() -> Refusal {
    (StatusCode::FORBIDDEN, "owner_only", "Only an owner changes an owner's account.".into())
}

fn not_admin() -> Refusal {
    (StatusCode::FORBIDDEN, "forbidden", "Only an admin changes someone else's account.".into())
}

impl Kernel {
    /// Whether `actor` may change `target`'s account. The server may change
    /// anyone's, a person their own, an owner anyone's, an admin anyone's
    /// but an owner's, and no one else anyone's.
    pub async fn may_change_account(&self, actor: &str, target: &str) -> Result<(), Refusal> {
        let Some(id) = asker(actor) else { return Ok(()) };
        if id == target {
            return Ok(());
        }
        let authority = self.asker_authority(id).await;
        if !authority.administers() {
            return Err(not_admin());
        }
        if authority != Authority::Owner && self.person(target).is_some_and(|t| rights(&t).is_server_owner()) {
            return Err(owner_only());
        }
        Ok(())
    }

    /// Whether `actor` may give `target` the role `role` of the four: the
    /// account rule, and then only an admin gives a role, their own
    /// included, and only an owner gives the owner role.
    pub async fn may_give_role(&self, actor: &str, target: &str, role: &str) -> Result<(), Refusal> {
        self.may_change_account(actor, target).await?;
        let Some(id) = asker(actor) else { return Ok(()) };
        let authority = self.asker_authority(id).await;
        if !authority.administers() {
            return Err(not_admin());
        }
        if role == "owner" && authority != Authority::Owner {
            return Err((StatusCode::FORBIDDEN, "owner_only", "Only an owner gives or takes the owner role.".into()));
        }
        Ok(())
    }

    /// The asker's authority; someone who is not a person here has none.
    async fn asker_authority(&self, id: &str) -> Authority {
        match self.person(id) {
            Some(p) => self.authority(&p).await,
            None => Authority::Guest,
        }
    }
}
