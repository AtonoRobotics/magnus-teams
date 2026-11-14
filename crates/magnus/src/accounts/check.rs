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

use super::{key, Accounts, Failures, SignInError, PERMANENT_YEARS};
use crate::identity::{Identity, Polled};
use anyhow::{Context, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;

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
        let person = key(a.person);
        let Some(provider) = keeper.provider.as_deref() else {
            return self.check_here(keeper.known, &person, a).await;
        };
        if let Some(machine) = a.machine_code {
            return self.check_backup(keeper, &person, a.password, machine, a.via).await;
        }
        self.refresh_rules().await;
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
            let _ = Self::password_ok(None, a.password);
            return Ok(Checked::Wrong(None));
        }
        Ok(match self.sign_in_counted_with_code(person, a.password, a.code, &counter_of(a.via, person)).await? {
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
        if !(keeper.backup_allowed && via == Via::ThisMachine && self.has_password(person).await) {
            return Ok(Checked::Wrong(None));
        }
        self.refresh_rules().await;
        let counter = Self::backup_counter(person);
        if let Some(until) = self.locked(&counter) {
            self.failed(&counter);
            return Ok(Checked::Locked(self.locked(&counter).unwrap_or(until)));
        }
        if !self.take_machine_code(machine).await? {
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

    /// Read the lockout rules fresh, as another process may have changed
    /// them; `locked` and `failed` read the cached ones.
    async fn refresh_rules(&self) {
        if let Ok(mut g) = self.client().await {
            if let Some(c) = g.as_mut() {
                let _ = self.fresh_policy(c).await;
            }
        }
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
        let person = key(person);
        if !self.policy.lock().unwrap().1.lockout_enabled {
            return None;
        }
        let now = Utc::now();
        let horizon = Duration::minutes(self.policy.lock().unwrap().1.lockout_max_wait_minutes.max(1));
        let mut f = self.failures.lock().unwrap();
        let r = f.get(person.as_str()).copied()?;
        if r.until.is_some_and(|u| now < u) {
            return r.until;
        }
        // A lapsed lock, or failures gone quiet past the longest wait,
        // forgives the counter.
        if r.until.is_some() || now - r.first > horizon {
            f.remove(person.as_str());
        }
        None
    }

    /// Count a wrong password. Only for people of this appliance, so
    /// guessing names cannot grow the table. Past the threshold each
    /// failure holds the counter until now plus the first wait, capped
    /// at the first failure plus the longest wait — or out the century,
    /// when a lockout is permanent.
    pub(super) fn failed(&self, person: &str) {
        let person = key(person);
        let p = self.policy.lock().unwrap().1;
        if !p.lockout_enabled {
            return;
        }
        let now = Utc::now();
        let mut f = self.failures.lock().unwrap();
        let e = f.entry(person).or_insert(Failures { n: 0, first: now, until: None });
        e.n += 1;
        if e.n >= p.lockout_failures.max(1) as u32 {
            e.until = Some(if p.lockout_permanent {
                e.first + Duration::days(365 * PERMANENT_YEARS)
            } else {
                (e.until.filter(|u| *u > now).unwrap_or(now) + Duration::minutes(p.lockout_wait_minutes.max(1))).min(e.first + Duration::minutes(p.lockout_max_wait_minutes.max(1)))
            });
        }
    }

    pub(super) fn succeeded(&self, person: &str) {
        let person = key(person);
        self.failures.lock().unwrap().remove(person.as_str());
    }

    /// Take back one counted attempt: one counted before its answer came,
    /// whose answer said nothing about the password (the provider was down).
    pub(super) fn forgive(&self, person: &str) {
        let person = key(person);
        let mut f = self.failures.lock().unwrap();
        if let Some(e) = f.get_mut(person.as_str()) {
            e.n = e.n.saturating_sub(1);
        }
    }

    /// Clear every counter of a person: their own and each scope's. A
    /// status switch does this, so turning someone back on also unlocks
    /// them, including out of a permanent lockout.
    pub(super) fn clear_counters(&self, person: &str) {
        let person = key(person);
        let mut f = self.failures.lock().unwrap();
        for c in [person.clone(), Self::network_counter(&person), Self::local_counter(&person), Self::console_counter(&person), Self::backup_counter(&person)] {
            f.remove(&key(&c));
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
        let person = key(person);
        let counter = key(counter);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let _ = self.fresh_policy(c).await;
        if let Some(until) = self.locked(&counter) {
            // Counted too: hammering a lock extends it, up to the rules'
            // longest wait, while waiting it out never does.
            self.failed(&counter);
            return Ok(Err(SignInError::Locked(self.locked(&counter).unwrap_or(until))));
        }
        let row: Option<(String, bool)> = c.query_opt("SELECT phc, temporary FROM public.account_passwords WHERE person = $1", &[&person]).await?.map(|r| (r.get(0), r.get(1)));
        let Some((phc, temporary)) = row else {
            // The same work as a wrong password, so timing does not tell
            // which names exist.
            let _ = Self::password_ok(None, password);
            return Ok(Err(SignInError::Rejected));
        };
        if !Self::password_ok(Some(&phc), password) {
            self.failed(&counter);
            return Ok(Err(SignInError::Rejected));
        }
        if temporary {
            return Ok(Err(SignInError::MustChoose));
        }
        self.succeeded(&counter);
        Ok(Ok(()))
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
    pub(super) async fn sign_in_counted_with_code(&self, person: &str, password: &str, code: Option<&str>, counter: &str) -> Result<std::result::Result<(), SignInError>> {
        self.sign_in_counted_with_code_at(person, password, code, counter, Utc::now()).await
    }

    /// `sign_in_counted_with_code` at `at`, for tests.
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
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let policy = self.fresh_policy(c).await;
        if let Some(until) = self.locked(&counter) {
            self.failed(&counter);
            return Ok(Err(SignInError::Locked(self.locked(&counter).unwrap_or(until))));
        }
        let row: Option<(String, bool)> = c.query_opt("SELECT phc, temporary FROM public.account_passwords WHERE person = $1", &[&person]).await?.map(|r| (r.get(0), r.get(1)));
        let Some((phc, temporary)) = row else {
            let _ = Self::password_ok(None, password);
            return Ok(Err(SignInError::Rejected));
        };
        if !Self::password_ok(Some(&phc), password) {
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
        let status: Option<String> = c.query_opt("SELECT status FROM public.account_statuses WHERE person = $1", &[&person]).await?.map(|r| r.get(0));
        if status.as_deref().is_some_and(|s| s != "active") {
            return Ok(Err(SignInError::Rejected));
        }
        // The password was right, so the person is known: a wrong code
        // counts on their counter, as a wrong password does.
        let secret: Option<String> = c.query_opt("SELECT secret FROM public.account_totp WHERE person = $1", &[&person]).await?.map(|r| r.get(0));
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
                    let n = c
                        .execute(
                            "INSERT INTO public.account_totp_steps (person, step) VALUES ($1, $2)
                             ON CONFLICT (person) DO UPDATE SET step = EXCLUDED.step WHERE account_totp_steps.step < EXCLUDED.step",
                            &[&person, &matched],
                        )
                        .await?;
                    if n != 1 {
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
mod tests {
    use super::super::tests::{cfg, db};
    use super::*;
    use crate::identity::{Begun, Rights};

    const RIGHT: &str = "ada-right-password";
    const WRONG: &str = "not-the-password";

    /// How a provider answers a password.
    type Answer = fn() -> Result<Polled>;

    /// A provider that answers every password the same way.
    struct Says(Answer);

    #[async_trait::async_trait]
    impl Identity for Says {
        fn label(&self) -> String {
            "Keycloak".into()
        }
        async fn begin(&self, _: &str, _: Option<&str>, _: bool) -> Result<Begun> {
            anyhow::bail!("not in this test")
        }
        async fn finish(&self, _: &str, _: &str, _: Option<&str>) -> Result<Polled> {
            anyhow::bail!("not in this test")
        }
        async fn password(&self, _: &str, _: &str, _: Option<&str>, _: bool) -> Result<Polled> {
            (self.0)()
        }
        async fn active(&self, _: &str, _: Option<&str>) -> Result<bool> {
            Ok(true)
        }
        async fn in_directory(&self, _: &str) -> Result<bool> {
            Ok(false)
        }
    }

    fn done(who: &str) -> Polled {
        Polled::Done { person: who.into(), app: true, rights: Rights::default() }
    }
    fn up_ada() -> Result<Polled> {
        Ok(done("ada"))
    }
    fn up_eve() -> Result<Polled> {
        Ok(done("eve"))
    }
    fn refused() -> Result<Polled> {
        Ok(Polled::Refused("The name or password is wrong.".into()))
    }
    fn needs_code() -> Result<Polled> {
        Ok(Polled::NeedsCode)
    }
    fn browser() -> Result<Polled> {
        Ok(Polled::UseBrowser("Sign in on the page.".into()))
    }
    fn down() -> Result<Polled> {
        anyhow::bail!("connection refused")
    }

    #[derive(Debug, PartialEq)]
    enum Is {
        Yes,
        NeedsCode,
        MustChoose,
        Wrong,
        Locked,
        Elsewhere,
        Unavailable,
    }

    fn is(c: &Checked) -> Is {
        match c {
            Checked::Yes(_) => Is::Yes,
            Checked::NeedsCode => Is::NeedsCode,
            Checked::MustChoose => Is::MustChoose,
            Checked::Wrong(_) => Is::Wrong,
            Checked::Locked(_) => Is::Locked,
            Checked::Elsewhere(_) => Is::Elsewhere,
            Checked::Unavailable(_) => Is::Unavailable,
        }
    }

    /// The machine code an attempt brings.
    #[derive(Clone, Copy)]
    enum Machine {
        None,
        Fresh,
        Bogus,
    }

    struct Row {
        what: &'static str,
        provider: Option<Answer>,
        known: bool,
        backup_allowed: bool,
        person: &'static str,
        password: &'static str,
        machine: Machine,
        via: Via,
        expect: Is,
        /// Each counter's count after the attempt.
        counts: &'static [(&'static str, u32)],
    }

    fn count(a: &Accounts, counter: &str) -> u32 {
        a.failures.lock().unwrap().get(counter).map_or(0, |f| f.n)
    }

    fn keeper(provider: Option<Answer>, known: bool, backup_allowed: bool) -> Keeper {
        Keeper { provider: provider.map(|f| Arc::new(Says(f)) as Arc<dyn Identity>), known, backup_allowed }
    }

    async fn attempt(a: &Accounts, k: &Keeper, person: &str, password: &str, machine: Machine, via: Via) -> Checked {
        let code = match machine {
            Machine::None => None,
            Machine::Fresh => Some(a.new_machine_code().await.unwrap()),
            Machine::Bogus => Some("000000".to_string()),
        };
        a.check_password(k, &Attempt { person, password, code: None, machine_code: code.as_deref(), via, app: true }).await.unwrap()
    }

    fn row(
        what: &'static str,
        provider: Option<Answer>,
        (known, backup_allowed): (bool, bool),
        (person, password, machine, via): (&'static str, &'static str, Machine, Via),
        expect: Is,
        counts: &'static [(&'static str, u32)],
    ) -> Row {
        Row { what, provider, known, backup_allowed, person, password, machine, via, expect, counts }
    }

    /// Every way a password is answered, through the one check: the
    /// provider up, down, wanting the code or its page, refusing, or
    /// signing in someone else; the store here, from the network and from
    /// this machine; the owner's backup; and who is counted where.
    #[tokio::test(flavor = "multi_thread")]
    async fn every_answer_counts_where_it_should() {
        use Machine::{Bogus, Fresh, None as NoCode};
        use Via::{Network, ThisMachine};
        let (_db, url) = db().await;
        let a = Accounts::open(&url, cfg()).await.unwrap();
        a.set_password("ada", RIGHT).await.unwrap();
        let ours = (true, false);
        let owner = (true, true);
        let stranger = (false, false);
        let rows = [
            row("provider says yes", Some(up_ada), ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
            row("provider signs in someone else", Some(up_eve), ours, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("provider refuses", Some(refused), ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("provider wants the code", Some(needs_code), ours, ("ada", RIGHT, NoCode, Network), Is::NeedsCode, &[("net:ada", 0)]),
            row("provider wants its page", Some(browser), ours, ("ada", RIGHT, NoCode, Network), Is::Elsewhere, &[("net:ada", 0)]),
            row("provider down", Some(down), ours, ("ada", RIGHT, NoCode, Network), Is::Unavailable, &[("net:ada", 0)]),
            row("provider refuses on this machine", Some(refused), ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
            row("provider refuses a stranger", Some(refused), stranger, ("nobody", WRONG, NoCode, Network), Is::Wrong, &[("net:nobody", 0)]),
            row("store says yes", None, ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
            row("store refuses", None, ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("store refuses on this machine", None, ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
            row("store ignores a machine code", None, ours, ("ada", RIGHT, Bogus, ThisMachine), Is::Yes, &[("local:ada", 0)]),
            row("store does not know them", None, stranger, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 0)]),
            row("backup says yes", Some(down), owner, ("ada", RIGHT, Fresh, ThisMachine), Is::Yes, &[("backup:ada", 0)]),
            row("backup, wrong machine code", Some(down), owner, ("ada", RIGHT, Bogus, ThisMachine), Is::Wrong, &[("backup:ada", 1)]),
            row("backup, wrong password", Some(down), owner, ("ada", WRONG, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 1), ("local:ada", 0)]),
            row("backup from the network", Some(down), owner, ("ada", RIGHT, Fresh, Network), Is::Wrong, &[("backup:ada", 0)]),
            row("backup for someone else", Some(down), ours, ("ada", RIGHT, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 0)]),
        ];
        for r in rows {
            a.failures.lock().unwrap().clear();
            let k = keeper(r.provider, r.known, r.backup_allowed);
            let got = attempt(&a, &k, r.person, r.password, r.machine, r.via).await;
            assert_eq!(is(&got), r.expect, "{}", r.what);
            for (counter, n) in r.counts {
                assert_eq!(count(&a, counter), *n, "{}: {counter}", r.what);
            }
        }
    }

    /// Past the threshold the counter locks, whoever judges the password:
    /// even the right one is refused, and every locked attempt extends the
    /// lock up to the longest wait. Once it lapses the right one signs in
    /// and clears it.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_lockout_holds_extends_and_lapses() {
        let (_db, url) = db().await;
        let a = Accounts::open(&url, cfg()).await.unwrap();
        a.set_password("ada", RIGHT).await.unwrap();
        let mut p = a.policy().await.unwrap();
        p.lockout_failures = 3;
        p.lockout_wait_minutes = 5;
        p.lockout_max_wait_minutes = 6;
        a.set_policy(p).await.unwrap();
        let cases: [(&str, Option<Answer>, Option<Answer>); 2] = [("provider", Some(refused), Some(up_ada)), ("store", None, None)];
        for (what, wrong, right) in cases {
            a.failures.lock().unwrap().clear();
            let (wrong, right) = (keeper(wrong, true, false), keeper(right, true, false));
            for n in 1..3 {
                assert_eq!(is(&attempt(&a, &wrong, "ada", WRONG, Machine::None, Via::Network).await), Is::Wrong, "{what}: wrong {n}");
            }
            assert_eq!(is(&attempt(&a, &wrong, "ada", WRONG, Machine::None, Via::Network).await), Is::Wrong, "{what}: the third wrong");
            let first = a.locked("net:ada").unwrap_or_else(|| panic!("{what}: three wrong did not lock"));
            let Checked::Locked(later) = attempt(&a, &right, "ada", RIGHT, Machine::None, Via::Network).await else { panic!("{what}: the lock lapsed") };
            assert!(later > first, "{what}: a locked attempt did not extend the lock: {first} to {later}");
            assert!(later - Utc::now() <= Duration::minutes(6), "{what}: the lock passed the longest wait: {later}");
            assert_eq!(is(&attempt(&a, &right, "ada", RIGHT, Machine::None, Via::ThisMachine).await), Is::Yes, "{what}: the network's lock held this machine");
            a.failures.lock().unwrap().get_mut("net:ada").unwrap().until = Some(Utc::now() - Duration::seconds(1));
            assert_eq!(is(&attempt(&a, &right, "ada", RIGHT, Machine::None, Via::Network).await), Is::Yes, "{what}: a lapsed lock still refused");
            assert_eq!(count(&a, "net:ada"), 0, "{what}: the right password did not clear the counter");
        }
    }

    /// The backup's own lock: wrong machine codes lock it, the provider's
    /// counter is untouched, and a locked attempt extends it.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_backup_locks_apart() {
        let (_db, url) = db().await;
        let a = Accounts::open(&url, cfg()).await.unwrap();
        a.set_password("ada", RIGHT).await.unwrap();
        let mut p = a.policy().await.unwrap();
        p.lockout_failures = 2;
        p.lockout_wait_minutes = 5;
        p.lockout_max_wait_minutes = 6;
        a.set_policy(p).await.unwrap();
        let k = keeper(Some(down), true, true);
        for _ in 0..2 {
            assert_eq!(is(&attempt(&a, &k, "ada", RIGHT, Machine::Bogus, Via::ThisMachine).await), Is::Wrong);
        }
        let first = a.locked("backup:ada").expect("wrong machine codes did not lock the backup");
        let Checked::Locked(later) = attempt(&a, &k, "ada", RIGHT, Machine::Fresh, Via::ThisMachine).await else { panic!("the backup's lock lapsed") };
        assert!(later > first, "a locked backup attempt did not extend the lock");
        assert_eq!(count(&a, "local:ada") + count(&a, "net:ada"), 0, "the backup counted on the provider's counters");
    }
}
