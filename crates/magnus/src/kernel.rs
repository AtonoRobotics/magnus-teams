//! The kernel: wiring, the single writer, boot, and the sweeper that keeps
//! runs moving. Every change goes through `emit`: append to the log first,
//! then apply to the projection.

use crate::adapters::Adapters;
use crate::apps::{AppTool, Mcp};
use crate::config::{Config, Person};
use crate::events::Ev;
use crate::log::{Event, Log};
use crate::sandbox::Sandboxes;
use crate::state::{State, Status};
use crate::vault::Vault;
use crate::web::Web;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const HARNESS_VERSION: &str = concat!("magnus-", env!("CARGO_PKG_VERSION"));

/// The record a sign-in resolves: the config first, else the server's own
/// directory, where a local person has no token, groups or roles: a member,
/// nothing more. Split out so tests reach it without a kernel (Postgres).
pub(crate) fn resolve_person(cfg: &Config, accounts: &crate::accounts::Accounts, id: &str) -> Option<Person> {
    if let Some(p) = cfg.person(id) {
        return Some(p.clone());
    }
    let local = accounts.local_person(id)?;
    Some(Person { id: id.to_string(), name: local.name, token: None, groups: vec![], roles: vec![] })
}

pub struct Kernel {
    pub cfg: Config,
    pub(crate) log: Arc<Log>,
    writer: crate::commit::Writer,
    /// Held while a person's decision is checked and recorded, so an inbox
    /// item or a chat card is decided once however many answers race in, and
    /// a change of roles keeps an owner however many race in.
    pub(crate) decide: tokio::sync::Mutex<()>,
    /// Held while a person's relationship notes are read and a new version
    /// written, so two changes at once never lose one.
    pub(crate) notes: tokio::sync::Mutex<()>,
    pub vault: Arc<Vault>,
    pub accounts: crate::accounts::Accounts,
    /// Computers paired with this appliance, and the certificates Cloudflare
    /// issued for them. Empty until device certificates are configured.
    pub devices: crate::devices::Devices,
    /// Sign-in through an identity provider, when one is configured.
    pub oidc: Option<crate::oidc::Provider>,
    pub adapters: Adapters,
    pub mcp: Mcp,
    pub web: Web,
    pub sandboxes: Sandboxes,
    /// Brokered connectors' microVMs, one per person per connector.
    pub brokered: crate::brokered::Brokered,
    pub browser: crate::browser::Browser,
    drivers: Mutex<HashSet<String>>,
    pub(crate) leases: Mutex<HashMap<String, String>>,
    pub(crate) live_tools: tokio::sync::Mutex<HashMap<String, (Instant, Vec<AppTool>)>>,
    /// Each connection whose app refused Magnus the last time it asked for
    /// the app's tools, with the app's answer: said in the person's app index.
    pub(crate) refused_apps: Mutex<HashMap<String, String>>,
    pub live_text: Arc<Live>,
    /// The knowledge base's search index.
    pub knowledge: crate::knowledge::Index,
    /// People's content, read for one person at a time (recall only).
    pub recall: crate::recall::Recall,
    /// How long each log append took since boot, in microseconds (V6).
    pub capture: Mutex<Capture>,
}

/// Capture overhead: the cost of writing each step to the log (V6).
#[derive(Default, Clone, serde::Serialize)]
pub struct Capture {
    pub appends: u64,
    pub total_us: u64,
    pub max_us: u64,
    /// The most recent append times, for percentiles.
    #[serde(skip)]
    pub recent: std::collections::VecDeque<u64>,
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// New work refused because the kill switch is on.
#[derive(Debug)]
pub struct KillSwitchOn;

impl std::fmt::Display for KillSwitchOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The kill switch is on: nothing new starts until the owner releases it.")
    }
}

impl std::error::Error for KillSwitchOn {}

/// How a new run differs from a plain task.
#[derive(Default)]
pub struct NewRun {
    pub experiment: Option<crate::events::ExperimentTag>,
    pub chat: Option<crate::events::ChatOrigin>,
    pub label: Option<crate::tools::Label>,
    pub budget: Option<crate::config::Budget>,
    /// Fixed ids, so a resent request finds the task it already created.
    pub ids: Option<(String, String)>,
    /// The adapter and model the person chose; the appliance's otherwise.
    pub model: Option<(String, String)>,
    /// A short title the model wrote when starting it.
    pub title: Option<String>,
    /// Library files the person gave it.
    pub files: Vec<String>,
    /// The project it starts in.
    pub project: Option<String>,
    /// Removable modules switched on or off for this run alone, over the
    /// model's settings (an ablation trial).
    pub modules: std::collections::BTreeMap<String, bool>,
    /// The scope the person confirmed for the schedule starting it.
    pub scope: Option<String>,
    /// The grant it runs under (#235).
    pub grant: Option<String>,
}

impl Kernel {
    pub async fn boot(cfg: Config) -> Result<Arc<Kernel>> {
        std::fs::create_dir_all(&cfg.appliance.state_dir)?;
        // The kernel before this one (a switch of releases, a restart, a
        // crash) may still be letting go of the log: Postgres frees its lock
        // only once it sees that connection closed.
        let log = Log::open_waiting(&cfg.database.url, Duration::from_secs(15)).await?;
        // A removal stopped after its record: its content goes now.
        if let Err(e) = log.finish_removals().await {
            tracing::warn!("could not finish removals (has `magnus migrate` run since this release was installed?): {e:#}");
        }
        let mut state = State::default();
        for e in log.load().await? {
            state.apply(&e);
        }
        let vault = Arc::new(Vault::open(&cfg.appliance.state_dir)?);
        vault.bind_unbound()?;
        if state.kill_switch {
            vault.seal();
        }
        let accounts = crate::accounts::Accounts::open(&cfg.database.url, cfg.sign_in.clone()).await?;
        let devices = crate::devices::Devices::open(&cfg.appliance.state_dir)?;
        let oidc = cfg.sign_in.oidc.clone().map(|o| crate::oidc::Provider::new(o, vault.clone())).transpose()?;
        let adapters = Adapters::new(&cfg.appliance.state_dir, vault.clone(), &cfg.adapters);
        let writer = crate::commit::Writer::new(log, state, cfg!(debug_assertions).then(|| cfg.appliance.state_dir.clone()));
        let live_text = Arc::new(Live { tx: tokio::sync::broadcast::channel(1024).0, so_far: Mutex::new(HashMap::new()) });
        let k = Arc::new(Kernel {
            web: Web::new(&cfg.web),
            sandboxes: Sandboxes::new(cfg.sandbox.clone(), &cfg.appliance.state_dir),
            brokered: crate::brokered::Brokered::new(cfg.sandbox.clone(), cfg.brokered.clone(), &cfg.appliance.state_dir),
            browser: crate::browser::Browser::new(cfg.browser.clone(), &cfg.appliance.state_dir),
            mcp: Mcp::new().allowing_private(cfg.web.allow_private_networks),
            adapters,
            vault,
            accounts,
            devices,
            oidc,
            log: writer.log.clone(),
            writer,
            decide: tokio::sync::Mutex::new(()),
            notes: tokio::sync::Mutex::new(()),
            drivers: Mutex::new(HashSet::new()),
            leases: Mutex::new(HashMap::new()),
            live_tools: tokio::sync::Mutex::new(HashMap::new()),
            refused_apps: Mutex::new(HashMap::new()),
            live_text,
            knowledge: crate::knowledge::Index::new(&cfg.database.url),
            recall: crate::recall::Recall::new(&cfg.database.url),
            capture: Mutex::new(Capture::default()),
            cfg,
        });
        k.emit("kernel", Ev::KernelBoot { version: HARNESS_VERSION.into(), pid: std::process::id(), instructions: crate::run::instructions_version() }).await?;
        // A kill switch a crash cut short is engaged again before anything
        // else runs: until it is recorded, the schedule and action gates do
        // not see it. Revoking its sign-ins at their providers goes on in
        // the background.
        k.finish_kill_switch().await;
        // Items left open under asks spec rev 140 retired are put in their
        // current form before any run is driven (`legacy_items`).
        let retired = k.retire_legacy_items().await?;
        if retired > 0 {
            tracing::info!("retired {retired} items of asks no longer made");
        }
        // The search index catches up with the log (a restore, a missed
        // write, a new embedding model) without holding up the boot.
        let sync = k.clone();
        tokio::spawn(async move {
            if let Err(e) = sync.sync_knowledge().await {
                tracing::warn!("knowledge index: {e:#}");
            }
        });
        // Guest processes did not survive the restart; tell runs that used a sandbox (K20).
        let restarted: Vec<(String, u32)> = k.read(|s| {
            s.runs.values().filter(|r| !r.status.terminal() && r.intents.values().any(|i| i.tool.starts_with("sandbox_"))).map(|r| (r.id.clone(), r.replies.len() as u32)).collect()
        });
        for (run, step) in restarted {
            k.emit("kernel", Ev::Notice { run, step, text: "The appliance restarted. Processes in your sandbox were stopped; files in /workspace are kept.".into() }).await?;
        }
        Ok(k)
    }

    /// Ready events for the log, under the write lock: a secret typed into
    /// chat is masked so it is never written (spec, Typed secrets), and each
    /// document version is numbered, never shared, counting on after removal.
    fn number<'a>(&self, s: &State, evs: impl Iterator<Item = &'a mut Ev>) {
        let mut last: HashMap<String, u32> = HashMap::new();
        for ev in evs {
            crate::typed_secrets::mask_event(ev);
            if let Ev::BundleWritten { path, version, .. } = ev {
                let n = last.get(path.as_str()).copied().unwrap_or_else(|| s.bundle_last.get(path.as_str()).copied().unwrap_or(0)) + 1;
                last.insert(path.clone(), n);
                *version = n;
            }
        }
    }

    /// Append an event, then apply it. Only correctness writes come here (K6).
    pub async fn emit(&self, actor: &str, ev: Ev) -> Result<Event> {
        let t = Instant::now();
        let e = self.emit_all(vec![(actor, ev)]).await?.pop().context("the log gave back no event")?;
        let us = t.elapsed().as_micros() as u64;
        let mut c = self.capture.lock().unwrap();
        c.total_us += us;
        c.max_us = c.max_us.max(us);
        c.recent.push_back(us);
        if c.recent.len() > 2000 {
            c.recent.pop_front();
        }
        Ok(e)
    }

    /// Append several events, each with its actor, in one write and apply
    /// them together: no reader sees some of them without the rest, and no
    /// crash leaves some of them out. Each document version is numbered and
    /// a secret typed into chat masked under the write lock (`number`).
    pub async fn emit_all(&self, mut evs: Vec<(&str, Ev)>) -> Result<Vec<Event>> {
        let es = self
            .writer
            .write(|s| {
                self.number(s, evs.iter_mut().map(|(_, e)| e));
                // A run started in this batch is the person's of its TaskCreated.
                let mut runs = HashMap::new();
                let owners = evs
                    .iter()
                    .map(|(_, ev)| {
                        if let Ev::TaskCreated { run, person, .. } = ev {
                            runs.insert(run.clone(), person.clone());
                        }
                        s.content_owner(ev, &runs)
                    })
                    .collect();
                crate::commit::Batch { evs: evs.into_iter().map(|(a, e)| (a.to_string(), e)).collect(), owners }
            })
            .await?;
        self.capture.lock().unwrap().appends += es.len() as u64;
        Ok(es)
    }

    /// Debug builds only, for the sweep interleaving tests: while
    /// `<state_dir>/<gate>` exists, the sweep waits here after writing
    /// `what` to `<state_dir>/<ack>` so the test knows it arrived.
    /// Removing the gate lets it go on. Release builds, the only ones
    /// shipped, have none of this.
    #[cfg(debug_assertions)]
    async fn debug_gate(&self, gate: &str, ack: &str, what: &str) {
        let dir = &self.cfg.appliance.state_dir;
        if !dir.join(gate).exists() {
            return;
        }
        let _ = std::fs::write(dir.join(ack), what);
        let start = std::time::Instant::now();
        while dir.join(gate).exists() && start.elapsed() < std::time::Duration::from_secs(120) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let _ = std::fs::remove_file(dir.join(ack));
    }

    /// Debug builds only: the firing gate for the sweep race test (#261).
    #[cfg(debug_assertions)]
    async fn sweep_gate(&self, schedule: &str) {
        self.debug_gate("sweep-gate", "sweep-gated", schedule).await;
    }

    /// Debug builds only: the release gate for the stale-release test (#318).
    #[cfg(debug_assertions)]
    async fn release_gate(&self, schedule: &str) {
        self.debug_gate("release-gate", "release-gated", schedule).await;
    }

    pub async fn run_events(&self, run: &str) -> Result<Vec<Event>> {
        self.log.run_events(run).await
    }

    pub async fn output_bytes(&self, run: &str, id: &str) -> Result<Option<(String, String, Vec<u8>)>> {
        self.log.output(run, id).await
    }

    pub fn read<T>(&self, f: impl FnOnce(&State) -> T) -> T {
        f(&self.writer.state.lock().unwrap())
    }

    /// A person as every decision sees them: from the config, with the
    /// groups and roles the identity provider gave at their latest sign-in
    /// in place of the config's (E2).
    /// A person's rights: the identity provider's where it names them, else
    /// the roles the owner set in Settings, else the config's.
    /// The identity provider people sign in through, if the appliance has one.
    /// End the sessions, and pause the schedules, of people the identity
    /// provider has turned off or no longer knows: once now, then every five
    /// minutes. Everyone signed in or with a schedule is asked about; a
    /// provider that cannot be asked changes nothing (owner's ruling 12).
    pub fn watch_sessions(self: &Arc<Self>) {
        let k = self.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                t.tick().await;
                k.end_turned_off().await;
            }
        });
    }

    pub async fn end_turned_off(&self) -> usize {
        let Some(id) = self.identity() else { return 0 };
        let mut people = self.accounts.signed_in().await;
        people.extend(self.read(|s| s.schedules.values().filter(|x| x.next_due().is_some()).map(|x| x.person.clone()).collect::<Vec<_>>()));
        let mut ended = 0;
        for person in people {
            let sub = self.read(|s| s.subjects.get(&person).cloned());
            match id.active(&person, sub.as_deref()).await {
                Ok(true) => {}
                Ok(false) => {
                    let label = id.label();
                    ended += self.turn_off(&person, &format!("{label} has turned {person} off or no longer knows them"), &format!("{label} has turned your account off")).await;
                }
                Err(e) => tracing::debug!("cannot ask whether {person} is still on: {e:#}"),
            }
        }
        ended
    }

    /// A person is turned off (at the provider, or by an admin in the
    /// console): their sessions here end, and their schedules stop acting
    /// for them until they are turned back on and resume them. `why` is for
    /// the log, `told` for the person. How many sessions ended.
    pub async fn turn_off(&self, person: &str, why: &str, told: &str) -> usize {
        let mut ended = 0;
        // Their apps answer no calls for them from now; the sign-ins stay in
        // the vault, unused, for when they are back (connectors spec, Isolation).
        if !self.read(|s| s.turned_off.contains(person)) {
            self.tell(vec![("kernel", Ev::PersonTurnedOff { person: person.to_string(), why: why.to_string() })]).await;
        }
        match self.accounts.end_all(person).await {
            Ok(sessions) => {
                if !sessions.is_empty() {
                    tracing::warn!("{why}: their {} sessions here are ended", sessions.len());
                }
                ended = sessions.len();
                let evs: Vec<(&str, Ev)> = sessions.into_iter().map(|session| ("kernel", Ev::SignedOut { person: person.to_string(), session })).collect();
                if !evs.is_empty() {
                    self.tell(evs).await;
                }
            }
            Err(e) => tracing::error!("ending {person}'s sessions: {e:#}"),
        }
        let schedules: Vec<crate::state::Schedule> = self.read(|s| s.schedules.values().filter(|x| x.person == person && x.next_due().is_some()).cloned().collect());
        for sc in schedules {
            let title = self.schedule_title_for_notice(&sc).await;
            let notice = self.notice_to(
                None,
                None,
                person.to_string(),
                format!("A schedule paused: {title}"),
                serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": told } }),
            );
            self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: told.to_string(), task: None, admin: false, source: None, turn: None }), ("kernel", notice)])
                .await;
        }
        ended
    }

    pub fn identity(&self) -> Option<&dyn crate::identity::Identity> {
        self.oidc.as_ref().map(|o| o as &dyn crate::identity::Identity)
    }

    pub fn person(&self, id: &str) -> Option<Person> {
        let mut p = resolve_person(&self.cfg, &self.accounts, id)?;
        if let Some(r) = self.read(|s| s.roles_set.get(id).cloned()) {
            p.roles = r;
        }
        if let Some((groups, roles)) = self.read(|s| s.provider_rights.get(id).cloned()) {
            if let Some(g) = groups {
                p.groups = g;
            }
            if let Some(r) = roles {
                p.roles = r;
            }
        }
        Some(p)
    }

    pub fn owners(&self) -> Vec<Person> {
        self.cfg.people.iter().filter_map(|p| self.person(&p.id)).filter(|p| p.is_owner()).collect()
    }

    /// Start driving a run if nothing is.
    /// The files a run's sandbox starts with: its task's attachments and
    /// the library files it was given, recalled for its person.
    async fn set_inputs(&self, run: &str) -> Result<()> {
        let task = self.read(|s| s.runs.get(run).map(|r| r.task.clone()).unwrap_or_default());
        let mut files: Vec<_> = self.read(|s| s.attachments.get(&task).cloned().unwrap_or_default()).into_iter().filter(|a| !crate::attachment::is_outside(a)).collect();
        files.extend(self.given_files(&task).await?);
        if !files.is_empty() {
            use base64::Engine;
            let decoded = files
                .iter()
                .filter_map(|a| {
                    let data = base64::engine::general_purpose::STANDARD.decode(a["data_b64"].as_str()?).ok()?;
                    Some((a["name"].as_str()?.to_string(), data))
                })
                .collect();
            self.sandboxes.set_inputs(run, decoded);
        }
        Ok(())
    }

    pub fn wake(self: &Arc<Self>, run: &str) {
        if !self.drivers.lock().unwrap().insert(run.to_string()) {
            return;
        }
        let k = self.clone();
        let run = run.to_string();
        tokio::spawn(async move {
            let r = match k.set_inputs(&run).await {
                Ok(()) => k.clone().drive(&run).await,
                Err(e) => Err(e),
            };
            k.drivers.lock().unwrap().remove(&run);
            if let Err(e) = r {
                tracing::error!("run {run}: {e:#}");
                // Stop it with a notice rather than retry it unseen (K24).
                let running = k.read(|s| s.runs.get(&run).is_some_and(|r| r.status == Status::Running));
                if running {
                    let title = match k.read(|s| s.runs.get(&run).cloned()) {
                        Some(r) => k.title_of(&r).await,
                        None => String::new(),
                    };
                    if let Err(e2) = k.suspend_with_notice(&run, "error", format!("Stopped by an error: {title}"), serde_json::json!({ "error": format!("{e:#}") })).await {
                        tracing::error!("run {run}: could not record the error: {e2:#}");
                    }
                }
            }
        });
    }

    pub fn any_driving(&self) -> bool {
        !self.drivers.lock().unwrap().is_empty()
    }

    pub fn driving(&self, run: &str) -> bool {
        self.drivers.lock().unwrap().contains(run)
    }

    /// Create a task for `person` with the current model and budgets.
    pub async fn create_task(self: &Arc<Self>, actor: &str, person: &str, text: String, attachments: Vec<serde_json::Value>) -> Result<(String, String)> {
        self.create_tagged(actor, person, text, attachments, None, None, Default::default()).await
    }

    /// Create a task, optionally as a trial of an experiment case (V2).
    pub async fn create_tagged(
        self: &Arc<Self>,
        actor: &str,
        person: &str,
        text: String,
        attachments: Vec<serde_json::Value>,
        experiment: Option<crate::events::ExperimentTag>,
        model: Option<(String, String)>,
        modules: std::collections::BTreeMap<String, bool>,
    ) -> Result<(String, String)> {
        self.create_run(actor, person, text, attachments, NewRun { experiment, model, modules, ..Default::default() }).await
    }

    /// The adapter and model new runs use unless the person chooses one.
    pub fn default_model(&self) -> (String, String) {
        let m = &self.cfg.model;
        self.read(|s| s.model.clone()).unwrap_or_else(|| (m.adapter.clone(), m.name.clone()))
    }

    /// The models a person may choose for a task or a chat message: each
    /// adapter's catalog as its provider lists it now. An adapter whose
    /// provider does not answer (not signed in, unreachable) offers none.
    pub async fn models(&self) -> Vec<(String, Vec<String>)> {
        // A provider an admin disabled offers nothing.
        let off = self.read(|s| s.providers_off.clone());
        let ids: Vec<String> = self.adapters.list().await.into_iter().map(|(id, ..)| id).filter(|id| !off.contains(id)).collect();
        let lists = futures::future::join_all(ids.iter().map(|id| self.adapters.catalog(id))).await;
        ids.into_iter()
            .zip(lists)
            .filter_map(|(id, ev)| match ev {
                Ok(magnus_adapter_proto::Event::Catalog { models }) if !models.is_empty() => Some((id, models.into_iter().map(|m| m.id).collect())),
                _ => None,
            })
            .collect()
    }

    /// A model the person chose, checked against its adapter's catalog. The
    /// appliance's own default needs no check.
    pub async fn check_model(&self, adapter: &str, model: &str) -> std::result::Result<(), String> {
        if self.default_model() == (adapter.to_string(), model.to_string()) {
            return Ok(());
        }
        if self.adapters.list().await.iter().all(|(id, ..)| id != adapter) {
            return Err(format!("no adapter {adapter}"));
        }
        match self.adapters.catalog(adapter).await {
            Ok(magnus_adapter_proto::Event::Catalog { models }) if models.iter().any(|m| m.id == model) => Ok(()),
            Ok(magnus_adapter_proto::Event::Catalog { .. }) => Err(format!("{adapter} does not offer {model}")),
            Ok(_) => Err(format!("{adapter} sent no model list")),
            Err(e) => Err(format!("{adapter} cannot list its models: {e:#}")),
        }
    }

    /// Create a task or a chat turn and start it. Nothing new starts while
    /// the kill switch is on (owner's ruling 11, 2026-11-01).
    pub async fn create_run(self: &Arc<Self>, actor: &str, person: &str, text: String, attachments: Vec<serde_json::Value>, o: NewRun) -> Result<(String, String)> {
        if self.read(|s| s.kill_switch) {
            return Err(KillSwitchOn.into());
        }
        let (task, run) = match &o.ids {
            Some((t, r)) => (t.clone(), r.clone()),
            None => (new_id("task"), new_id("run")),
        };
        let m = &self.cfg.model;
        let (adapter, model) = o.model.clone().unwrap_or_else(|| self.default_model());
        let (mut modules, escalate_to) = self.modules_for(&adapter, &model);
        modules.extend(o.modules.clone());
        crate::outcomes::mark(&mut modules, o.chat.as_ref());
        let config = crate::events::RunConfig {
            adapter,
            model,
            max_retries: m.max_retries,
            max_tokens: m.max_tokens,
            budget: o.budget.unwrap_or_else(|| self.cfg.budget.clone()),
            verifier: modules.get("verifier").copied().unwrap_or(false),
            modules,
            escalate_to,
            lost: self.cfg.modules.lost.clone(),
            verifier_model: self.cfg.verifier.model.clone(),
            instructions: if actor == "review" {
                crate::run::review_instructions_version()
            } else if o.chat.as_ref().is_some_and(|c| c.turn) {
                crate::run::chat_instructions_version()
            } else {
                crate::run::instructions_version()
            },
            harness: HARNESS_VERSION.into(),
            playbook: self.read(|s| s.playbook_version),
        };
        // The project's instructions and reference files as they stand now.
        let project = o.project.as_ref().and_then(|id| {
            self.read(|s| {
                let p = s.projects.get(id).filter(|p| p.person == person)?;
                let mut files: Vec<&crate::state::LibFile> = s.files.values().filter(|f| f.person == person && s.project_of(&f.id).as_deref() == Some(id)).collect();
                files.sort_by_key(|f| f.at);
                Some(crate::events::ProjectContext {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    instructions: p.instructions.clone(),
                    files: files.into_iter().map(|f| f.id.clone()).collect(),
                })
            })
        });
        self.emit(
            actor,
            Ev::TaskCreated {
                task: task.clone(),
                run: run.clone(),
                person: person.into(),
                text,
                attachments,
                config,
                experiment: o.experiment,
                chat: o.chat,
                label: o.label,
                title: o.title,
                files: o.files,
                project,
                scope: o.scope,
                grant: o.grant,
            },
        )
        .await?;
        self.wake(&run);
        Ok((task, run))
    }

    /// Keep runs moving: restart drivers for running runs, wake due timers,
    /// start scheduled tasks, return ended tasks' outcomes to their threads.
    pub async fn sweep(self: Arc<Self>) {
        loop {
            let now = chrono::Utc::now();
            // A schedule past its end or out of runs is over: it stays,
            // marked with why, and its person is told.
            let finished: Vec<(crate::state::Schedule, String)> = self.read(|s| s.schedules.values().filter_map(|x| x.finished(now).map(|why| (x.clone(), why))).collect());
            for (sc, why) in finished {
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    None,
                    None,
                    sc.person.clone(),
                    format!("A schedule ended: {title}"),
                    serde_json::json!({ "reason": "schedule_ended", "evidence": { "schedule": sc.id, "why": why } }),
                );
                self.tell(vec![("kernel", Ev::ScheduleEnded { id: sc.id.clone(), reason: why, run: None }), ("kernel", notice)]).await;
            }
            // A schedule whose run waits on a person pauses until that person
            // answers (owner, 2026-10-30: without it, it acts without the
            // authority or the information it needs), then goes on by itself.
            let (hold, release): (Vec<(crate::state::Schedule, String, String)>, Vec<String>) = self.read(|s| {
                let mut hold = vec![];
                let mut release = vec![];
                for x in s.schedules.values() {
                    match (s.held_by_person(&x.id), &x.paused_on) {
                        (Some((task, why)), None) if x.paused.is_none() && x.ended.is_none() => hold.push((x.clone(), task, why)),
                        (None, Some(_)) => release.push(x.id.clone()),
                        _ => {}
                    }
                }
                (hold, release)
            });
            for (sc, task, why) in hold {
                let title = self.schedule_title_for_notice(&sc).await;
                let notice = self.notice_to(
                    Some(task.clone()),
                    None,
                    sc.person.clone(),
                    format!("A schedule paused until you answer its run: {title}"),
                    serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": why, "task": task } }),
                );
                self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: why, task: Some(task), admin: false, source: None, turn: None }), ("kernel", notice)])
                    .await;
            }
            for id in release {
                #[cfg(debug_assertions)]
                self.release_gate(&id).await;
                // The hold's end was read before the notifications above
                // went out; a pause that landed since must not be resumed
                // here. Only a schedule still paused on a run that no
                // longer waits goes on (#318).
                let ended = self.read(|s| s.held_by_person(&id).is_none());
                let held_pause = self.read(|s| s.schedules.get(&id).is_some_and(|x| x.paused_on.is_some()));
                if ended && held_pause {
                    let _ = self.emit("kernel", Ev::ScheduleResumed { id, source: None, turn: None }).await;
                }
            }
            let due_schedules: Vec<crate::state::Schedule> =
                self.read(|s| if s.kill_switch { vec![] } else { s.schedules.values().filter(|x| x.next_due().is_some_and(|t| t <= now)).cloned().collect() });
            for sc in due_schedules {
                let title = self.schedule_title_for_notice(&sc).await;
                // Not while its last run is open: running, or waiting on
                // someone. Runs that pile up waiting flood the inbox
                // (2026-10-30: 75 runs of one test schedule).
                if let Some((task, why)) = self.read(|s| s.open_run_of(&sc.id)) {
                    let _ = self.emit("kernel", Ev::ScheduleSkipped { id: sc.id.clone(), task, reason: why }).await;
                    continue;
                }
                // Too many failures in a row: it stops spending until its
                // person looks.
                let n = self.cfg.schedules.pause_after_failures;
                if n > 0 && self.read(|s| s.failures_in_a_row(&sc.id)) >= n {
                    let why = format!("its last {n} runs failed");
                    let notice = self.notice_to(
                        None,
                        None,
                        sc.person.clone(),
                        format!("A schedule paused: {title}"),
                        serde_json::json!({ "reason": "schedule_paused", "evidence": { "schedule": sc.id, "why": why } }),
                    );
                    self.tell(vec![("kernel", Ev::SchedulePaused { id: sc.id.clone(), reason: why, task: None, admin: false, source: None, turn: None }), ("kernel", notice)])
                        .await;
                    continue;
                }
                // A scheduled task acts for the person who set it up (K54),
                // within the schedule's budget.
                // Its runs start in the project the schedule is in now.
                // What it asks, and the scope its person confirmed, recalled
                // for them; a recall that fails is a firing that failed.
                let started = match self.words_of(&sc.person, sc.seq).await {
                    Ok(w) => {
                        #[cfg(debug_assertions)]
                        self.sweep_gate(&sc.id).await;
                        // Cancelled or paused while its words were recalled:
                        // no run starts for a schedule that is gone or paused
                        // now (behavior spec: cancelling stops future runs and
                        // any run in flight; pausing stops future runs). No
                        // await stands between this recheck and the creation
                        // below.
                        if !self.read(|s| s.schedules.get(&sc.id).is_some_and(|x| x.paused.is_none())) {
                            continue;
                        }
                        let o = NewRun {
                            // The server's per-task limit, never one the model set on the card.
                            budget: Some(crate::task_budget::scheduled(&self.cfg.budget, sc.budget.as_ref())),
                            label: (sc.label != crate::tools::Label::PRINCIPAL).then_some(sc.label),
                            project: self.read(|s| s.project_of(&sc.id)),
                            scope: w.opt(sc.seq, "scope"),
                            // Each run posts in the conversation the schedule was set up in, where
                            // its outcome wakes the agent too (outcomes.rs); one set up elsewhere,
                            // or before that was recorded, in its own thread (owner, 2026-11-10).
                            chat: Some(crate::events::ChatOrigin {
                                thread: self.read(|s| crate::schedule_thread::of(s, &sc)),
                                turn: false,
                                from_run: None,
                                conversation: vec![],
                                opener: None,
                            }),
                            // A confirmed schedule runs on its own: the verifier judges the criteria
                            // no code checks, and only what it cannot judge goes to its person
                            // (owner, 2026-11-02; invariant 7 holds).
                            modules: [("verifier".to_string(), true)].into(),
                            ..Default::default()
                        };
                        self.create_run(&format!("schedule:{}", sc.id), &sc.person, w.text(sc.seq).to_string(), vec![], o).await
                    }
                    Err(e) => Err(e),
                };
                match started {
                    Ok((task, _)) => {
                        let _ = self.emit("kernel", Ev::ScheduleFired { id: sc.id.clone(), task }).await;
                    }
                    Err(e) => {
                        tracing::error!("schedule {}: {e:#}", sc.id);
                        let notice = self.notice_to(
                            None,
                            None,
                            sc.person.clone(),
                            format!("A schedule could not start: {title}"),
                            serde_json::json!({ "reason": "schedule_failed", "evidence": { "schedule": sc.id, "error": format!("{e:#}") } }),
                        );
                        self.tell(vec![("kernel", Ev::ScheduleFailed { id: sc.id.clone(), reason: format!("{e:#}") }), ("kernel", notice)]).await;
                    }
                }
            }
            let (running, due): (Vec<String>, Vec<(String, u32)>) = self.read(|s| {
                let running = s.runs.values().filter(|r| r.status == Status::Running).map(|r| r.id.clone()).collect();
                let due = s
                    .runs
                    .values()
                    .filter(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("timer") && r.wait_until.is_some_and(|t| t <= now))
                    .map(|r| (r.id.clone(), r.replies.len() as u32))
                    .collect();
                (running, due)
            });
            // What the model is told and the resume, in one write: a crash
            // between them left the run waiting with the wait's end told,
            // and told again on the next sweep.
            for (run, step) in due {
                let told = Ev::Notice { run: run.clone(), step, text: format!("It is now {}. Your wait has ended.", now.to_rfc3339()) };
                if self.emit_all(vec![("kernel", told), ("kernel", Ev::Resumed { run: run.clone() })]).await.is_ok() {
                    self.wake(&run);
                }
            }
            for run in running {
                if !self.driving(&run) {
                    self.wake(&run);
                }
            }
            // The proactive loop's own wakes: cadence, overnight, coming-up schedules, due commitments and stale approvals (#237).
            self.settle_background().await;
            self.return_outcomes().await;
            self.review_due(now).await;
            // Scheduled polls of watched reads (#406).
            self.poll_due(now).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Release a run's resources when it stops moving.
    pub async fn on_stop(&self, run: &str, terminal: bool) {
        self.leases.lock().unwrap().retain(|_, holder| holder != run);
        if terminal {
            self.close_run_browsers(run).await;
            self.sandboxes.destroy(run).await;
        } else {
            self.sandboxes.stop(run).await;
        }
    }

    pub async fn suspend(&self, run: &str, reason: &str, item: Option<String>) -> Result<()> {
        self.emit("kernel", Ev::Suspended { run: run.into(), reason: reason.into(), item }).await?;
        self.on_stop(run, false).await;
        Ok(())
    }

    /// Stop a run on an inbox item (`item`, when it is new) in one write:
    /// nobody sees the item while the run still shows as running.
    pub async fn suspend_on(&self, run: &str, reason: &str, id: String, item: Option<Ev>) -> Result<()> {
        // Background work never waits on its person: it ends, and the next
        // wake tries again. The log keeps why, for the run inspector; the
        // person sees nothing (owner, 2026-11-07).
        if self.is_background(run) && reason != "kill_switch" {
            let ended = Ev::Failed { run: run.into(), reason: format!("background work ended rather than wait on its person ({reason})"), by: "kernel".into() };
            self.emit("kernel", ended).await?;
            self.on_stop(run, true).await;
            return Ok(());
        }
        let stop = Ev::Suspended { run: run.into(), reason: reason.into(), item: Some(id) };
        self.emit_all(item.into_iter().chain([stop]).map(|e| ("kernel", e)).collect()).await?;
        self.on_stop(run, false).await;
        Ok(())
    }

    /// Code stops a run and tells the person why, with the evidence (K24).
    pub async fn suspend_with_notice(&self, run: &str, reason: &str, summary: String, details: serde_json::Value) -> Result<()> {
        let (task, person) = self.read(|s| s.runs.get(run).map(|r| (r.task.clone(), r.person.clone()))).unwrap_or_default();
        let (id, ev) = notice_event(Some(task), Some(run.into()), person, summary, serde_json::json!({ "reason": reason, "evidence": details }));
        self.suspend_on(run, reason, id, Some(ev)).await
    }

    /// Background work left waiting on its person, from before it stopped
    /// asking (owner, 2026-11-07): each such run ends, and every card it
    /// put up that is not one of its outputs is put away. A run so held
    /// also held every later review for its person.
    pub async fn settle_background(&self) {
        let (runs, items) = self.read(|s| {
            let bg = |task: &Option<String>| task.as_ref().and_then(|t| s.tasks.get(t)).is_some_and(|t| t.background());
            let runs: Vec<String> = s
                .runs
                .values()
                .filter(|r| r.status == crate::state::Status::Suspended && !["timer", "update", "kill_switch"].contains(&r.reason.as_deref().unwrap_or_default()))
                .filter(|r| s.tasks.get(&r.task).is_some_and(|t| t.background()))
                .map(|r| r.id.clone())
                .collect();
            let items: Vec<String> =
                s.items.values().filter(|i| i.resolution.is_none() && bg(&i.task) && !crate::state::BACKGROUND_OUTPUTS.contains(&i.kind.as_str())).map(|i| i.id.clone()).collect();
            (runs, items)
        });
        if runs.is_empty() && items.is_empty() {
            return;
        }
        let mut evs: Vec<(&str, Ev)> = items
            .into_iter()
            .map(|id| ("kernel", Ev::ItemResolved { id, decision: "dismiss".into(), scope: None, text: Some("background work: not the person's to answer".into()) }))
            .collect();
        evs.extend(runs.iter().map(|r| ("kernel", Ev::Failed { run: r.clone(), reason: "background work ended rather than wait on its person".into(), by: "kernel".into() })));
        self.tell(evs).await;
        for r in &runs {
            self.on_stop(r, true).await;
        }
    }

    /// Whether a run is Magnus's own background work (`Task::background`).
    pub fn is_background(&self, run: &str) -> bool {
        self.read(|s| s.authority(run).is_some_and(|t| t.background()))
    }

    /// A notice in the person's inbox (something ended or stopped, with the
    /// evidence) as an event, to be written in the same write as what it
    /// tells of (`emit_all`, K24).
    pub fn notice_to(&self, task: Option<String>, run: Option<String>, person: String, summary: String, details: serde_json::Value) -> Ev {
        notice_event(task, run, person, summary, details).1
    }

    /// The sweep writes an event and its notice; if the write fails, both
    /// are left unwritten and the next sweep tries again, so it is logged
    /// rather than returned.
    async fn tell(&self, evs: Vec<(&str, Ev)>) {
        if let Err(e) = self.emit_all(evs).await {
            tracing::error!("sweep: {e:#}");
        }
    }

    /// A run's notice as an event, to be written in the same write as what
    /// it tells of (`emit_all`): a failure is never seen, nor left by a
    /// crash, without its notice (K24).
    pub fn notice_of(&self, run: &crate::state::Run, summary: String, details: serde_json::Value) -> Ev {
        self.notice_to(Some(run.task.clone()), Some(run.id.clone()), run.person.clone(), summary, details)
    }

    /// The task's title, or the start of what the person asked, recalled
    /// for its person. A notice still goes when it cannot be recalled.
    pub async fn title_of(&self, run: &crate::state::Run) -> String {
        let Some(t) = self.read(|s| s.tasks.get(&run.task).cloned()) else { return String::new() };
        self.task_title(&t, Some(run)).await.unwrap_or_else(|e| {
            tracing::warn!("recall the title of {}: {e:#}", t.task_id);
            String::new()
        })
    }

    /// A schedule's title for a notice: a notice still goes when it cannot
    /// be recalled.
    pub async fn schedule_title_for_notice(&self, sc: &crate::state::Schedule) -> String {
        self.schedule_title(sc).await.unwrap_or_else(|e| {
            tracing::warn!("recall the title of schedule {}: {e:#}", sc.id);
            String::new()
        })
    }

    /// The run's last few tool results, for a notice's evidence.
    pub fn last_results(&self, run: &crate::state::Run) -> Vec<serde_json::Value> {
        let r = self.read(|s| s.runs.get(&run.id).cloned()).unwrap_or_else(|| run.clone());
        r.results_in_order().into_iter().rev().take(3).collect()
    }

    /// In a chat turn: the actions the person asked for (start a task,
    /// propose a schedule) that failed and were not made good later, with
    /// the reason.
    pub fn unfinished_chat_actions(&self, run: &crate::state::Run) -> Vec<serde_json::Value> {
        let r = self.read(|s| s.runs.get(&run.id).cloned()).unwrap_or_else(|| run.clone());
        let mut out: Vec<serde_json::Value> = vec![];
        for tool in ["create_task", "create_schedule"] {
            // Each attempt with its title and whether it worked, in order.
            let tried: Vec<(String, bool, String)> = r
                .replies
                .iter()
                .flat_map(|x| x.calls())
                .filter(|(_, name, _)| name == tool)
                .filter_map(|(id, _, input)| r.results.get(&id).map(|x| (input["title"].as_str().unwrap_or_default().trim().to_lowercase(), x.is_error, x.text.clone())))
                .collect();
            // A failed attempt is made good by a later success of the same
            // thing (the same title, or any title when it had none); one
            // success makes good one failure.
            let mut used = vec![false; tried.len()];
            let mut failed = vec![];
            for (i, (title, err, text)) in tried.iter().enumerate() {
                if !err {
                    continue;
                }
                let later = (i + 1..tried.len()).find(|&j| !used[j] && !tried[j].1 && (title.is_empty() || tried[j].0 == *title));
                match later {
                    Some(j) => used[j] = true,
                    None => failed.push(text.clone()),
                }
            }
            if !failed.is_empty() {
                out.push(serde_json::json!({ "tool": tool, "attempts": tried.iter().filter(|t| t.1).count(), "error": failed.last() }));
            }
        }
        out
    }
}

pub(crate) fn notice_event(task: Option<String>, run: Option<String>, person: String, summary: String, details: serde_json::Value) -> (String, Ev) {
    let id = new_id("item");
    (id.clone(), Ev::Item { id, item_kind: "notice".into(), task, run, person, summary, details })
}

/// Model text as it streams, before the reply is appended to the log (K6),
/// and each run's reply so far, so a view that opens mid-reply starts with
/// what was already said.
pub struct Live {
    tx: tokio::sync::broadcast::Sender<(String, String)>,
    so_far: Mutex<HashMap<String, String>>,
}

impl Live {
    pub fn send(&self, run: &str, text: &str) {
        let mut so_far = self.so_far.lock().unwrap();
        so_far.entry(run.to_string()).or_default().push_str(text);
        let _ = self.tx.send((run.to_string(), text.to_string()));
    }

    /// The run's reply is complete (or failed): nothing more streams for it.
    pub fn end(&self, run: &str) {
        self.so_far.lock().unwrap().remove(run);
    }

    /// The run's reply so far and what streams after it, with nothing lost
    /// or repeated between the two.
    pub fn subscribe(&self, run: &str) -> (String, tokio::sync::broadcast::Receiver<(String, String)>) {
        let so_far = self.so_far.lock().unwrap();
        (so_far.get(run).cloned().unwrap_or_default(), self.tx.subscribe())
    }
}

#[cfg(test)]
mod tests {
    /// Sign-ins resolve the config first and the appliance's own directory
    /// after: a config person keeps their record even with a local namesake,
    /// a local person arrives as a plain member, and nobody else resolves.
    #[tokio::test(flavor = "multi_thread")]
    async fn sign_ins_resolve_config_first_then_local() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        std::fs::write(&p, "[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n[[people]]\nid = \"sam\"\nname = \"Sam\"\nroles = [\"owner\"]\n").unwrap();
        let cfg = super::Config::load(&p).unwrap();
        let server = std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into());
        let name = format!("k_{}", uuid::Uuid::new_v4().simple());
        let (server_client, conn) = tokio_postgres::connect(&server, tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        server_client.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
        let mut u = url::Url::parse(&server).unwrap();
        u.set_path(&format!("/{name}"));
        let url = u.to_string();
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        client.batch_execute(&crate::accounts::migrate_sql("postgres")).await.unwrap();
        let accounts = crate::accounts::Accounts::open(&url, cfg.sign_in.clone()).await.unwrap();
        accounts.add_person("sam", "Intruder", None).await.unwrap();
        accounts.add_person("ada", "Ada", None).await.unwrap();
        let sam = super::resolve_person(&cfg, &accounts, "sam").unwrap();
        assert_eq!((sam.name.as_str(), sam.is_owner()), ("Sam", true), "the config lost to a local namesake");
        let ada = super::resolve_person(&cfg, &accounts, "ada").unwrap();
        assert_eq!(ada.name.as_str(), "Ada", "the local record came back wrong");
        assert!(!ada.is_owner() && !ada.is_developer() && ada.token.is_none() && ada.groups.is_empty(), "a local person arrived with powers");
        assert!(super::resolve_person(&cfg, &accounts, "ghost").is_none(), "nobody resolved");
        server_client.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await.unwrap();
    }
}
