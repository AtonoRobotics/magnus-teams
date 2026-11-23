//! The lockout counters kept in the store, next to the sign-in rules they
//! follow (`account_sign_in_policy`), so a restart or a deploy forgives no
//! lockout and no guessing in progress (decision
//! `2026-11-23-credential-changes-end-other-sessions.md`). The running
//! server counts in memory, at once, so guesses sent together cannot all
//! pass; each change is then written here.

use super::lockout::{counters, Failures};
use super::Accounts;
use anyhow::{Context, Result};
use tokio_postgres::Client;

impl Accounts {
    /// Every counter the store keeps.
    pub(super) async fn read_counters(c: &Client) -> Result<Vec<(String, Failures)>> {
        let rows = c.query("SELECT counter, failures, first, until FROM public.account_lockouts", &[]).await.context("read the lockout counters")?;
        Ok(rows.into_iter().map(|r| (r.get(0), Failures { n: u32::try_from(r.get::<_, i64>(1)).unwrap_or(u32::MAX), first: r.get(2), until: r.get(3) })).collect())
    }

    /// Write every counter changed since the last write. A write that fails
    /// is logged and tried again with the next.
    pub(super) async fn keep_counters(&self) {
        let changed = counters(&self.failures).take_changed();
        if changed.is_empty() {
            return;
        }
        if let Err(e) = self.write_counters(&changed).await {
            tracing::warn!("the lockout counters were not stored: {e:#}");
            counters(&self.failures).mark(changed.into_iter().map(|(c, _)| c));
        }
    }

    async fn write_counters(&self, changed: &[(String, Option<Failures>)]) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        for (counter, f) in changed {
            match f {
                Some(f) => {
                    c.execute(
                        "INSERT INTO public.account_lockouts (counter, failures, first, until) VALUES ($1, $2, $3, $4)
                         ON CONFLICT (counter) DO UPDATE SET failures = EXCLUDED.failures, first = EXCLUDED.first, until = EXCLUDED.until",
                        &[counter, &i64::from(f.n), &f.first, &f.until],
                    )
                    .await?
                }
                None => c.execute("DELETE FROM public.account_lockouts WHERE counter = $1", &[counter]).await?,
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cfg, db};
    use super::super::{Accounts, Status};

    /// The counters lived in memory, so a deploy forgave a lockout and every
    /// guess in progress. They are kept beside the rules now: a lock and a
    /// count both outlive a restart, and turning someone on still clears them.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_lockout_and_guessing_in_progress_outlive_a_restart() {
        let (_db, url) = db().await;
        let accounts = Accounts::open(&url, cfg()).await.unwrap();
        let mut p = accounts.policy().await.unwrap();
        p.lockout_failures = 2;
        p.lockout_wait_minutes = 5;
        p.lockout_max_wait_minutes = 6;
        accounts.set_policy(p).await.unwrap();
        for id in ["ada", "bo"] {
            accounts.set_password(id, "a-long-password").await.unwrap();
        }
        let (ada, bo) = (Accounts::network_counter("ada"), Accounts::network_counter("bo"));
        for _ in 0..2 {
            let _ = accounts.sign_in_counted("ada", "wrong-password", &ada).await;
        }
        let _ = accounts.sign_in_counted("bo", "wrong-password", &bo).await;
        let held = accounts.locked(&ada).expect("two failures did not lock");
        assert!(accounts.locked(&bo).is_none());
        drop(accounts);

        let accounts = Accounts::open(&url, cfg()).await.unwrap();
        let after = accounts.locked(&ada).expect("a restart forgave the lockout");
        assert!((after - held).num_milliseconds().abs() < 1, "the lock moved across the restart: {held} to {after}");
        let _ = accounts.sign_in_counted("bo", "wrong-password", &bo).await;
        assert!(accounts.locked(&bo).is_some(), "a restart forgave the guess in progress");
        accounts.set_status("ada", Status::Active).await.unwrap();
        drop(accounts);

        let accounts = Accounts::open(&url, cfg()).await.unwrap();
        assert!(accounts.locked(&ada).is_none(), "turning ada on did not clear the stored lock");
        assert!(accounts.locked(&bo).is_some());
    }
}
