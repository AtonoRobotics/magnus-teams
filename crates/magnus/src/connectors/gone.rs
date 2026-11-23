//! An app gone: one an admin added and removed, one a person built for
//! themselves and deleted, or a catalog app withdrawn. Everything that names
//! it goes with it: every sign-in to it and its key, revoked; every grant
//! that covers it, revoked; every rule made for it, removed. Its tools'
//! definitions and its connect screen's acceptances go when the app itself
//! does (`tool_gate::Ledger::forget_app`), so another server added later
//! under the same name inherits none of it.

use super::admin::{failed, Refusal};
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::Kernel;

impl Kernel {
    /// Remove an app, as `actor`: its sign-ins and key revoked, what names
    /// it taken away, then the app itself. Grants and rules go even when a
    /// sign-in cannot be revoked, since taking a permission away is always
    /// safe; the app stays then, so its token is not lost from view. The
    /// number of sign-ins and keys revoked.
    pub(crate) async fn app_gone(&self, actor: &str, c: &Connector) -> Result<usize, Refusal> {
        let revoked = self.revoke_all(c).await;
        self.take_away_permissions(actor, &c.name).await;
        let n = revoked?;
        self.emit(actor, Ev::ConnectorRemoved { name: c.name.clone() }).await.map_err(failed)?;
        Ok(n)
    }

    /// Every grant that covers `app`, of anyone, revoked, and every rule
    /// made for it removed. Each failure is logged, and the rest go on.
    pub(crate) async fn take_away_permissions(&self, actor: &str, app: &str) {
        let (grants, rules) = self.read(|s| {
            let grants: Vec<(String, String)> =
                s.arrangements.grants().values().filter(|g| g.revoked.is_none() && g.apps.iter().any(|a| a == app)).map(|g| (g.id.clone(), g.person.clone())).collect();
            let rules: Vec<String> = s.rules.made_for(app);
            (grants, rules)
        });
        for (id, person) in grants {
            if let Err(e) = self.emit(actor, Ev::GrantRevoked { id: id.clone(), person }).await {
                tracing::error!("{app} is gone: grant {id} was not revoked: {e:#}");
            }
        }
        for id in rules {
            if let Err(e) = self.emit(actor, Ev::Rule(crate::rules::Change::Removed { id: id.clone() })).await {
                tracing::error!("{app} is gone: rule {id} was not removed: {e:#}");
            }
        }
    }
}

#[cfg(test)]
#[path = "gone_tests.rs"]
mod tests;
