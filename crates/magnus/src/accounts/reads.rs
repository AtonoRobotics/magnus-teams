//! Rows the accounts store reads inside a transaction, for the rules
//! (`rules`, the last-admin guard) to judge before it writes.

use super::{Accounts, Status};
use anyhow::Result;

impl Accounts {
    /// Whether `person` is an enabled admin with no other: demoting or
    /// disabling them would strand the organisation. Unrecorded people are
    /// members, so an unmanaged switch never guards.
    pub(super) async fn sole_enabled_admin_tx(tx: &tokio_postgres::Transaction<'_>, person: &str) -> Result<bool> {
        let role: Option<String> = tx.query_opt("SELECT role FROM public.account_roles WHERE person = $1", &[&person]).await?.map(|r| r.get(0));
        if role.as_deref() != Some("admin") {
            return Ok(false);
        }
        if Self::status_tx(tx, person).await?.is_some_and(|s| s != Status::Active) {
            return Ok(false);
        }
        let others: i64 = tx
            .query_one(
                "SELECT COUNT(*) FROM public.account_roles r LEFT JOIN public.account_statuses s ON s.person = r.person
                 WHERE r.role = 'admin' AND (s.status IS NULL OR s.status = 'active') AND r.person <> $1",
                &[&person],
            )
            .await?
            .get(0);
        Ok(others == 0)
    }

    /// A person's recorded status, inside a transaction; None when none is
    /// recorded.
    pub(super) async fn status_tx(tx: &tokio_postgres::Transaction<'_>, person: &str) -> Result<Option<Status>> {
        Ok(tx.query_opt("SELECT status FROM public.account_statuses WHERE person = $1", &[&person]).await?.map(|r| Status::parse(r.get::<_, String>(0).as_str())))
    }
}
