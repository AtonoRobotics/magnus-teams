//! The appliance configuration file (TOML), written by the installer.

mod connector;
mod load;

mod sections;

pub use connector::{Connector, ToolClass, Watch};
pub use sections::*;

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
    /// `[memory]`, the retired memory engine's: still read, so a deployed config loads, and ignored.
    #[serde(default)]
    pub memory: Option<toml::Value>,
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
