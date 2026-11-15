//! What the sign-in check reads and writes, and nothing more: a person's
//! password hash, status and authenticator secret, the step their last
//! code used, and the owner's one-time machine codes. `Accounts` is the
//! Postgres adapter; the tests have a table in memory, so the check's
//! rules run without a database.

use super::{Accounts, Status};
use anyhow::{Context, Result};

/// The sign-in check's store. Person ids arrive canonical.
#[async_trait::async_trait]
pub(super) trait Credentials: Send + Sync {
    /// The person's password hash, and whether it is temporary.
    async fn password(&self, person: &str) -> Result<Option<(String, bool)>>;
    /// The person's recorded status; None when none is recorded.
    async fn status(&self, person: &str) -> Result<Option<Status>>;
    /// The person's authenticator secret, when they enrolled one.
    async fn totp_secret(&self, person: &str) -> Result<Option<String>>;
    /// Record `step` as the person's last used code step, only if it is
    /// later than the one recorded: true when it was. One atomic step,
    /// so concurrent uses of one code admit exactly one (#441).
    async fn advance_totp_step(&self, person: &str, step: i64) -> Result<bool>;
    /// Take a one-time machine code: true once, if it is current.
    async fn take_machine_code(&self, code: &str) -> Result<bool>;
}

#[async_trait::async_trait]
impl Credentials for Accounts {
    async fn password(&self, person: &str) -> Result<Option<(String, bool)>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        Ok(c.query_opt("SELECT phc, temporary FROM public.account_passwords WHERE person = $1", &[&person]).await?.map(|r| (r.get(0), r.get(1))))
    }

    async fn status(&self, person: &str) -> Result<Option<Status>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        Ok(c.query_opt("SELECT status FROM public.account_statuses WHERE person = $1", &[&person]).await?.map(|r| Status::parse(r.get::<_, String>(0).as_str())))
    }

    async fn totp_secret(&self, person: &str) -> Result<Option<String>> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        Ok(c.query_opt("SELECT secret FROM public.account_totp WHERE person = $1", &[&person]).await?.map(|r| r.get(0)))
    }

    async fn advance_totp_step(&self, person: &str, step: i64) -> Result<bool> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        // Compare-and-set in one statement: the row moves only forward.
        let n = c
            .execute(
                "INSERT INTO public.account_totp_steps (person, step) VALUES ($1, $2)
                 ON CONFLICT (person) DO UPDATE SET step = EXCLUDED.step WHERE account_totp_steps.step < EXCLUDED.step",
                &[&person, &step],
            )
            .await?;
        Ok(n == 1)
    }

    async fn take_machine_code(&self, code: &str) -> Result<bool> {
        Accounts::take_machine_code(self, code).await
    }
}

/// The store as tables in memory, for the check's tests.
#[cfg(test)]
pub(super) mod table {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

    #[derive(Default)]
    pub(in crate::accounts) struct Table {
        pub(in crate::accounts) passwords: Mutex<BTreeMap<String, (String, bool)>>,
        pub(in crate::accounts) statuses: Mutex<BTreeMap<String, Status>>,
        pub(in crate::accounts) totp: Mutex<BTreeMap<String, String>>,
        pub(in crate::accounts) steps: Mutex<BTreeMap<String, i64>>,
        pub(in crate::accounts) machine_codes: Mutex<BTreeSet<String>>,
    }

    impl Table {
        /// A permanent password for `person`.
        pub(in crate::accounts) fn with_password(self, person: &str, password: &str) -> Table {
            self.passwords.lock().unwrap().insert(person.into(), (super::super::phc::hash(password).unwrap(), false));
            self
        }

        /// A fresh machine code, as `magnus sign-in-code` prints one.
        pub(in crate::accounts) fn new_machine_code(&self) -> String {
            let code = format!("CODE{}", self.machine_codes.lock().unwrap().len());
            self.machine_codes.lock().unwrap().insert(code.clone());
            code
        }
    }

    #[async_trait::async_trait]
    impl Credentials for Table {
        async fn password(&self, person: &str) -> Result<Option<(String, bool)>> {
            Ok(self.passwords.lock().unwrap().get(person).cloned())
        }
        async fn status(&self, person: &str) -> Result<Option<Status>> {
            Ok(self.statuses.lock().unwrap().get(person).copied())
        }
        async fn totp_secret(&self, person: &str) -> Result<Option<String>> {
            Ok(self.totp.lock().unwrap().get(person).cloned())
        }
        async fn advance_totp_step(&self, person: &str, step: i64) -> Result<bool> {
            let mut steps = self.steps.lock().unwrap();
            if steps.get(person).is_some_and(|s| *s >= step) {
                return Ok(false);
            }
            steps.insert(person.into(), step);
            Ok(true)
        }
        async fn take_machine_code(&self, code: &str) -> Result<bool> {
            Ok(self.machine_codes.lock().unwrap().remove(code.trim()))
        }
    }
}
