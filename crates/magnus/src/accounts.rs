//! Password sign-in and sessions for people's apps. Sign-in through an
//! identity provider (`oidc`) opens the same sessions.
//!
//! People, password hashes (Argon2id), sessions and the rest live in the
//! appliance's Postgres (`account_*` tables, made by `migrate`), never in
//! the log: a session token is a credential (invariant 2). The log records
//! only that a person signed in or out, with the session id. Every check
//! reads the store, so a removal or disabling from another process (the
//! CLI's own handle) takes effect here at once, and re-adding an id never
//! resurrects its ended sessions (#466, #469).
//!
//! The one exception is the directory itself (`is_local`, `local_person`,
//! `local_people`): resolving a person is sync (`Kernel::person`), and the
//! database is not, so those three read a cache every other call refreshes
//! when it is older than `LOCAL_TTL`. A name or address may lag half a
//! minute on a quiet server; whether anyone may sign in or stay signed in
//! is always read fresh, so the lag never admits anyone.

use crate::config::SignIn;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;
use tokio_postgres::Client;
mod check;
mod credentials;
mod directory;
mod lockout;
mod password;
mod phc;
mod reads;
mod rules;
mod schema;
mod sessions;
mod tokens;
pub use check::{Attempt, Checked, Keeper, Via};
pub use schema::{import_json, migrate_sql, TABLES};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    pub person: String,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    /// What signed in: "app" or "browser".
    #[serde(default)]
    pub client: String,
    /// The paired device this session was opened on, when it came through
    /// the tunnel or was bound at pairing. Absent on a session that is
    /// older than device certificates, and on a LAN session not yet paired.
    #[serde(default)]
    pub device: Option<String>,
}

pub enum SignInError {
    /// Wrong person or password; the caller says nothing more.
    Rejected,
    /// Too many failures; try again after this time.
    Locked(DateTime<Utc>),
    /// The password was right and the person uses an authenticator app:
    /// ask for the code and try again. Uncounted: needing the code says
    /// nothing was wrong, as with the identity provider's `NeedsCode`.
    NeedsCode,
    /// The password was right but temporary: it chooses the permanent
    /// one, never signs in. Uncounted, like `NeedsCode`.
    MustChoose,
}

/// Whether a person may sign in: invited until they accept, active after,
/// disabled when turned off. Nobody recorded counts as active, so everyone
/// already here stays signed in (#429).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Invited,
    Active,
    Disabled,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Invited => "invited",
            Status::Active => "active",
            Status::Disabled => "disabled",
        }
    }

    fn parse(s: &str) -> Status {
        match s {
            "invited" => Status::Invited,
            "disabled" => Status::Disabled,
            _ => Status::Active,
        }
    }
}

/// A live invitation: who it is for, where the link went, and until when
/// it accepts. Stored by the hash of its token; the token itself is shown
/// once, at the invite.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Invite {
    pub person: String,
    pub email: Option<String>,
    pub expires: DateTime<Utc>,
}

/// What a person may administer: admins enter the console, members do not.
/// Nobody recorded is a member. This is the console axis, not the kernel's
/// owner/developer roles (#429).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    fn parse(s: &str) -> Role {
        match s {
            "admin" => Role::Admin,
            _ => Role::Member,
        }
    }
}

/// The sign-in rules the console shows under Security › Sign-in and the
/// kernel enforces on the local path (#508): whether a code is required,
/// how long member sessions last idle and at most, and when repeated
/// failures lock an account. One row (`id = 1`) in
/// `account_sign_in_policy`, seeded from the config at first open.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Policy {
    pub code_required: bool,
    pub idle_minutes: i64,
    pub max_hours: i64,
    pub lockout_enabled: bool,
    pub lockout_failures: i64,
    pub lockout_wait_minutes: i64,
    pub lockout_max_wait_minutes: i64,
    pub lockout_permanent: bool,
}

impl Policy {
    fn seed(cfg: &SignIn) -> Policy {
        Policy {
            code_required: false,
            idle_minutes: 30,
            max_hours: cfg.session_days as i64 * 24,
            lockout_enabled: true,
            lockout_failures: cfg.max_failures as i64,
            lockout_wait_minutes: cfg.lockout_minutes as i64,
            // No escalation until an admin sets a longer wait: the lock
            // then lasts exactly the first wait, as before.
            lockout_max_wait_minutes: cfg.lockout_minutes as i64,
            lockout_permanent: false,
        }
    }
}

/// Days an invitation stays usable.
pub const INVITE_DAYS: i64 = 7;

/// Hours a password-reset link stays usable.
pub const RESET_HOURS: i64 = 1;

/// Seconds before a reset request sends again: re-asking inside the
/// window answers the same and sends nothing.
pub const RESET_RESEND_SECONDS: i64 = 60;

/// A password-reset request that found its person: who, a token good for
/// one email, and whether to send it (a recent request answers the same
/// and sends nothing).
pub struct ResetRequest {
    pub person: String,
    pub token: String,
    pub send: bool,
}

/// How long the directory cache serves before a call refreshes it.
const LOCAL_TTL: std::time::Duration = std::time::Duration::from_secs(30);

pub struct Accounts {
    cfg: SignIn,
    url: String,
    client: tokio::sync::Mutex<Option<Client>>,
    /// Failed attempts per counter. In memory, as before: a restart
    /// forgives guesses in flight.
    failures: Mutex<lockout::Lockout>,
    /// The directory (id to record) and when it was read. See the module docs.
    local: Mutex<(Instant, BTreeMap<String, LocalPerson>)>,
    /// The sign-in rules and when they were read. Async auth entries
    /// refresh them from the store first, so a changed rule bites at
    /// once; per-request session checks serve the cache within its TTL.
    policy: Mutex<(Instant, Policy)>,
    /// Unconfirmed authenticator secrets started in Settings › Account,
    /// promoted to `account_totp` only after the person confirms a 6-digit code.
    pending_totp: Mutex<BTreeMap<String, String>>,
}

fn token_hash(token: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

fn session_of(r: &tokio_postgres::Row) -> Session {
    Session { id: r.get("id"), person: r.get("person"), created: r.get("created"), expires: r.get("expires"), client: r.get("client"), device: r.get("device") }
}

/// A person the appliance itself keeps: config-independent identity for
/// the embedded directory (#429). The id is the key, not repeated here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalPerson {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

pub(crate) fn valid_person_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Person ids are lowercase-canonical: sign-in lowercases at entry, so
/// every read and write goes through here and `Ada` never becomes a
/// second, unreachable account beside `ada` (#470).
fn key(person: &str) -> String {
    person.to_lowercase()
}

impl Accounts {
    /// Open the store, connecting at once (so a dead database fails here,
    /// not at the first sign-in) and warming the directory cache.
    pub async fn open(db_url: &str, cfg: SignIn) -> Result<Accounts> {
        let client = crate::log::connect(db_url).await?;
        let local = Self::read_local(&client).await?;
        let seed = Policy::seed(&cfg);
        client
            .execute(
                "INSERT INTO public.account_sign_in_policy (id, code_required, idle_minutes, max_hours, lockout_enabled, lockout_failures, lockout_wait_minutes, lockout_max_wait_minutes, lockout_permanent)
                 VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
                &[&seed.code_required, &seed.idle_minutes, &seed.max_hours, &seed.lockout_enabled, &seed.lockout_failures, &seed.lockout_wait_minutes, &seed.lockout_max_wait_minutes, &seed.lockout_permanent],
            )
            .await
            .context("seed the sign-in rules")?;
        let policy = Self::read_policy(&client).await?.unwrap_or(seed);
        Ok(Accounts {
            cfg,
            url: db_url.into(),
            client: tokio::sync::Mutex::new(Some(client)),
            failures: Mutex::new(lockout::Lockout::default()),
            local: Mutex::new((Instant::now(), local)),
            policy: Mutex::new((Instant::now(), policy)),
            pending_totp: Mutex::new(BTreeMap::new()),
        })
    }

    async fn client(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Client>>> {
        let mut g = self.client.lock().await;
        if g.as_ref().is_none_or(|c| c.is_closed()) {
            *g = Some(crate::log::connect(&self.url).await?);
        }
        Ok(g)
    }

    /// The directory as it stands, refreshing the cache first when it is
    /// older than `LOCAL_TTL`. Every async call starts here.
    async fn fresh_local(&self, c: &Client) -> Result<()> {
        if self.local.lock().unwrap_or_else(|e| e.into_inner()).0.elapsed() > LOCAL_TTL {
            self.refresh_local(c).await?;
        }
        Ok(())
    }

    async fn read_policy(c: &Client) -> Result<Option<Policy>> {
        Ok(c
            .query_opt("SELECT code_required, idle_minutes, max_hours, lockout_enabled, lockout_failures, lockout_wait_minutes, lockout_max_wait_minutes, lockout_permanent FROM public.account_sign_in_policy WHERE id = 1", &[])
            .await
            .context("read the sign-in rules")?
            .map(|r| Policy {
                code_required: r.get(0),
                idle_minutes: r.get(1),
                max_hours: r.get(2),
                lockout_enabled: r.get(3),
                lockout_failures: r.get(4),
                lockout_wait_minutes: r.get(5),
                lockout_max_wait_minutes: r.get(6),
                lockout_permanent: r.get(7),
            }))
    }

    /// The rules as they stand, read fresh: every password check starts
    /// here, so a changed rule bites on the next attempt.
    async fn fresh_policy(&self, c: &Client) -> Policy {
        match Self::read_policy(c).await.ok().flatten() {
            Some(p) => {
                *self.policy.lock().unwrap_or_else(|e| e.into_inner()) = (Instant::now(), p);
                p
            }
            None => self.policy.lock().unwrap_or_else(|e| e.into_inner()).1,
        }
    }

    /// The cached rules, refreshing first when older than `LOCAL_TTL`:
    /// for per-request session checks, which must not read the store
    /// twice on every request.
    async fn ttl_policy(&self, c: &Client) -> Policy {
        if self.policy.lock().unwrap_or_else(|e| e.into_inner()).0.elapsed() > LOCAL_TTL {
            return self.fresh_policy(c).await;
        }
        self.policy.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    /// The sign-in rules, read fresh.
    pub async fn policy(&self) -> Result<Policy> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        Ok(self.fresh_policy(c).await)
    }

    /// Set the sign-in rules whole: the caller checked the ranges.
    pub async fn set_policy(&self, policy: Policy) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        c.execute(
            "INSERT INTO public.account_sign_in_policy (id, code_required, idle_minutes, max_hours, lockout_enabled, lockout_failures, lockout_wait_minutes, lockout_max_wait_minutes, lockout_permanent)
             VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET code_required = EXCLUDED.code_required, idle_minutes = EXCLUDED.idle_minutes, max_hours = EXCLUDED.max_hours,
               lockout_enabled = EXCLUDED.lockout_enabled, lockout_failures = EXCLUDED.lockout_failures, lockout_wait_minutes = EXCLUDED.lockout_wait_minutes,
               lockout_max_wait_minutes = EXCLUDED.lockout_max_wait_minutes, lockout_permanent = EXCLUDED.lockout_permanent",
            &[&policy.code_required, &policy.idle_minutes, &policy.max_hours, &policy.lockout_enabled, &policy.lockout_failures, &policy.lockout_wait_minutes, &policy.lockout_max_wait_minutes, &policy.lockout_permanent],
        )
        .await?;
        *self.policy.lock().unwrap_or_else(|e| e.into_inner()) = (Instant::now(), policy);
        self.refresh_local(c).await?;
        Ok(())
    }

    async fn refresh_local(&self, c: &Client) -> Result<()> {
        let map = Self::read_local(c).await?;
        *self.local.lock().unwrap_or_else(|e| e.into_inner()) = (Instant::now(), map);
        Ok(())
    }

    async fn read_local(c: &Client) -> Result<BTreeMap<String, LocalPerson>> {
        let mut map = BTreeMap::new();
        for r in c.query("SELECT person, name, email FROM public.account_people ORDER BY person", &[]).await.context("read the directory")? {
            let person: String = r.get("person");
            map.insert(person, LocalPerson { name: r.get("name"), email: r.get("email") });
        }
        Ok(map)
    }

    /// Whether the person has a password set.
    pub async fn has_password(&self, person: &str) -> bool {
        credentials::Credentials::password(self, &key(person)).await.ok().flatten().is_some()
    }

    /// Whether the person enrolled an authenticator app.
    pub async fn has_totp(&self, person: &str) -> bool {
        let person = key(person);
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return false,
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return false,
        };
        c.query_opt("SELECT 1 FROM public.account_totp WHERE person = $1", &[&person]).await.map(|r| r.is_some()).unwrap_or(false)
    }

    /// Whether the person may sign in: active unless recorded otherwise.
    pub async fn status(&self, person: &str) -> Status {
        let person = key(person);
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return Status::Active,
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return Status::Active,
        };
        let _ = self.fresh_local(c).await;
        c.query_opt("SELECT status FROM public.account_statuses WHERE person = $1", &[&person])
            .await
            .ok()
            .flatten()
            .map(|r| Status::parse(r.get::<_, String>(0).as_str()))
            .unwrap_or(Status::Active)
    }

    /// Turn a person on or off. Disabling ends their live sessions at once
    /// and returns the ids of those ended, for the caller to record.
    /// Refuses to disable the organisation's last enabled admin.
    pub async fn set_status(&self, person: &str, status: Status) -> Result<Vec<String>> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        if status == Status::Disabled && Self::sole_enabled_admin_tx(&tx, &person).await? {
            anyhow::bail!("This is the organisation's last admin: make someone else an admin first.");
        }
        tx.execute(
            "INSERT INTO public.account_statuses (person, status) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET status = EXCLUDED.status",
            &[&person, &status.as_str()],
        )
        .await?;
        let mut ended = vec![];
        if status == Status::Disabled {
            for r in tx.query("DELETE FROM public.account_sessions WHERE person = $1 RETURNING id", &[&person]).await? {
                ended.push(r.get("id"));
            }
        }
        tx.commit().await?;
        self.refresh_local(c).await?;
        self.clear_counters(&person);
        Ok(ended)
    }

    /// What the person may administer: member unless recorded an admin.
    pub async fn role(&self, person: &str) -> Role {
        let person = key(person);
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return Role::Member,
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return Role::Member,
        };
        let _ = self.fresh_local(c).await;
        c.query_opt("SELECT role FROM public.account_roles WHERE person = $1", &[&person])
            .await
            .ok()
            .flatten()
            .map(|r| Role::parse(r.get::<_, String>(0).as_str()))
            .unwrap_or(Role::Member)
    }

    /// Whether the appliance itself keeps this person (as opposed to the
    /// config or an outside directory). From the directory cache.
    pub fn is_local(&self, person: &str) -> bool {
        let person = key(person);
        self.local.lock().unwrap_or_else(|e| e.into_inner()).1.contains_key(person.as_str())
    }

    /// `is_local`, read from the store instead of the cache: for the
    /// sign-in routing decision, which must see a person the CLI added
    /// seconds ago, not the cache from before (#466).
    pub async fn is_local_fresh(&self, person: &str) -> bool {
        let mut g = match self.client().await {
            Ok(g) => g,
            Err(_) => return self.is_local(person),
        };
        let c = match g.as_mut() {
            Some(c) => c,
            None => return self.is_local(person),
        };
        let _ = self.fresh_local(c).await;
        c.query_opt("SELECT 1 FROM public.account_people WHERE person = $1", &[&person]).await.map(|r| r.is_some()).unwrap_or_else(|_| self.is_local(person))
    }

    /// The appliance's record for a local person, if there is one. From
    /// the directory cache.
    pub fn local_person(&self, person: &str) -> Option<LocalPerson> {
        let person = key(person);
        self.local.lock().unwrap_or_else(|e| e.into_inner()).1.get(person.as_str()).cloned()
    }

    /// Every local person by id. From the directory cache.
    pub fn local_people(&self) -> Vec<(String, LocalPerson)> {
        self.local.lock().unwrap_or_else(|e| e.into_inner()).1.clone().into_iter().collect()
    }

    /// Refresh the directory cache from the store. Sign-in calls this
    /// before resolving the person, so someone the CLI added seconds ago
    /// resolves (#466).
    pub async fn refresh_people(&self) -> Result<()> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        self.refresh_local(c).await
    }

    /// Remove a local person and every local trace: the record, password,
    /// authenticator, used codes, status, role, invites, recovery tokens
    /// and sessions. Refuses an unknown id, and the organisation's last
    /// enabled admin. The sessions die in the store, so no handle keeps
    /// them (#469); the recovery token dies too, so a link mailed before
    /// the removal cannot take over a recreated id (#541).
    pub async fn remove_person(&self, person: &str) -> Result<Vec<String>> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        let n = tx.execute("DELETE FROM public.account_people WHERE person = $1", &[&person]).await?;
        anyhow::ensure!(n == 1, "{person} is not a person here");
        if Self::sole_enabled_admin_tx(&tx, &person).await? {
            anyhow::bail!("This is the organisation's last admin: make someone else an admin first.");
        }
        for table in ["account_passwords", "account_totp", "account_totp_steps", "account_statuses", "account_roles", "account_reset_tokens", "account_invites"] {
            tx.execute(&format!("DELETE FROM public.{table} WHERE person = $1"), &[&person]).await?;
        }
        let mut ended = vec![];
        for r in tx.query("DELETE FROM public.account_sessions WHERE person = $1 RETURNING id", &[&person]).await? {
            ended.push(r.get("id"));
        }
        tx.commit().await?;
        self.clear_counters(&person);
        self.refresh_local(c).await?;
        Ok(ended)
    }

    /// Set a person's role. Refuses to demote the organisation's last
    /// enabled admin.
    pub async fn set_role(&self, person: &str, role: Role) -> Result<()> {
        let person = key(person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        if role == Role::Member && Self::sole_enabled_admin_tx(&tx, &person).await? {
            anyhow::bail!("This is the organisation's last admin: make someone else an admin first.");
        }
        tx.execute("INSERT INTO public.account_roles (person, role) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET role = EXCLUDED.role", &[&person, &role.as_str()]).await?;
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(())
    }

    /// Set a person's password. A password completes an invitation by any
    /// path, so someone invited becomes active; anyone disabled stays
    /// disabled until explicitly turned back on.
    pub async fn set_password(&self, person: &str, password: &str) -> Result<()> {
        let person = key(person);
        phc::long_enough(password, self.cfg.min_password_length)?;
        let phc = phc::hash(password)?;
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        tx.execute(
            "INSERT INTO public.account_passwords (person, phc, temporary) VALUES ($1, $2, false) ON CONFLICT (person) DO UPDATE SET phc = EXCLUDED.phc, temporary = false",
            &[&person, &phc],
        )
        .await?;
        tx.execute("UPDATE public.account_statuses SET status = 'active' WHERE person = $1 AND status = 'invited'", &[&person]).await?;
        // A finished invitation spends its token: accepting afterwards
        // must not reset the new password (#527).
        tx.execute("DELETE FROM public.account_invites WHERE person = $1", &[&person]).await?;
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(())
    }

    /// Give an invited person a password to choose with once, for when no
    /// invite link can reach them: it is temporary, so it never signs in
    /// (the check answers `MustChoose`), and they stay invited until
    /// they choose their own. Anyone else keeps their password.
    pub async fn set_temporary_password(&self, person: &str, password: &str) -> Result<()> {
        let person = key(person);
        phc::long_enough(password, self.cfg.min_password_length)?;
        let phc = phc::hash(password)?;
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let n = c.execute("INSERT INTO public.account_passwords (person, phc, temporary) VALUES ($1, $2, true) ON CONFLICT (person) DO UPDATE SET phc = EXCLUDED.phc, temporary = true WHERE (SELECT status FROM public.account_statuses WHERE person = $1) IS NULL OR (SELECT status FROM public.account_statuses WHERE person = $1) = 'invited'", &[&person, &phc]).await?;
        anyhow::ensure!(n == 1, "{person} is not an invitation waiting for its password");
        self.refresh_local(c).await?;
        Ok(())
    }

    /// Choose the permanent password with a temporary one: the temp plus
    /// the new one, together. A wrong temp counts on the counter for `via`,
    /// like a wrong password; choosing activates, as accepting does.
    pub async fn choose_password(&self, person: &str, temp: &str, password: &str, via: Via) -> Result<std::result::Result<(), SignInError>> {
        let person = key(person);
        let counter = key(&check::counter_of(via, &person));
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let _ = self.fresh_policy(c).await;
        if let Some(until) = self.locked(&counter) {
            self.failed(&counter);
            return Ok(Err(SignInError::Locked(self.locked(&counter).unwrap_or(until))));
        }
        let row: Option<(String, bool)> = c.query_opt("SELECT phc, temporary FROM public.account_passwords WHERE person = $1", &[&person]).await?.map(|r| (r.get(0), r.get(1)));
        // Unknown names refuse uncounted, so guessing them cannot grow
        // the table; a held password counts, temporary or not.
        let Some((phc, temporary)) = row else { return Ok(Err(SignInError::Rejected)) };
        if !temporary || !Self::password_ok(Some(&phc), temp) {
            self.failed(&counter);
            return Ok(Err(SignInError::Rejected));
        }
        phc::long_enough(password, self.cfg.min_password_length)?;
        let phc = phc::hash(password)?;
        let tx = c.transaction().await?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts'))").await?;
        // Still temporary: a racing choose won, and this temp is spent.
        let n = tx.execute("UPDATE public.account_passwords SET phc = $2, temporary = false WHERE person = $1 AND temporary", &[&person, &phc]).await?;
        if n != 1 {
            tx.rollback().await?;
            self.failed(&counter);
            return Ok(Err(SignInError::Rejected));
        }
        tx.execute("UPDATE public.account_statuses SET status = 'active' WHERE person = $1 AND status = 'invited'", &[&person]).await?;
        tx.execute("DELETE FROM public.account_invites WHERE person = $1", &[&person]).await?;
        tx.commit().await?;
        self.succeeded(&counter);
        self.refresh_local(c).await?;
        Ok(Ok(()))
    }

    /// Enroll a person's authenticator app: a fresh 160-bit secret is
    /// stored, and the `otpauth://` URI to enroll is returned.
    /// Re-enrolling replaces the secret.
    pub async fn enroll_totp(&self, person: &str) -> Result<String> {
        let person = key(person);
        let mut raw = [0u8; 20];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let secret = crate::totp::base32_encode(&raw);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        c.execute("INSERT INTO public.account_totp (person, secret) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET secret = EXCLUDED.secret", &[&person, &secret]).await?;
        self.refresh_local(c).await?;
        let label = percent_encoding::utf8_percent_encode(&person, percent_encoding::NON_ALPHANUMERIC);
        Ok(format!("otpauth://totp/Magnus:{label}?secret={}&issuer=Magnus&digits=6&period=30", secret.trim_end_matches('=')))
    }

    /// Begin self-service authenticator enrollment without overwriting an
    /// active secret until a 6-digit code confirms it.
    pub fn begin_totp(&self, person: &str) -> (String, String) {
        let person = key(person);
        let mut raw = [0u8; 20];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let secret = crate::totp::base32_encode(&raw);
        let clean = secret.trim_end_matches('=').to_string();
        let label = percent_encoding::utf8_percent_encode(&person, percent_encoding::NON_ALPHANUMERIC);
        let uri = format!("otpauth://totp/Magnus:{label}?secret={clean}&issuer=Magnus&digits=6&period=30");
        self.pending_totp.lock().unwrap_or_else(|e| e.into_inner()).insert(person, secret);
        (clean, uri)
    }

    /// Confirm a pending self-service authenticator enrollment with a 6-digit code.
    pub async fn confirm_totp(&self, person: &str, code: &str) -> Result<bool> {
        let person = key(person);
        let secret = self.pending_totp.lock().unwrap_or_else(|e| e.into_inner()).get(&person).cloned();
        let Some(secret) = secret else { return Ok(false) };
        let Some(step) = crate::totp::verify_step(&secret, code, Utc::now()) else { return Ok(false) };
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        tx.execute("INSERT INTO public.account_totp (person, secret) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET secret = EXCLUDED.secret", &[&person, &secret]).await?;
        tx.execute("INSERT INTO public.account_totp_steps (person, step) VALUES ($1, $2) ON CONFLICT (person) DO UPDATE SET step = EXCLUDED.step", &[&person, &step]).await?;
        tx.commit().await?;
        self.pending_totp.lock().unwrap_or_else(|e| e.into_inner()).remove(&person);
        self.refresh_local(c).await?;
        Ok(true)
    }

    /// Remove a person's enrolled authenticator secret.
    pub async fn remove_totp(&self, person: &str) -> Result<bool> {
        let person = key(person);
        self.pending_totp.lock().unwrap_or_else(|e| e.into_inner()).remove(&person);
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let tx = c.transaction().await?;
        let n = tx.execute("DELETE FROM public.account_totp WHERE person = $1", &[&person]).await?;
        tx.execute("DELETE FROM public.account_totp_steps WHERE person = $1", &[&person]).await?;
        tx.commit().await?;
        self.refresh_local(c).await?;
        Ok(n > 0)
    }

    /// How many active local people have not yet enrolled an authenticator app.
    pub async fn unenrolled_totp_count(&self) -> Result<i64> {
        let mut g = self.client().await?;
        let c = g.as_mut().context("accounts connection")?;
        let row = c
            .query_one(
                "SELECT COUNT(*) FROM public.account_people p
                 LEFT JOIN public.account_statuses s ON s.person = p.person
                 LEFT JOIN public.account_totp t ON t.person = p.person
                 WHERE (s.status IS NULL OR s.status = 'active') AND t.person IS NULL",
                &[],
            )
            .await?;
        Ok(row.get(0))
    }
}

#[cfg(test)]
mod tests;
