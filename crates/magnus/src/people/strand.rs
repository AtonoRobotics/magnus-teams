//! One "would this leave the server with no enabled owner or admin", read
//! from every place a role is kept (decision
//! `2026-11-23-one-reader-for-server-roles.md`). A change to a role, a
//! status or a removal asks it under the kernel's account lock
//! (`Kernel::account_changes`) before anything changes.

use super::{Authority, Refusal};
use crate::accounts::Status;
use crate::kernel::Kernel;
use axum::http::StatusCode;

/// What a person would be after a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Holding this role of the four.
    Role(Authority),
    /// Turned off.
    Off,
    /// Removed.
    Gone,
}

impl Kernel {
    /// Refused when `person` becoming `after` would leave no enabled owner,
    /// or no enabled owner or admin, where there was one. Enabled is an
    /// account that is on and no longer only invited. The caller holds
    /// `account_changes` from this check until its change is made.
    pub async fn would_strand(&self, person: &str, after: After) -> Result<(), Refusal> {
        let (mut owners, mut owners_after, mut admins, mut admins_after) = (0, 0, 0, 0);
        for p in self.people().all() {
            if self.accounts.status(&p.id).await != Status::Active {
                continue;
            }
            let now = self.authority(&p).await;
            let then = match after {
                _ if p.id != person => Some(now),
                After::Role(a) => Some(a),
                After::Off | After::Gone => None,
            };
            owners += usize::from(now == Authority::Owner);
            owners_after += usize::from(then == Some(Authority::Owner));
            admins += usize::from(now.administers());
            admins_after += usize::from(then.is_some_and(Authority::administers));
        }
        if owners > 0 && owners_after == 0 {
            return Err((StatusCode::CONFLICT, "last_owner", "This is the last owner: make someone else an owner first.".into()));
        }
        if admins > 0 && admins_after == 0 {
            return Err((StatusCode::CONFLICT, "last_admin", "This is the organisation's last admin: make someone else an admin first.".into()));
        }
        Ok(())
    }
}
