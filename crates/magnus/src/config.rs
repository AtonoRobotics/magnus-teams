//! The appliance configuration file (TOML), written by the installer.

mod connector;
mod load;

pub use connector::{Connector, ToolClass, Watch};

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

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
    /// What child processes inherit from Magnus's own environment, read once here.
    #[serde(skip, default = "crate::spawn::HostEnv::from_process")]
    pub host: crate::spawn::HostEnv,
    /// The server's own time zone by its IANA name, read once here: the zone
    /// of a person who names none (`cadence::zone_of`).
    #[serde(skip, default = "crate::config::load::server_zone_name")]
    pub server_zone: String,
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
    /// This server's registration with the relay. None: no sign-in goes through it.
    pub relay: Option<crate::connectors::relay::Settings>,
    /// The facts the agreements state about whoever runs the server; all required.
    #[serde(default)]
    pub legal: crate::legal::Settings,
    #[serde(default)]
    pub memory: crate::memory_index::MemoryConfig,
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
    /// HTTPS for people's apps on the network. Without `tls_cert` and `tls_key`, the appliance
    /// makes its own certificate on first start and apps pin it on first connection.
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
    /// Optional fallback models (`"adapter/model"`) tried in order when the
    /// primary provider is unavailable or over its usage limit after retries.
    #[serde(default)]
    pub fallbacks: Vec<String>,
    /// Optional fast worker model (`"adapter/model"`) used for lightweight
    /// sub-agents like `research_web` so they run faster and cheaper.
    #[serde(default)]
    pub worker: Option<String>,
}

impl Model {
    pub fn worker_model(&self) -> Option<(String, String)> {
        let raw = self.worker.as_deref()?.trim();
        let (a, m) = raw.split_once('/')?;
        (!a.is_empty() && !m.is_empty()).then(|| (a.to_string(), m.to_string()))
    }
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
    /// What it charges, in US dollars per million tokens; 0 for a local
    /// server. Unpriced, it embeds only where no spend limit applies
    /// (`spend::paid`), and search falls back to words elsewhere.
    #[serde(default)]
    pub usd_per_mtok: Option<f64>,
}

/// As long as the identity provider's requests may take.
fn embed_timeout() -> u64 {
    30
}
