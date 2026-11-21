//! One password check that owns its lockout: which counter an attempt
//! counts on, whether the identity provider or the store here judges the
//! password, and the order that keeps concurrent guesses from all passing
//! the lockout. Every place a person types a password (sign-in, the Admin
//! step-up, changing and choosing a password) goes through here; the
//! counters themselves are private to `accounts`.
//!
//! The rules, in one place:
//! - Counters are where the attempt came from: this machine apart from the
//!   network, so the network cannot lock a person out of the server itself,
//!   and the owner's backup apart from both.
//! - Only known people are counted, so guessing names cannot grow the table.
//! - With a provider, the attempt is counted before it answers, so requests
//!   sent at once cannot all pass the lockout first. A right answer clears
//!   the counter; an answer that says nothing about the password (it needs
//!   the code, its own page, or it is down) takes the count back.
//! - A locked attempt is counted too: hammering a lock extends it, up to the
//!   rules' longest wait, while waiting it out never does.
//! - The provider signing in someone other than the person asked about is a
//!   wrong password, never a yes.

use super::credentials::Credentials;
use super::lockout::{counters, Lockout};
use super::{key, Accounts, Policy, SignInError, Status};
use crate::identity::{Identity, Polled};
use anyhow::{Context, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::sync::Mutex;

/// Where a password was typed: on the server's own machine, or from
/// anywhere else. Each counts on its own counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Network,
    ThisMachine,
}

/// One password attempt, as typed.
pub struct Attempt<'a> {
    pub person: &'a str,
    pub password: &'a str,
    /// The code from the person's authenticator app, when they gave one.
    pub code: Option<&'a str>,
    /// The one-time code printed on the server's machine
    /// (`magnus sign-in-code`): the owner's backup, under a provider only.
    pub machine_code: Option<&'a str>,
    pub via: Via,
    /// Whether the app asks, for the provider's sign-in.
    pub app: bool,
}

/// What the server knows about the person, settled by
/// `crate::password_check::check`, the only caller.
pub struct Keeper {
    /// The identity provider that keeps the password, or None when the
    /// store here does.
    pub provider: Option<Arc<dyn Identity>>,
    /// Whether the person is one of this server's: the config's or the
    /// store's own. Only they are counted.
    pub known: bool,
    /// Whether the person may use the backup password under a provider.
    pub backup_allowed: bool,
}

/// What checking a password found.
pub enum Checked {
    /// The password (and code) are right. Under a provider, its answer,
    /// to open the session with the rights it gives.
    Yes(Option<Polled>),
    /// The password may be right; the authenticator code is missing.
    NeedsCode,
    /// A temporary password, right: it only chooses the permanent one.
    MustChoose,
    /// Wrong, with the provider's message when it gave one.
    Wrong(Option<String>),
    /// Too many wrong ones: no password is tried until then.
    Locked(DateTime<Utc>),
    /// The provider answered without judging the password: its own page
    /// must sign the person in, or the sign-in failed there.
    Elsewhere(Polled),
    /// The provider, named here, did not answer.
    Unavailable(String),
}

/// The counter an attempt from `via` counts on.
pub(super) fn counter_of(via: Via, person: &str) -> String {
    match via {
        Via::Network => Accounts::network_counter(person),
        Via::ThisMachine => Accounts::local_counter(person),
    }
}

/// The counter for codes guessed from one address, such as invitation
/// links: apart from every person's.
fn guesses_counter(from: std::net::IpAddr) -> String {
    format!("signup:{from}")
}

impl Accounts {
    /// Check a password where it is kept, counting it as the rules above
    /// say. Called through `crate::password_check::check`, which settles
    /// the `Keeper`.
    pub(crate) async fn check_password(&self, keeper: &Keeper, a: &Attempt<'_>) -> Result<Checked> {
        self.refresh_rules().await;
        self.checker().check_password(keeper, a).await
    }

    /// The check over this store, its counters and its rules as cached.
    fn checker(&self) -> Checker<'_, Accounts> {
        Checker { creds: self, lockout: &self.failures, policy: self.policy.lock().unwrap_or_else(|e| e.into_inner()).1 }
    }

    /// Read the lockout rules fresh, as another process may have changed
    /// them; `locked` and `failed` read the cached ones.
    async fn refresh_rules(&self) {
        if let Ok(mut g) = self.client().await {
            if let Some(c) = g.as_mut() {
                let _ = self.fresh_policy(c).await;
            }
        }
    }

    /// Read the rules fresh, failing when the store cannot be reached.
    async fn fresh_rules(&self) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let _ = self.fresh_policy(c).await;
        Ok(())
    }

    /// Until when codes guessed from `from` (an invitation link's) may not
    /// be tried again. Never for passwords: those go through
    /// `check_password`.
    pub fn guesses_locked(&self, from: std::net::IpAddr) -> Option<DateTime<Utc>> {
        self.locked(&guesses_counter(from))
    }

    /// Count a wrong code guessed from `from`.
    pub fn guessed_wrong(&self, from: std::net::IpAddr) {
        self.failed(&guesses_counter(from));
    }

    /// Until when a counter may not try another password, after too many
    /// wrong ones: the appliance's own and the identity provider's alike.
    /// Reads the cached rules; async auth entries refresh them first.
    pub(super) fn locked(&self, person: &str) -> Option<DateTime<Utc>> {
        let p = self.policy.lock().unwrap_or_else(|e| e.into_inner()).1;
        counters(&self.failures).locked(&key(person), Utc::now(), &p)
    }

    /// Count a wrong password (`Lockout::failed`). Only for people of this
    /// appliance, so guessing names cannot grow the table.
    pub(super) fn failed(&self, person: &str) {
        let p = self.policy.lock().unwrap_or_else(|e| e.into_inner()).1;
        counters(&self.failures).failed(&key(person), Utc::now(), &p);
    }

    pub(super) fn succeeded(&self, person: &str) {
        counters(&self.failures).succeeded(&key(person));
    }

    /// Clear every counter of a person: their own and each scope's. A
    /// status switch does this, so turning someone back on also unlocks
    /// them, including out of a permanent lockout.
    pub(super) fn clear_counters(&self, person: &str) {
        let person = key(person);
        let mut f = counters(&self.failures);
        for c in [person.clone(), Self::network_counter(&person), Self::local_counter(&person), Self::console_counter(&person), Self::backup_counter(&person)] {
            f.clear(&key(&c));
        }
    }

    /// The counter that guards the owner's backup password: its own, so
    /// wrong passwords sent to the identity provider from anywhere cannot
    /// lock the owner out of the backup on the appliance's machine.
    pub(super) fn backup_counter(person: &str) -> String {
        format!("backup:{person}")
    }

    /// The counter for attempts made over the network. Every counter is a
    /// scope and the whole person id, so no two people's counters meet,
    /// whatever their ids.
    pub(super) fn network_counter(person: &str) -> String {
        format!("net:{person}")
    }

    /// The counter for attempts made on the appliance's own machine: its own,
    /// so wrong passwords sent from the network cannot lock the person out
    /// of the appliance itself.
    pub(super) fn local_counter(person: &str) -> String {
        format!("local:{person}")
    }

    /// The counter for console sign-in attempts: its own, so guessing at
    /// the console cannot lock the person's apps, and guessing at the apps
    /// cannot lock the console (#429).
    pub(super) fn console_counter(person: &str) -> String {
        format!("console:{person}")
    }

    /// Check a password. The caller opens the session.
    #[cfg(test)]
    pub(super) async fn sign_in(&self, person: &str, password: &str) -> Result<std::result::Result<(), SignInError>> {
        self.sign_in_counted(person, password, &Self::network_counter(person)).await
    }

    /// Check a password, counting wrong ones on `counter`.
    pub(super) async fn sign_in_counted(&self, person: &str, password: &str, counter: &str) -> Result<std::result::Result<(), SignInError>> {
        self.fresh_rules().await?;
        self.checker().sign_in_counted(person, password, counter).await
    }

    pub(super) fn password_ok(phc: Option<&str>, password: &str) -> bool {
        // Verify against a throwaway hash when the person is unknown, so
        // timing does not tell which names exist.
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default().hash_password(salt.as_str().as_bytes(), &salt).map(|h| h.to_string()).unwrap_or_default()
        });
        PasswordHash::new(phc.unwrap_or(dummy)).map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok()).unwrap_or(false)
    }

    /// Check a password and, where the person enrolled one, the code from
    /// their authenticator app. A missing code answers `NeedsCode`
    /// (uncounted); a wrong code rejects and counts, like a wrong password.
    #[cfg(test)]
    pub(super) async fn sign_in_counted_with_code(&self, person: &str, password: &str, code: Option<&str>, counter: &str) -> Result<std::result::Result<(), SignInError>> {
        self.sign_in_counted_with_code_at(person, password, code, counter, Utc::now()).await
    }

    /// `sign_in_counted_with_code` at `at`, for tests.
    #[cfg(test)]
    pub(super) async fn sign_in_counted_with_code_at(
        &self,
        person: &str,
        password: &str,
        code: Option<&str>,
        counter: &str,
        at: DateTime<Utc>,
    ) -> Result<std::result::Result<(), SignInError>> {
        self.fresh_rules().await?;
        self.checker().sign_in_counted_with_code_at(person, password, code, counter, at).await
    }
}

/// One check over a store (`Credentials`), the in-memory counters and the
/// sign-in rules read at its start. The counters see the real clock; the
/// authenticator code is judged at the time it is given.
pub(super) struct Checker<'a, C: Credentials + ?Sized> {
    pub(super) creds: &'a C,
    pub(super) lockout: &'a Mutex<Lockout>,
    pub(super) policy: Policy,
}

impl<C: Credentials + ?Sized> Checker<'_, C> {
    fn locked(&self, counter: &str) -> Option<DateTime<Utc>> {
        counters(self.lockout).locked(&key(counter), Utc::now(), &self.policy)
    }

    fn failed(&self, counter: &str) {
        counters(self.lockout).failed(&key(counter), Utc::now(), &self.policy);
    }

    fn succeeded(&self, counter: &str) {
        counters(self.lockout).succeeded(&key(counter));
    }

    /// Take back one counted attempt: one counted before its answer came,
    /// whose answer said nothing about the password (the provider was down).
    fn forgive(&self, counter: &str) {
        counters(self.lockout).forgive(&key(counter));
    }

    pub(super) async fn check_password(&self, keeper: &Keeper, a: &Attempt<'_>) -> Result<Checked> {
        let person = key(a.person);
        let Some(provider) = keeper.provider.as_deref() else {
            return self.check_here(keeper.known, &person, a).await;
        };
        if let Some(machine) = a.machine_code {
            return self.check_backup(keeper, &person, a.password, machine, a.via).await;
        }
        let counter = counter_of(a.via, &person);
        let known = keeper.known;
        if let Some(until) = self.locked(&counter) {
            if known {
                self.failed(&counter);
            }
            return Ok(Checked::Locked(self.locked(&counter).unwrap_or(until)));
        }
        if known {
            self.failed(&counter);
        }
        let take_back = || {
            if known {
                self.forgive(&counter);
            }
        };
        Ok(match provider.password(&person, a.password, a.code, a.app).await {
            Ok(polled @ Polled::Done { .. }) => {
                if matches!(&polled, Polled::Done { person: who, .. } if key(who) == person) {
                    if known {
                        self.succeeded(&counter);
                    }
                    Checked::Yes(Some(polled))
                } else {
                    // Someone else signed in: not this person's password,
                    // and it stays counted.
                    Checked::Wrong(None)
                }
            }
            Ok(Polled::Refused(m)) => Checked::Wrong(Some(m)),
            Ok(Polled::NeedsCode) => {
                take_back();
                Checked::NeedsCode
            }
            Ok(other) => {
                take_back();
                Checked::Elsewhere(other)
            }
            Err(e) => {
                tracing::warn!("identity: {} could not check a password: {e:#}", provider.label());
                take_back();
                Checked::Unavailable(provider.label())
            }
        })
    }

    /// The store here judges: its password and, where enrolled, the code.
    /// Someone the server does not know gets the same work and answer as a
    /// wrong password, uncounted.
    async fn check_here(&self, known: bool, person: &str, a: &Attempt<'_>) -> Result<Checked> {
        if !known {
            let _ = Accounts::password_ok(None, a.password);
            return Ok(Checked::Wrong(None));
        }
        Ok(match self.sign_in_counted_with_code_at(person, a.password, a.code, &counter_of(a.via, person), Utc::now()).await? {
            Ok(()) => Checked::Yes(None),
            Err(SignInError::NeedsCode) => Checked::NeedsCode,
            Err(SignInError::MustChoose) => Checked::MustChoose,
            Err(SignInError::Rejected) => Checked::Wrong(None),
            Err(SignInError::Locked(until)) => Checked::Locked(until),
        })
    }

    /// The owner's backup under a provider: their password set here and a
    /// one-time code printed on the server's machine, which no relay or
    /// tunnel can carry. Counted apart, so wrong passwords sent to the
    /// provider cannot lock the owner out of it. The machine code is the
    /// second factor, so no authenticator code is asked.
    async fn check_backup(&self, keeper: &Keeper, person: &str, password: &str, machine: &str, via: Via) -> Result<Checked> {
        if !(keeper.backup_allowed && via == Via::ThisMachine && self.creds.password(person).await.ok().flatten().is_some()) {
            return Ok(Checked::Wrong(None));
        }
        let counter = Accounts::backup_counter(person);
        if let Some(until) = self.locked(&counter) {
            self.failed(&counter);
            return Ok(Checked::Locked(self.locked(&counter).unwrap_or(until)));
        }
        if !self.creds.take_machine_code(machine).await? {
            self.failed(&counter);
            return Ok(Checked::Wrong(None));
        }
        Ok(match self.sign_in_counted(person, password, &counter).await? {
            Ok(()) => Checked::Yes(None),
            Err(SignInError::Locked(until)) => Checked::Locked(until),
            // A temporary password never takes the backup.
            Err(SignInError::Rejected | SignInError::NeedsCode | SignInError::MustChoose) => Checked::Wrong(None),
        })
    }

    /// Check a password, counting wrong ones on `counter`.
    pub(super) async fn sign_in_counted(&self, person: &str, password: &str, counter: &str) -> Result<std::result::Result<(), SignInError>> {
        let person = key(person);
        let counter = key(counter);
        if let Some(until) = self.locked(&counter) {
            // Counted too: hammering a lock extends it, up to the rules'
            // longest wait, while waiting it out never does.
            self.failed(&counter);
            return Ok(Err(SignInError::Locked(self.locked(&counter).unwrap_or(until))));
        }
        let row = self.creds.password(&person).await?;
        let Some((phc, temporary)) = row else {
            // The same work as a wrong password, so timing does not tell
            // which names exist.
            let _ = Accounts::password_ok(None, password);
            return Ok(Err(SignInError::Rejected));
        };
        if !Accounts::password_ok(Some(&phc), password) {
            self.failed(&counter);
            return Ok(Err(SignInError::Rejected));
        }
        if temporary {
            return Ok(Err(SignInError::MustChoose));
        }
        self.succeeded(&counter);
        Ok(Ok(()))
    }

    /// Check a password and, where enrolled, the code, at `at`.
    pub(super) async fn sign_in_counted_with_code_at(
        &self,
        person: &str,
        password: &str,
        code: Option<&str>,
        counter: &str,
        at: DateTime<Utc>,
    ) -> Result<std::result::Result<(), SignInError>> {
        let person = key(person);
        let counter = key(counter);
        let policy = self.policy;
        if let Some(until) = self.locked(&counter) {
            self.failed(&counter);
            return Ok(Err(SignInError::Locked(self.locked(&counter).unwrap_or(until))));
        }
        let row = self.creds.password(&person).await?;
        let Some((phc, temporary)) = row else {
            let _ = Accounts::password_ok(None, password);
            return Ok(Err(SignInError::Rejected));
        };
        if !Accounts::password_ok(Some(&phc), password) {
            self.failed(&counter);
            return Ok(Err(SignInError::Rejected));
        }
        // A temporary password only chooses: it never reaches the code,
        // whatever the rules require.
        if temporary {
            return Ok(Err(SignInError::MustChoose));
        }
        // After the password, so timing does not tell the status apart, and
        // before the code, so no NeedsCode betrays a right password for
        // someone turned off. Uncounted: the password was not wrong.
        if self.creds.status(&person).await?.is_some_and(|s| s != Status::Active) {
            return Ok(Err(SignInError::Rejected));
        }
        // The password was right, so the person is known: a wrong code
        // counts on their counter, as a wrong password does.
        let secret = self.creds.totp_secret(&person).await?;
        let Some(secret) = secret else {
            // With a code required, a password alone signs no one in —
            // they enroll first, through the owner — answered as a wrong
            // password, and uncounted, as the password was not wrong.
            if policy.code_required {
                return Ok(Err(SignInError::Rejected));
            }
            self.succeeded(&counter);
            return Ok(Ok(()));
        };
        match code.map(str::trim).filter(|c| !c.is_empty()) {
            None => Ok(Err(SignInError::NeedsCode)),
            Some(one) => match crate::totp::verify_step(&secret, one, at) {
                Some(matched) => {
                    // Recorded by compare-and-set, so concurrent uses of
                    // the code admit exactly one; a used or passed step
                    // rejects and counts, like a wrong code (#441).
                    if !self.creds.advance_totp_step(&person, matched).await? {
                        self.failed(&counter);
                        return Ok(Err(SignInError::Rejected));
                    }
                    self.succeeded(&counter);
                    Ok(Ok(()))
                }
                None => {
                    self.failed(&counter);
                    Ok(Err(SignInError::Rejected))
                }
            },
        }
    }
}

#[cfg(test)]
#[path = "check_tests.rs"]
mod tests;
