//! The appliance configuration file (TOML), written by the installer.

use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub use crate::mail_config::Mail;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub appliance: Appliance,
    /// `[admin]`, the retired console's (spec rev 141): still read, so a deployed config loads, and ignored.
    #[serde(default)]
    pub admin: Option<toml::Value>,
    pub database: Database,
    pub model: Model,
    #[serde(default)]
    pub adapters: Vec<Adapter>,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub verifier: Verifier,
    #[serde(default)]
    pub reviewer: Reviewer,
    #[serde(default)]
    pub auto_review: crate::auto_review::Settings,
    #[serde(default)]
    pub safety: crate::safety::Settings,
    #[serde(default)]
    pub images: Option<crate::ui::images::Settings>,
    #[serde(default)]
    pub local: crate::local::Settings,
    #[serde(default)]
    pub backup: Backup,
    #[serde(default)]
    pub trash: Trash,
    #[serde(default)]
    pub schedules: Schedules,
    #[serde(default)]
    pub proposals: Proposals,
    #[serde(default)]
    pub updates: Updates,
    #[serde(default)]
    pub proactive: Proactive,
    #[serde(default)]
    pub outcomes: crate::outcomes::Settings,
    /// The file this was read from: a new release is checked against it.
    #[serde(skip)]
    pub path: PathBuf,
    /// Removable modules, on or off for every model, and per model.
    #[serde(default)]
    pub modules: Modules,
    #[serde(default)]
    pub web: Web,
    #[serde(default)]
    pub sandbox: Option<Sandbox>,
    /// Brokered connectors' microVMs, booted with the sandbox's kernel.
    #[serde(default)]
    pub brokered: Brokered,
    #[serde(default)]
    pub browser: Option<Browser>,
    #[serde(default)]
    pub computer: Option<crate::computer::Settings>,
    #[serde(default)]
    pub payments: Option<crate::payments::Settings>,
    /// Local speech to text for voice input (`config/install-whisper.sh`).
    #[serde(default)]
    pub speech: Option<Speech>,
    /// The read-aloud voice, run on the appliance (`config/install-voice.sh`).
    #[serde(default)]
    pub read_aloud: Option<ReadAloud>,
    #[serde(default)]
    pub knowledge: Knowledge,
    #[serde(default)]
    pub people: Vec<Person>,
    #[serde(default)]
    pub connectors: Vec<Connector>,
    #[serde(default)]
    pub sign_in: SignIn,
    /// Client certificates for the public hostname. None: no device pairing.
    #[serde(default)]
    pub client_certs: Option<ClientCerts>,
    #[serde(default)]
    pub observability: Observability,
    /// How account emails are sent (recovery, invitations). None: they are not.
    #[serde(default)]
    pub mail: Option<Mail>,
    /// The facts the agreements state about whoever runs the server; all required.
    #[serde(default)]
    pub legal: crate::legal::Settings,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Observability {
    /// Developer view (K51): a running run with no new event for this long counts as stalled (V3).
    #[serde(default = "stall_minutes")]
    pub stall_minutes: u32,
}

impl Default for Observability {
    fn default() -> Self {
        Observability { stall_minutes: stall_minutes() }
    }
}

fn stall_minutes() -> u32 {
    15
}

#[derive(Deserialize, Clone, Debug)]
pub struct Appliance {
    pub state_dir: PathBuf,
    /// Plain HTTP. Passwords are refused here except from this machine.
    pub listen: String,
    /// HTTPS for people's apps on the network. Without `tls_cert` and
    /// `tls_key`, the appliance makes its own certificate on first start and
    /// apps pin it on first connection.
    #[serde(default)]
    pub tls_listen: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// The server's https address on the internet, where providers send notifications; unset, none are subscribed to.
    pub public_url: Option<crate::public_url::PublicUrl>,
}

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

/// An OpenID Connect provider (Keycloak, for one). People approve the
/// sign-in on its own page (the device-code grant, RFC 8628); its endpoints
/// come from its discovery document.
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
    /// A PEM certificate authority to trust for that address, when a
    /// private one issued its certificate.
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

#[derive(Deserialize, Clone, Debug)]
pub struct Database {
    /// The kernel's own role: INSERT and SELECT on the log, nothing else.
    pub url: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Model {
    pub adapter: String,
    pub name: String,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_retries() -> u32 {
    3
}

fn default_max_tokens() -> u32 {
    16000
}

#[derive(Deserialize, Clone, Debug)]
pub struct Adapter {
    pub id: String,
    pub command: Vec<String>,
    /// Provider settings passed to the adapter. `credential` names the vault
    /// secret the kernel resolves and passes along; it is not sent itself.
    #[serde(default)]
    pub settings: serde_json::Map<String, Value>,
}

#[derive(Deserialize, Clone, Debug, serde::Serialize)]
pub struct Budget {
    pub max_model_calls: u32,
    pub max_actions: u32,
    pub max_usd: f64,
}

impl Budget {
    /// Each limit the lower of the two.
    pub fn within(&self, cap: &Budget) -> Budget {
        Budget { max_model_calls: self.max_model_calls.min(cap.max_model_calls), max_actions: self.max_actions.min(cap.max_actions), max_usd: self.max_usd.min(cap.max_usd) }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Budget { max_model_calls: 300, max_actions: 500, max_usd: 50.0 }
    }
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
}

impl Default for Proactive {
    fn default() -> Self {
        Proactive { on: proactive_on(), review_every_minutes: review_every_minutes(), interruptions_per_day: interruptions_per_day() }
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
    include_str!("../../../release/update.pub").trim().to_string()
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

impl Config {
    pub fn updates_dir(&self) -> std::path::PathBuf {
        self.updates.dir.clone().unwrap_or_else(|| self.appliance.state_dir.join("updates"))
    }
    pub fn backup_dir(&self) -> std::path::PathBuf {
        self.backup.dir.clone().unwrap_or_else(|| self.appliance.state_dir.join("backups"))
    }
    pub fn backup_key_file(&self) -> std::path::PathBuf {
        self.backup.key_file.clone().unwrap_or_else(|| self.appliance.state_dir.join("backup.key"))
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Verifier {
    #[serde(default)]
    pub enabled: bool,
    /// Its model, `adapter/model`; the run's own when unset.
    #[serde(default)]
    pub model: Option<String>,
}

/// The send reviewer: off unless enabled, like the verifier. It judges
/// with the run's own model: a send it reads is already fully in context.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Reviewer {
    #[serde(default)]
    pub enabled: bool,
}

/// Removable modules (each compensates for a model weakness and is kept
/// only while its ablation shows it helps): the default for every model,
/// then per model (`adapter/model`). The verifier's and the reviewer's
/// defaults are `[verifier] enabled` and `[reviewer] enabled`.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Modules {
    #[serde(default)]
    pub lost_reset: Option<bool>,
    #[serde(default)]
    pub escalation: Option<bool>,
    #[serde(default)]
    pub memory_pass: Option<bool>,
    #[serde(default)]
    pub lost: crate::run::lost::Thresholds,
    #[serde(default)]
    pub model: Vec<ModelModules>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ModelModules {
    /// `adapter/model`.
    pub model: String,
    #[serde(default)]
    pub verifier: Option<bool>,
    #[serde(default)]
    pub lost_reset: Option<bool>,
    #[serde(default)]
    pub escalation: Option<bool>,
    #[serde(default)]
    pub memory_pass: Option<bool>,
    #[serde(default)]
    pub reviewer: Option<bool>,
    /// The stronger model a lost run escalates to, `adapter/model`.
    #[serde(default)]
    pub escalate_to: Option<String>,
}

impl Config {
    /// Each removable module on or off for a model, and where it escalates:
    /// per-model settings over the defaults. Lost-reset and escalation are
    /// on unless turned off: they act only on a run that is lost.
    pub fn modules_for(&self, adapter: &str, model: &str) -> (std::collections::BTreeMap<String, bool>, Option<String>) {
        let key = format!("{adapter}/{model}");
        let per = self.modules.model.iter().find(|m| m.model == key);
        let pick = |name: &str, default: bool| {
            let p = per.and_then(|m| match name {
                "verifier" => m.verifier,
                "lost_reset" => m.lost_reset,
                "memory_pass" => m.memory_pass,
                "reviewer" => m.reviewer,
                _ => m.escalation,
            });
            (name.to_string(), p.unwrap_or(default))
        };
        let on = [
            pick("verifier", self.verifier.enabled),
            pick("lost_reset", self.modules.lost_reset.unwrap_or(true)),
            pick("escalation", self.modules.escalation.unwrap_or(true)),
            pick("memory_pass", self.modules.memory_pass.unwrap_or(true)),
            pick("reviewer", self.reviewer.enabled),
        ]
        .into_iter()
        .collect();
        (on, per.and_then(|m| m.escalate_to.clone()))
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Web {
    /// Allow fetching loopback and private addresses. Off on a real appliance.
    #[serde(default)]
    pub allow_private_networks: bool,
    /// Optional search endpoint (SearXNG JSON API).
    #[serde(default)]
    pub search_url: Option<String>,
}

pub use crate::browser::Settings as Browser;

#[derive(Deserialize, Clone, Debug)]
pub struct Speech {
    /// whisper.cpp's `whisper-cli`.
    pub whisper: PathBuf,
    /// A ggml model file.
    pub model: PathBuf,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ReadAloud {
    /// The `speak` program: text on stdin, MP3 on stdout.
    pub speak: PathBuf,
    /// Kokoro's model and voices files.
    pub model: PathBuf,
    pub voices: PathBuf,
    /// One of Kokoro's voices, such as `af_heart`.
    pub voice: String,
    /// Seconds one piece may take before it is stopped.
    #[serde(default = "speak_timeout")]
    pub timeout_s: u64,
}

fn speak_timeout() -> u64 {
    60
}

#[derive(Deserialize, Clone, Debug)]
pub struct Sandbox {
    pub firecracker: PathBuf,
    pub kernel_image: PathBuf,
    pub rootfs: PathBuf,
    #[serde(default)]
    pub jailer: Option<PathBuf>,
    #[serde(default = "one")]
    pub vcpus: u32,
    #[serde(default = "mem")]
    pub memory_mib: u32,
    #[serde(default = "disk")]
    pub workspace_mib: u32,
}

fn one() -> u32 {
    1
}
fn mem() -> u32 {
    1024
}
fn disk() -> u32 {
    2048
}

/// Brokered connectors (connectors spec, Brokered API connectors): each
/// person's instance of one is a microVM booted from its server image.
#[derive(Deserialize, Clone, Debug)]
pub struct Brokered {
    /// Where the server images are, each named by its digest:
    /// `<sha256 hex>.cpio`.
    #[serde(default)]
    pub images: Option<PathBuf>,
    /// Seconds an instance waits unused before it is stopped ("stopped
    /// when idle").
    #[serde(default = "brokered_idle")]
    pub idle_s: u64,
    #[serde(default = "brokered_memory")]
    pub memory_mib: u32,
    /// Where the egress proxy reaches a manifest host (`host:port`) instead
    /// of the address DNS gives: a pinned address, or a stand-in for the
    /// provider in a test. What a microVM may ask for is still only its
    /// manifest's hosts.
    #[serde(default)]
    pub resolve: std::collections::BTreeMap<String, String>,
    /// Certificates (DER files) servers trust beside the public roots: an
    /// organisation whose egress inspects TLS, or a test's stand-in.
    #[serde(default)]
    pub roots: Vec<PathBuf>,
}

/// Five minutes: the steps of one run call an app minutes apart at most
/// (each waits on a model's answer), so a run keeps its instance, which
/// boots in about a second, while a person who has stopped working holds
/// no microVM for long.
fn brokered_idle() -> u64 {
    300
}

/// The image is unpacked into the microVM's memory and is a few megabytes;
/// the rest is for what one call carries (a message with its attachments).
fn brokered_memory() -> u32 {
    128
}

impl Default for Brokered {
    fn default() -> Self {
        Brokered { images: None, idle_s: brokered_idle(), memory_mib: brokered_memory(), resolve: Default::default(), roots: vec![] }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Person {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// An API token for scripts; people's apps sign in (provider or password) instead.
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct Connector {
    /// Takes login codes and sign-in links out of what this app returns.
    #[serde(default)]
    pub filter_login_codes: bool,
    #[serde(default)]
    pub owner: Option<String>, // The one person a generated connector belongs to.
    pub name: String,
    #[serde(default = "mcp_http")]
    pub transport: String,
    pub url: String,
    #[serde(default)]
    pub credential: Option<String>,
    pub audience: String,
    pub label: String,
    #[serde(default)]
    pub access_groups: Vec<String>,
    /// Groups blocked from it, whatever `access_groups` says (connectors spec).
    #[serde(default)]
    pub blocked_groups: Vec<String>,
    #[serde(default = "header")]
    pub idempotency: String,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolClass>,
    /// `oauth`: the person signs in to the app (MCP authorization); the
    /// credential holds the tokens. Otherwise the credential is a bearer token.
    #[serde(default)]
    pub auth: Option<String>,
    /// A client registered with the app by hand, for an app that does not
    /// register clients itself.
    #[serde(default)]
    pub client_id: Option<String>,
    /// The company's record of what it holds (an ERP, a CRM): a knowledge
    /// entry whose quote is found in its answer is verified.
    #[serde(default)]
    pub system_of_record: bool,
    /// The access asked for at sign-in, when the app's manifest names it
    /// (else what the app's metadata offers).
    #[serde(default)]
    pub scopes: Vec<String>,
    /// The provider whose registration (Alpha Vector's client, set by the
    /// owner) signs people in to it.
    #[serde(default)]
    pub registration: Option<String>,
    /// Its name and one sentence on what Magnus can do with it, from its
    /// manifest.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// From the catalog, not added by the owner.
    #[serde(default)]
    pub catalog: bool,
    /// A brokered connector (`transport = "brokered"`, `url` its server
    /// image's digest, `sha256:<hex>`): the provider API hosts its microVM
    /// may reach, `host` (port 443) or `host:port`, and the provider's
    /// authorization server people sign in at.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    /// The sign-in sends PKCE (S256). Off only for a confidential client
    /// whose provider rejects a code challenge until the app opts in, and
    /// whose opt-in cannot be undone.
    #[serde(default = "pkce_on")]
    pub pkce: bool,
    /// The one redirect the provider registered. The token exchange repeats it.
    #[serde(default)]
    pub redirect_uri: Option<String>,
    /// The loopback address the desktop listens on when `redirect_uri` is
    /// the public https callback.
    #[serde(default)]
    pub handoff: Option<String>,
    /// Where a tool's action goes, read from what it acts on, by tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub destinations: BTreeMap<String, Destination>,
    /// How the gate asks whether the person has mailed an address before
    /// (#210): a read tool called with a query naming the address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<SentSearch>,
    /// Watched reads (#369): when any run's call of `tool` answers,
    /// the items at `items` (a JSON pointer into the answer, `""` for
    /// the answer itself) are diffed by their `id` field against what
    /// was last seen, and new ones wake a review for the person.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<Watch>,
}

/// One watched read of an app (#369): `tool` names the remote tool,
/// `items` points at its answer's array, `id` names each item's
/// stable id. Items without an id are skipped, never waked on.
/// `poll_minutes`, when set, polls the read on schedule so items
/// nobody reads still wake (#406); unset watches diff only reads.
#[derive(Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct Watch {
    pub tool: String,
    pub items: String,
    pub id: String,
    #[serde(default)]
    pub poll_minutes: Option<u64>,
    /// A poll's arguments; `{now}`, `{now-1d}` read as times (`watch_reads`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// How the gate asks whether the person has mailed an address before, a
/// recipient-provenance source beside the person's own words (#210):
/// `read` (a read tool of the same app) is called with `input` set to
/// `query` with `{address}` replaced, through the person's own sign-in,
/// and the address counts as mailed when `found` (a JSON pointer into
/// what it returns) is not empty.
#[derive(Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct SentSearch {
    pub read: String,
    pub input: String,
    pub query: String,
    pub found: String,
}

impl SentSearch {
    /// What is wrong with this declaration, given each tool's risk class:
    /// it reads with a read tool of the same app, names its input, asks
    /// about the address, and points at where the answer says found.
    pub fn fault(&self, risk: impl Fn(&str) -> Option<String>) -> Option<String> {
        if risk(&self.read).as_deref() != Some("read") {
            return Some(format!("sent mail is searched with {}, which is not a read tool of the same app", self.read));
        }
        if self.input.is_empty() || self.query.is_empty() || self.found.is_empty() {
            return Some("sent mail names no input, query or found pointer".into());
        }
        if !self.query.contains("{address}") {
            return Some("sent mail's query does not ask about {address}".into());
        }
        if !self.found.starts_with('/') {
            return Some(format!("sent mail's {:?} is not a JSON pointer", self.found));
        }
        None
    }
}

/// Where an action goes, read at the gate from the object it acts on and
/// never taken from what the model wrote (approvals spec, Destination
/// fields): `read` is the app's read tool for that object, called with the
/// action's `id` field as it is; `recipients` and `content` point (JSON
/// pointers) into what it returns. The approval card shows them, and an
/// approval holds only for exactly what they were.
#[derive(Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct Destination {
    pub read: String,
    pub id: String,
    pub recipients: Vec<String>,
    #[serde(default)]
    pub content: Vec<String>,
    /// Fields the approval holds for but the card does not show: a digest
    /// of every part of the message, so a change to a part the card does
    /// not show (an HTML part, an attachment's bytes) asks again (#177).
    #[serde(default)]
    pub bound: Vec<String>,
}

impl Destination {
    /// What is wrong with this declaration for `tool`, given each tool's
    /// risk class: it is for a write, it reads with a read tool of the same
    /// app, and each pointer is one and names its own field.
    pub fn fault(&self, tool: &str, risk: impl Fn(&str) -> Option<String>) -> Option<String> {
        let r = risk(tool)?;
        if r == "read" {
            return Some(format!("{tool} is a read: it goes nowhere"));
        }
        if risk(&self.read).as_deref() != Some("read") {
            return Some(format!("{tool} reads its destinations with {}, which is not a read tool of the same app", self.read));
        }
        if self.id.is_empty() || self.recipients.is_empty() {
            return Some(format!("{tool} names no id field or no recipients"));
        }
        let mut names = std::collections::BTreeSet::new();
        for p in self.recipients.iter().chain(&self.content).chain(&self.bound) {
            let name = p.rsplit('/').next().unwrap_or_default();
            if !p.starts_with('/') || name.is_empty() || !names.insert(name) {
                return Some(format!("{tool}: {p:?} is not a JSON pointer ending in a field name of its own"));
            }
        }
        None
    }
}

fn pkce_on() -> bool {
    true
}

/// The company knowledge base. Without an embedding server it is searched
/// by its words (Postgres full-text search).
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Knowledge {
    #[serde(default)]
    pub embeddings: Option<Embeddings>,
}

/// An embedding server with the OpenAI-compatible `/embeddings` call (a
/// local one such as Ollama, llama.cpp or TEI keeps the text on site).
#[derive(Deserialize, Clone, Debug)]
pub struct Embeddings {
    /// The API's base address, e.g. `http://127.0.0.1:11434/v1`.
    pub url: String,
    pub model: String,
    /// The vault secret holding its API key, if it needs one.
    #[serde(default)]
    pub credential: Option<String>,
    /// Seconds a request may take before the server counts as failed and
    /// search falls back to its words.
    #[serde(default = "embed_timeout")]
    pub timeout_s: u64,
}

/// As long as the identity provider's requests may take.
fn embed_timeout() -> u64 {
    30
}

fn mcp_http() -> String {
    "mcp_http".into()
}
/// Apps are assumed not to deduplicate unless the owner says they honor an
/// `Idempotency-Key` header, so a crash never causes a blind resend.
fn header() -> String {
    "none".into()
}

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct ToolClass {
    pub risk: String,
    /// Overrides the app's inbound label for this tool's results.
    #[serde(default)]
    pub label: Option<String>,
    /// What the tool reaches: `self` for the person's own account
    /// (`tools::REACH_SELF`); absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<String>,
}

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        cfg.path = path.canonicalize()?;
        cfg.outcomes.check()?;
        cfg.legal.check()?;
        if cfg.admin.is_some() {
            tracing::warn!("{}: [admin] configured the retired admin console and is ignored; administration is in the app", path.display());
        }
        cfg.images.as_ref().map(|i| i.check(&cfg.adapters)).transpose()?;
        anyhow::ensure!(cfg.backup.keep_days >= 1, "[backup] keep_days must be at least 1: with 0 every backup is deleted as soon as it is made");
        if let Some(b) = &cfg.browser {
            for site in &b.allow {
                anyhow::ensure!(
                    browser_allow_ok(site),
                    "[browser] allow: {site:?} is not a name, address or range: want `name`, `name:port`, `1.2.3.4`, `[::1]:8080` or `10.0.0.0/8`"
                );
            }
        }
        for c in &cfg.connectors {
            anyhow::ensure!(
                !c.name.contains("__") && c.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
                "connector name {:?} must be letters, digits, '-' or '_' and must not contain '__'",
                c.name
            );
            for (t, class) in &c.tools {
                anyhow::ensure!(crate::tools::Risk::parse(&class.risk).is_some(), "tool {}.{t}: unknown risk class {}", c.name, class.risk);
                if let Some(r) = &class.reach {
                    anyhow::ensure!(
                        crate::tools::reach_fits(r, &class.risk),
                        "tool {}.{t}: reach {r:?} does not fit a {} tool; the only reach is \"self\", on a write that stays in the person's own account",
                        c.name,
                        class.risk
                    );
                }
            }
            for (t, d) in &c.destinations {
                let risk = |t: &str| c.tools.get(t).map(|class| class.risk.clone());
                anyhow::ensure!(risk(t).is_some(), "connector {}: destinations for {t}, which is not one of its tools", c.name);
                if let Some(why) = d.fault(t, risk) {
                    anyhow::bail!("connector {}: {why}", c.name);
                }
            }
            if let Some(s) = &c.sent {
                let risk = |t: &str| c.tools.get(t).map(|class| class.risk.clone());
                if let Some(why) = s.fault(risk) {
                    anyhow::bail!("connector {}: {why}", c.name);
                }
            }
            anyhow::ensure!(crate::tools::Label::parse(&c.label).is_some(), "connector {}: unknown label {}", c.name, c.label);
            anyhow::ensure!(crate::tools::Audience::parse(&c.audience).is_some(), "connector {}: unknown audience {}", c.name, c.audience);
            if c.transport == crate::brokered::BROKERED {
                for r in &cfg.brokered.roots {
                    std::fs::read(r).map_err(|e| anyhow::anyhow!("[brokered] roots: {}: {e}", r.display()))?;
                }
                crate::brokered::digest(&c.url).map_err(|e| anyhow::anyhow!("connector {}: {e}", c.name))?;
                anyhow::ensure!(!c.hosts.is_empty(), "connector {}: a brokered connector names the provider hosts it may reach", c.name);
                // Its calls carry no idempotency key to its server, so one
                // whose outcome is unknown cannot be sent again blindly: the
                // person is asked (`none`).
                anyhow::ensure!(c.idempotency != "header", "connector {}: a brokered connector's calls carry no idempotency key, so it cannot have idempotency = \"header\"; leave it out, and a call whose outcome is unknown is put to the person", c.name);
            }
        }
        Ok(cfg)
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }

    /// Whether a password set on the appliance signs this person in. With an
    /// identity provider it is the owner's backup alone, so turning someone
    /// off at the provider shuts them out (owner, 2026-10-30: "The back up
    /// password is only for me"), and only on the appliance's own machine
    /// (the API checks where); the owner is read from this file, never from
    /// the provider. Without one, it is everyone's way in.
    pub fn may_use_password(&self, id: &str) -> bool {
        self.person(id).is_some_and(|p| self.sign_in.oidc.is_none() || p.is_owner())
    }

    pub fn person_by_token(&self, token: &str) -> Option<&Person> {
        use sha2::Digest;
        // Compare digests so the comparison time does not depend on the token.
        let want = sha2::Sha256::digest(token.as_bytes());
        self.people.iter().find(|p| p.token.as_ref().is_some_and(|t| sha2::Sha256::digest(t.as_bytes()) == want))
    }
}

impl Person {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
    pub fn is_owner(&self) -> bool {
        self.has_role("owner")
    }
    /// The developer view: raw log, runs, experiments (K10, K45).
    pub fn is_developer(&self) -> bool {
        self.has_role("developer")
    }
}

/// Whether this `[browser] allow` entry parses as the sidecar reads it
/// (`browser/destinations.js`, `parseAllow`): a name or address with or
/// without `:port`, or a range. A port is 1-5 digits, as there. Checked at
/// load so a bad entry fails here naming itself, instead of stopping the
/// sidecar with only "the browser sidecar did not start" (#242).
/// As the sidecar's `net.isIPv6`, which accepts a zone: exactly one `%`,
/// and the zone is one or more of letters, digits, `-.:` (probed on Node
/// 24.21: `fe80::1%eth0` and even `fe80::1%eth0:8080` parse; `fe80::1%`,
/// a second `%`, nothing before it, and `_`, space, `@`, `+`, `~`, `[`
/// do not). Rust's parser takes no zone at all, so it is tested without.
fn is_ipv6(s: &str) -> bool {
    match s.split_once('%') {
        None => s.parse::<std::net::Ipv6Addr>().is_ok(),
        Some((ip, zone)) => !zone.is_empty() && zone.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b':')) && ip.parse::<std::net::Ipv6Addr>().is_ok(),
    }
}

fn browser_allow_ok(entry: &str) -> bool {
    let lower = entry.trim().to_lowercase();
    let s = lower.strip_suffix('.').unwrap_or(&lower);
    if s.contains('/') || is_ipv6(s) {
        return range_ok(s);
    }
    // `[host]` or `[host]:port`.
    if let Some(rest) = s.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else { return false };
        if host.is_empty() {
            return false;
        }
        let port = if after.is_empty() { None } else { after.strip_prefix(':') };
        if !after.is_empty() && port.is_none() {
            return false;
        }
        return port.is_none_or(port_ok) && ip_or_name_ok(host);
    }
    // `host` or `host:port`, with no colon in the host.
    let (host, port) = match s.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (s, None),
    };
    if host.is_empty() || host.contains([':', '[', ']']) {
        return false;
    }
    port.is_none_or(port_ok) && ip_or_name_ok(host)
}

fn port_ok(p: &str) -> bool {
    (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit())
}

/// An address must parse; anything else is a name, which the sidecar
/// resolves when it is used.
fn ip_or_name_ok(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok() || !host.is_empty()
}

/// Whether this parses as the sidecar's `range`: an address, or an address
/// with a prefix length it has that many bits for.
fn range_ok(s: &str) -> bool {
    let (ip, len) = match s.split_once('/') {
        Some((ip, len)) => (ip, Some(len)),
        None => (s, None),
    };
    // As the sidecar's `bytes`: brackets and a zone come off first.
    let ip = ip.trim_start_matches('[').trim_end_matches(']');
    let ip = ip.split('%').next().unwrap_or("");
    let bits = match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => 32,
        Ok(std::net::IpAddr::V6(_)) => 128,
        Err(_) => return false,
    };
    match len {
        None => true,
        Some(l) => l.parse::<u32>().is_ok_and(|n| n <= bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::TEST_CONFIG as LEGAL;

    fn load(connector: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[[connectors]]\nname = \"mail\"\naudience = \"public\"\nlabel = \"private+untrusted\"\n{connector}");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// The console is retired, and a server's config written for it still
    /// loads: its `[admin]` section, every field the console read, is ignored.
    #[test]
    fn the_retired_consoles_section_still_loads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let admin = "[admin]\nhostname = \"admin.example\"\nlisten = [\"10.0.0.2:443\"]\nallow = [\"10.0.0.0/8\"]\ntls_cert = \"/c.pem\"\ntls_key = \"/k.pem\"\nidle_minutes = 30\ndatabase_url = \"postgres://magnus_root:x@h/m\"\ntrusted_via = \"tailscale\"\n";
        let text = format!(
            "[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}{admin}"
        );
        std::fs::write(&p, text).unwrap();
        assert!(Config::load(&p).unwrap().admin.is_some());
    }

    /// A brokered connector's calls carry no idempotency key, so it is
    /// never taken to deduplicate: one that says it does is refused, and
    /// one that says nothing does not.
    #[test]
    fn a_brokered_connector_cannot_claim_to_deduplicate() {
        let brokered = format!("transport = \"brokered\"\nurl = \"sha256:{}\"\nhosts = [\"api.example\"]\n", "0a".repeat(32));
        let e = load(&format!("{brokered}idempotency = \"header\"\n")).unwrap_err();
        assert!(format!("{e:#}").contains("carry no idempotency key"), "{e:#}");
        assert_eq!(load(&brokered).unwrap().connectors[0].idempotency, "none");
        // A remote app may say it honors the header.
        assert!(load("url = \"https://a.example/mcp\"\nidempotency = \"header\"\n").is_ok());
    }

    fn load_backup(backup: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[backup]\n{backup}");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// The schedule's waits come from `[backup]` (#289): a set value lands,
    /// an absent one keeps the old default.
    #[test]
    fn backup_waits_come_from_config() {
        let unset = load_backup("").unwrap().backup;
        assert_eq!(unset.fallback_wait_secs, 60);
        assert_eq!(unset.retry_wait_secs, 3600);
        let set = load_backup("fallback_wait_secs = 7\nretry_wait_secs = 120\n").unwrap().backup;
        assert_eq!(set.fallback_wait_secs, 7);
        assert_eq!(set.retry_wait_secs, 120);
    }

    /// The off-host destination comes from `[backup]` only (#297): none
    /// configured, no off-host copy.
    #[test]
    fn backup_copy_comes_from_config() {
        let unset = load_backup("").unwrap().backup;
        assert_eq!((unset.copy_to, unset.copy_timeout_secs), (None, 300));
        let set = load_backup("copy_to = \"strix-beta:/backups\"\ncopy_timeout_secs = 60\n").unwrap().backup;
        assert_eq!((set.copy_to.as_deref(), set.copy_timeout_secs), (Some("strix-beta:/backups"), 60));
    }

    fn load_browser(allow: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[browser]\ncommand = [\"node\", \"sidecar.js\"]\nchromium = \"/c\"\nallow = [{allow}]");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// A bad `[browser] allow` entry fails at load naming itself and what
    /// was expected, instead of stopping the sidecar with only "the browser
    /// sidecar did not start". What parses mirrors the sidecar's
    /// `parseAllow` (`browser/destinations.js`).
    #[test]
    fn a_bad_browser_allow_entry_fails_at_load_naming_itself() {
        for good in [
            "\"printer\"",
            "\"printer.lan\"",
            "\"printer.lan:631\"",
            "\"intranet.\"",
            "\"EXAMPLE.com:443\"",
            "\"10.1.2.3\"",
            "\"10.1.2.3:8080\"",
            "\"[::1]:8080\"",
            "\"::1\"",
            "\"10.1.0.0/16\"",
            "\"fc00::/7\"",
            "\"10.1.0.0/8\"",
            "\"fe80::1%eth0\"",
            "\"FE80::1%ETH0\"",
            "\"fe80::1%eth0:8080\"",
        ] {
            assert!(load_browser(good).is_ok(), "{good}");
        }
        for bad in [
            "\"\"",
            "\"10.0.0.0/33\"",
            "\"10.0.0.0/8/8\"",
            "\"10.0.0.0/-1\"",
            "\"[::1\"",
            "\"[::1]x\"",
            "\"printer.lan:\"",
            "\"printer.lan:123456\"",
            "\"a:b:c\"",
            "\":8080\"",
            "\"fe80::1%\"",
            "\"fe80::1%a%b\"",
            "\"fe80::1%eth_0\"",
            "\"fe80::1%eth 0\"",
        ] {
            let e = load_browser(bad).unwrap_err();
            assert!(format!("{e:#}").contains("[browser] allow") && format!("{e:#}").contains(bad.trim_matches('"')), "{bad}: {e:#}");
        }
    }

    /// A tool's reach is `self` on a write, or nothing: a read reaches no
    /// one to begin with, money asks every time, and any other word is
    /// unknown (approvals spec, The approval model).
    #[test]
    fn a_tool_reaches_self_only_as_a_write() {
        let tool = |risk: &str, reach: &str| load(&format!("url = \"https://a.example/mcp\"\n[connectors.tools.draft]\nrisk = \"{risk}\"\nreach = \"{reach}\"\n"));
        assert_eq!(tool("reversible_write", "self").unwrap().connectors[0].tools["draft"].reach.as_deref(), Some("self"));
        assert!(tool("irreversible", "self").is_ok());
        for (risk, reach) in [("financial", "self"), ("read", "self"), ("prohibited", "self"), ("reversible_write", "everyone")] {
            let e = tool(risk, reach).unwrap_err();
            assert!(format!("{e:#}").contains("does not fit"), "{risk} {reach}: {e:#}");
        }
    }
}
