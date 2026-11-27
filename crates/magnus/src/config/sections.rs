//! Config subsections (sign-in, client certs, backup, trash, schedules, proactive, updates).

use serde::Deserialize;

/// Sign-in for people's apps. Tunable values, not decisions (K51).
#[derive(Deserialize, Clone, Debug)]
pub struct SignIn {
    #[serde(default = "session_days")]
    pub session_days: u32,
    #[serde(default = "max_failures")]
    pub max_failures: u32,
    #[serde(default = "lockout_minutes")]
    pub lockout_minutes: u32,
    #[serde(default = "min_password_length")]
    pub min_password_length: usize,
    /// Sign in through an identity provider instead of passwords.
    #[serde(default)]
    pub oidc: Option<Oidc>,
}

impl Default for SignIn {
    fn default() -> Self {
        SignIn { session_days: session_days(), max_failures: max_failures(), lockout_minutes: lockout_minutes(), min_password_length: min_password_length(), oidc: None }
    }
}

/// An OpenID Connect provider (Keycloak, for one). People approve the sign-in on its own page
/// (the device-code grant, RFC 8628); its endpoints come from its discovery document.
#[derive(Deserialize, Clone, Debug)]
pub struct Oidc {
    /// For example `https://identity.example.com/realms/magnus`.
    pub issuer: String,
    /// A public client with the device-code grant turned on.
    pub client_id: String,
    /// The ID token claim that holds a person's id on this appliance.
    #[serde(default = "person_claim")]
    pub person_claim: String,
    #[serde(default = "oidc_scope")]
    pub scope: String,
    /// What the sign-in button names; the issuer's host when unset.
    #[serde(default)]
    pub label: Option<String>,
    /// Sign-ins waiting for approval at once. Starting one needs no
    /// credential, so this bounds what strangers can make the appliance hold.
    #[serde(default = "max_pending")]
    pub max_pending: usize,
    /// Where the ID token carries the person's groups (a JSON pointer, for
    /// Keycloak `/groups`). When set, the provider's groups replace the
    /// config's at each sign-in (E2).
    #[serde(default)]
    pub groups_claim: Option<String>,
    /// Where it carries their roles (Keycloak realm roles: `/realm_access/roles`).
    #[serde(default)]
    pub roles_claim: Option<String>,
    /// How long a sign-in in the person's browser may take: finding a
    /// password or passkey, two-factor. The app waits as long, from what the
    /// appliance tells it.
    #[serde(default = "sign_in_minutes")]
    pub sign_in_minutes: u32,
    /// A confidential client whose service account may only view users
    /// (Keycloak: `realm-management` → `view-users`). The appliance asks it
    /// who uses a code from an authenticator app, since the password grant
    /// answers the same to a wrong password and a missing code.
    #[serde(default)]
    pub status_client: Option<String>,
    /// The vault secret holding that client's secret.
    #[serde(default)]
    pub status_credential: Option<String>,
    /// Where Keycloak's admin API answers, when not at the issuer's host: a
    /// server can keep it on an address of its own (`hostname-admin`), such
    /// as `https://127.0.0.1:19446`. Unset: the issuer's host.
    #[serde(default)]
    pub status_admin_url: Option<String>,
    /// A PEM certificate authority to trust for that address, when a private one issued its certificate.
    #[serde(default)]
    pub status_admin_ca: Option<std::path::PathBuf>,
}

fn person_claim() -> String {
    "preferred_username".into()
}

fn oidc_scope() -> String {
    "openid".into()
}
fn max_pending() -> usize {
    100
}
fn sign_in_minutes() -> u32 {
    10
}

fn session_days() -> u32 {
    30
}
fn max_failures() -> u32 {
    5
}
fn lockout_minutes() -> u32 {
    15
}
fn min_password_length() -> usize {
    10
}

/// Cloudflare's managed CA issues one certificate per paired computer.
/// The token in the vault is zone-scoped and can write client certificates;
/// it is not an all-accounts token.
#[derive(Deserialize, Clone, Debug)]
pub struct ClientCerts {
    /// The public hostname, without a scheme: `magnus.pyrallon.ai`.
    pub hostname: String,
    /// The zone id of that hostname. Not a secret; it is in the dashboard.
    pub zone_id: String,
    /// The vault secret whose text is the Cloudflare API token.
    #[serde(default = "client_cert_credential")]
    pub credential: String,
    /// How long a certificate is valid. Cloudflare accepts 1 to 3650 days.
    #[serde(default = "client_cert_days")]
    pub days: u32,
}

fn client_cert_credential() -> String {
    "cloudflare_mtls".into()
}
fn client_cert_days() -> u32 {
    365
}

/// Backups of the event log and the vault (K10).
#[derive(Deserialize, Clone, Debug)]
pub struct Backup {
    /// Where backups are written; `<state_dir>/backups` unless set. Set it to another disk: a backup on the appliance's own disk is lost with it.
    #[serde(default)]
    pub dir: Option<std::path::PathBuf>,
    /// The key backups are encrypted with; `<state_dir>/backup.key` unless
    /// set, made on the first backup. Keep a copy off the appliance.
    #[serde(default)]
    pub key_file: Option<std::path::PathBuf>,
    /// Hours between scheduled backups; 0 turns the schedule off.
    #[serde(default = "every_hours")]
    pub every_hours: u32,
    /// Days a backup is kept: the record keeps key-store backups no longer
    /// than the legal deletion deadline, and the GDPR gives a month to act
    /// on an erasure request (Art. 12(3)), so a deleted secret leaves every
    /// backup within it.
    #[serde(default = "keep_days")]
    pub keep_days: u32,
    /// The other Postgres databases the appliance depends on (its identity
    /// provider's, #296), as connection URLs: each backup holds a dump of
    /// each, and `magnus restore` puts them back.
    #[serde(default)]
    pub databases: Vec<String>,
    /// Seconds to wait when the scheduled wait cannot be converted to a
    /// sleep (#289): a floor, never the plan, so a minute.
    #[serde(default = "fallback_wait_secs")]
    pub fallback_wait_secs: u32,
    /// Seconds after a failed scheduled backup before trying again (#289):
    /// an hour, so a transient fault (a full disk) can clear.
    #[serde(default = "retry_wait_secs")]
    pub retry_wait_secs: u32,
    /// Where each backup is mirrored after it is made and old ones are
    /// dropped (#297): an rsync destination, a path or `host:path`, from
    /// configuration only. None configured, no off-host copy.
    #[serde(default)]
    pub copy_to: Option<String>,
    /// Seconds an off-host copy may take before it is stopped and recorded
    /// as failed (#297).
    #[serde(default = "copy_timeout_secs")]
    pub copy_timeout_secs: u32,
}

/// Daily: with a month kept, a month of daily restore points.
fn every_hours() -> u32 {
    24
}
fn keep_days() -> u32 {
    30
}
fn fallback_wait_secs() -> u32 {
    60
}
fn retry_wait_secs() -> u32 {
    3600
}
fn copy_timeout_secs() -> u32 {
    300
}

impl Default for Backup {
    fn default() -> Self {
        Backup {
            dir: None,
            key_file: None,
            every_hours: every_hours(),
            keep_days: keep_days(),
            databases: Vec::new(),
            fallback_wait_secs: fallback_wait_secs(),
            retry_wait_secs: retry_wait_secs(),
            copy_to: None,
            copy_timeout_secs: copy_timeout_secs(),
        }
    }
}

/// Deleted items wait in the trash, restorable, for `keep_days`, then their
/// content is removed. A month, as backups are kept: the same date answers
/// "when is it gone?", within the GDPR's month to act on erasure.
#[derive(Deserialize, Clone, Debug)]
pub struct Trash {
    #[serde(default = "keep_days")]
    pub keep_days: u32,
}

impl Default for Trash {
    fn default() -> Self {
        Trash { keep_days: keep_days() }
    }
}

/// Recurring tasks.
#[derive(Deserialize, Clone, Debug)]
pub struct Schedules {
    /// A schedule pauses itself, and tells its person, after this many of its runs fail in a row (or cannot start), so a broken one stops spending.
    #[serde(default = "pause_after_failures")]
    pub pause_after_failures: u32,
    /// The shortest interval a schedule may repeat at.
    #[serde(default = "min_every_minutes")]
    pub min_every_minutes: u32,
}

impl Default for Schedules {
    fn default() -> Self {
        Schedules { pause_after_failures: pause_after_failures(), min_every_minutes: min_every_minutes() }
    }
}

fn pause_after_failures() -> u32 {
    3
}

fn min_every_minutes() -> u32 {
    5
}

/// What Magnus proposes by itself (behavior spec, Suggestions and proposals).
#[derive(Deserialize, Clone, Debug)]
pub struct Proposals {
    /// At a person's cold start, and when they connect an app for the first
    /// time, Magnus opens a conversation and proposes what it could take
    /// over (`openers.rs`). On unless set to false.
    #[serde(default = "openers")]
    pub openers: bool,
}

impl Default for Proposals {
    fn default() -> Self {
        Proposals { openers: openers() }
    }
}

fn proactive_on() -> bool {
    true
}

fn review_every_minutes() -> u32 {
    60
}

fn interruptions_per_day() -> u32 {
    3
}

/// The proactive loop's operating parameters (behavior step 7, #237):
/// how often reviews wake and how many interruptions a day they may
/// deliver. People override the cadence and the interruption budget with
/// preferences; `on` is the deployment kill switch. A wake spends from the
/// per-task `[budget]` like any task (spec rev 133); the retired
/// `wake_max_usd` key is ignored where a config still sets it.
#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct Proactive {
    #[serde(default = "proactive_on")]
    pub on: bool,
    #[serde(default = "review_every_minutes")]
    pub review_every_minutes: u32,
    #[serde(default = "interruptions_per_day")]
    pub interruptions_per_day: u32,
    /// How long new items gather before one review reads them (`wake_budget::ARRIVAL_GATHER_MINUTES` unless set).
    #[serde(default)]
    pub arrival_gather_minutes: Option<u32>,
}

impl Default for Proactive {
    fn default() -> Self {
        Proactive { on: proactive_on(), review_every_minutes: review_every_minutes(), interruptions_per_day: interruptions_per_day(), arrival_gather_minutes: None }
    }
}

fn openers() -> bool {
    true
}

/// Signed updates of the appliance and of the desktop app (gate item 5).
#[derive(Deserialize, Clone, Debug)]
pub struct Updates {
    /// Where release packages arrive (`appliance/` and `desktop/`, each with
    /// its `latest.json`); `<state_dir>/updates` unless set. The appliance
    /// installs its own from here and serves the desktop app's to the apps.
    #[serde(default)]
    pub dir: Option<std::path::PathBuf>,
    /// The release key (minisign, as `tauri signer` writes it); packages
    /// signed by anything else are refused. The project's key unless set.
    #[serde(default = "release_key")]
    pub public_key: String,
    /// Install a newer appliance release as soon as it is found.
    #[serde(default = "yes")]
    pub automatic: bool,
    /// Minutes between looks for a new release; 0 looks only when asked.
    #[serde(default = "check_minutes")]
    pub check_minutes: u32,
}

fn release_key() -> String {
    include_str!("../../../../release/update.pub").trim().to_string()
}
fn yes() -> bool {
    true
}
/// Hourly: a look reads one local file, and a release reaches the
/// appliance within the hour it is published.
fn check_minutes() -> u32 {
    60
}

impl Default for Updates {
    fn default() -> Self {
        Updates { dir: None, public_key: release_key(), automatic: true, check_minutes: check_minutes() }
    }
}
