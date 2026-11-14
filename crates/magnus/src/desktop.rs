//! Programs on the person's own computers (connectors spec, Local desktop
//! connectors; GAP10 phase 1, the bridge).
//!
//! The desktop app finds the supported programs installed on its computer
//! and reports them here, with whether each answers on its loopback port.
//! The person allows a program once per computer ("Use Blender on this
//! computer"), which the log keeps. A call to a program's tool is decided
//! here, by the layered Cedar policy like any other, and then handed to that
//! computer's app over the same long poll it already holds with its own
//! session. The app forwards it to the program's server on 127.0.0.1 only.
//! The server never names an address: the app holds the port for each
//! program it enabled and refuses anything else.
use crate::boundary::{Action, Facts};
use crate::config::Person;
use crate::kernel::Kernel;
use crate::policy::{Outcome, ToolFacts};
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use anyhow::Result;
use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Notify};

/// A program's app name is this and its id: `desktop_blender`.
pub const APP_PREFIX: &str = "desktop_";
/// Set on an ask that only a person can answer: the person's own rule
/// (policy asks first) or a tool that runs code inside a program. Case 4 of
/// the four asks.
pub const ASK_RULE: &str = "kernel:local_program";
/// How long a call waits for the computer to answer.
pub(crate) const JOB_WAIT: Duration = Duration::from_secs(120);
/// Within this of its last poll or report, a computer counts as online.
pub(crate) const ONLINE_GRACE: Duration = Duration::from_secs(30);
pub(crate) const MAX_RESULT_CHARS: usize = 512 * 1024;
pub(crate) const MAX_TOOLS_PER_PROGRAM: usize = 128;
pub(crate) const MAX_TOOL_NAME_CHARS: usize = 48;
pub(crate) const MAX_DESCRIPTION_CHARS: usize = 1024;
pub(crate) const MAX_SCHEMA_BYTES: usize = 16 * 1024;
/// A program's results are what it read on the person's computer: untrusted, and private.
const BOTH: Label = Label { untrusted: true, private: true };

/// A program the bridge supports. Detection data is the desktop app's
/// (`desktop/src-tauri/src/programs.rs`); this is what the server knows.
pub struct Program {
    pub id: &'static str,
    pub title: &'static str,
    /// Tools known not to run code, with their class. Every other tool of
    /// the program is taken to run code inside it: the safer reading.
    pub classified: &'static [(&'static str, Risk)],
}

/// The supported programs (docs/connectors.md, Programs). Phase 1 knows
/// them by id; each one's own tools come in its own phase.
pub const PROGRAMS: &[Program] = &[
    Program { id: "blender", title: "Blender", classified: &[] },
    Program { id: "freecad", title: "FreeCAD", classified: &[] },
    Program { id: "fusion", title: "Autodesk Fusion", classified: &[] },
    Program { id: "solidworks", title: "SolidWorks", classified: &[] },
];

pub fn program(id: &str) -> Option<&'static Program> {
    PROGRAMS.iter().find(|p| p.id == id)
}

/// The program a tool's app names: `desktop_blender` is Blender.
pub fn program_of_app(app: &str) -> Option<&'static Program> {
    app.strip_prefix(APP_PREFIX).and_then(program)
}

/// The tool that tells the agent, per computer, where each program stands.
pub const BUILTINS: &[Builtin] = &[Builtin { name: "desktop_programs", app: "desktop", risk: Risk::Read, label: Label::PRIVATE, audience: Audience::Private }];

/// Whether a program's tool runs code inside it.
pub fn executes_code(p: &Program, tool: &str) -> bool {
    !p.classified.iter().any(|(t, _)| *t == tool)
}

/// What the boundary knows of `desktop_<program>__<tool>`. A tool that runs
/// code is irreversible, so the default policy asks before it.
pub fn facts(app: &str, remote: &str) -> Option<Facts> {
    let p = program_of_app(app)?;
    if !valid_tool_name(remote) {
        return None;
    }
    let risk = p.classified.iter().find(|(t, _)| *t == remote).map_or(Risk::Irreversible, |(_, r)| *r);
    let risk = if risk == Risk::Sandbox { Risk::Irreversible } else { risk };
    Some(Facts { name: format!("{app}__{remote}"), app: app.into(), risk, label: BOTH, audience: Audience::Private, connector: None, remote: remote.into(), reaches_self: false })
}

pub fn valid_tool_name(t: &str) -> bool {
    !t.is_empty() && t.len() <= MAX_TOOL_NAME_CHARS && !t.contains("__") && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What the log keeps. A kind is never edited or deleted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// A computer's app announced itself under the id it made, with its name.
    Device { id: String, person: String, name: String },
    /// The person removed the computer: its consents go with it.
    Removed { id: String, person: String },
    /// "Use <program> on this computer": allowed, or taken back.
    Consent { device: String, person: String, program: String, allowed: bool },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub person: String,
    pub name: String,
    pub allowed: BTreeSet<String>,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    devices: BTreeMap<String, Device>,
}

/// Their computers' apps and the programs they allowed on each.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.devices.retain(|_, d| !who.is(&d.person));
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Device { id, person, name } => {
                let d = self.devices.entry(id.clone()).or_insert_with(|| Device { id: id.clone(), person: person.clone(), name: name.clone(), allowed: BTreeSet::new() });
                // An id is its first person's: another's report never takes it.
                if d.person == *person {
                    d.name = name.clone();
                }
            }
            Change::Removed { id, person } => {
                if self.devices.get(id).is_some_and(|d| d.person == *person) {
                    self.devices.remove(id);
                }
            }
            Change::Consent { device, person, program, allowed } => {
                if let Some(d) = self.devices.get_mut(device).filter(|d| d.person == *person) {
                    if *allowed {
                        d.allowed.insert(program.clone());
                    } else {
                        d.allowed.remove(program);
                    }
                }
            }
        }
    }

    pub fn of(&self, person: &str) -> Vec<Device> {
        self.devices.values().filter(|d| d.person == person).cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }
}

/// One of a program's tools, as its server lists it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReportedTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

/// What the app found of one program.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reported {
    pub program: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Its server answered on its loopback port.
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub tools: Vec<ReportedTool>,
}

/// Where one program stands on one computer (connectors spec, Report state).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgramState {
    NotInstalled,
    NotEnabled,
    Closed,
    Ready,
}

impl ProgramState {
    pub fn of(allowed: bool, online: bool, r: Option<&Reported>) -> ProgramState {
        match r {
            None => ProgramState::NotInstalled,
            Some(_) if !allowed => ProgramState::NotEnabled,
            Some(r) if !r.running || !online => ProgramState::Closed,
            Some(_) => ProgramState::Ready,
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            ProgramState::NotInstalled => "not installed",
            ProgramState::NotEnabled => "not enabled",
            ProgramState::Closed => "closed",
            ProgramState::Ready => "ready",
        }
    }
}

/// Only supported programs, named tools and bounded schemas are kept; the
/// rest of a report is dropped. Its text is the program's, so untrusted.
pub fn clean_report(rs: Vec<Reported>) -> Vec<Reported> {
    let mut seen = BTreeSet::new();
    rs.into_iter()
        .filter(|r| program(&r.program).is_some() && seen.insert(r.program.clone()))
        .map(|mut r| {
            r.version = r.version.map(|v| v.chars().take(40).collect());
            r.path = r.path.map(|v| v.chars().take(260).collect());
            let mut names = BTreeSet::new();
            r.tools.retain(|t| valid_tool_name(&t.name) && names.insert(t.name.clone()) && t.input_schema.to_string().len() <= MAX_SCHEMA_BYTES);
            r.tools.truncate(MAX_TOOLS_PER_PROGRAM);
            for t in &mut r.tools {
                t.description = t.description.chars().take(MAX_DESCRIPTION_CHARS).collect();
                if !t.input_schema.is_object() {
                    t.input_schema = json!({ "type": "object" });
                }
            }
            r
        })
        .collect()
}

/// A computer's line to the server: its last report, its polls and its calls.
#[derive(Default)]
pub(crate) struct Line {
    pub(crate) reported: Vec<Reported>,
    seen: Option<Instant>,
    polls: usize,
    pub(crate) jobs: VecDeque<Value>,
    waiting: BTreeMap<String, oneshot::Sender<(bool, String)>>,
    wake: Arc<Notify>,
}

static LINES: LazyLock<Mutex<BTreeMap<String, Line>>> = LazyLock::new(Default::default);

pub(crate) fn line<T>(id: &str, f: impl FnOnce(&mut Line) -> T) -> T {
    let mut all = LINES.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.entry(id.to_string()).or_default())
}

pub(crate) fn online(id: &str) -> bool {
    line(id, |l| l.polls > 0 || l.seen.is_some_and(|t| t.elapsed() < ONLINE_GRACE))
}

pub(crate) fn report(id: &str, rs: Vec<Reported>) {
    line(id, |l| {
        l.reported = clean_report(rs);
        l.seen = Some(Instant::now());
    });
}

/// Counts a poll in progress; dropped when the app's connection closes.
pub(crate) struct Polling(pub(crate) String);

impl Polling {
    pub(crate) fn start(id: &str) -> Polling {
        line(id, |l| l.polls += 1);
        Polling(id.to_string())
    }
}

impl Drop for Polling {
    fn drop(&mut self) {
        line(&self.0, |l| {
            l.polls = l.polls.saturating_sub(1);
            l.seen = Some(Instant::now());
        });
    }
}

/// The next call for the computer, waiting up to `wait` for one.
pub(crate) async fn next_job(id: &str, wait: Duration) -> Option<Value> {
    let until = Instant::now() + wait;
    loop {
        let (job, wake) = line(id, |l| (l.jobs.pop_front(), l.wake.clone()));
        if job.is_some() {
            return job;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || tokio::time::timeout(left, wake.notified()).await.is_err() {
            return None;
        }
    }
}

/// The computer's answer to a call, given to whoever waits for it.
pub(crate) fn answer(id: &str, job: &str, ok: bool, text: String) -> bool {
    let text: String = text.chars().take(MAX_RESULT_CHARS).collect();
    match line(id, |l| l.waiting.remove(job)) {
        Some(w) => w.send((ok, text)).is_ok(),
        None => false,
    }
}

/// Hands a call to one computer and waits for its answer; with none,
/// whether it was never taken.
pub(crate) async fn run_on(device: &str, mut job: Value, wait: Duration) -> (Option<(bool, String)>, bool) {
    let id = uuid::Uuid::new_v4().simple().to_string();
    job["id"] = json!(id);
    let (done, answer) = oneshot::channel();
    let wake = line(device, |l| {
        l.waiting.insert(id.clone(), done);
        l.jobs.push_back(job);
        l.wake.clone()
    });
    wake.notify_one();
    let result = tokio::time::timeout(wait, answer).await.ok().and_then(|r| r.ok());
    // Taken back so that it cannot run later, when nobody is waiting.
    let never_taken = line(device, |l| {
        l.waiting.remove(&id);
        let queued = l.jobs.iter().any(|j| j["id"] == json!(id));
        l.jobs.retain(|j| j["id"] != json!(id));
        queued
    });
    (result, never_taken)
}

/// One computer as the person and the agent see it.
#[derive(Serialize, Clone, Debug)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub programs: Vec<ProgramView>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProgramView {
    pub program: String,
    pub title: String,
    pub state: ProgramState,
    pub allowed: bool,
    pub version: Option<String>,
    #[serde(skip)]
    pub tools: Vec<ReportedTool>,
}

pub fn view(d: &Device, online: bool, reported: &[Reported]) -> DeviceView {
    let programs = PROGRAMS
        .iter()
        .map(|p| {
            let r = reported.iter().find(|r| r.program == p.id);
            let allowed = d.allowed.contains(p.id);
            let state = ProgramState::of(allowed, online, r);
            let tools = if state == ProgramState::Ready { r.map(|r| r.tools.clone()).unwrap_or_default() } else { vec![] };
            ProgramView { program: p.id.into(), title: p.title.into(), state, allowed, version: r.and_then(|r| r.version.clone()), tools }
        })
        .collect();
    DeviceView { id: d.id.clone(), name: d.name.clone(), online, programs }
}

/// The computer a call goes to: the one named, or the only one where the
/// program is ready. Refused, with where it stands, when there is none.
pub fn pick(views: &[DeviceView], program: &str, named: Option<&str>) -> std::result::Result<String, String> {
    let title = crate::desktop::program(program).map_or(program, |p| p.title);
    if views.is_empty() {
        return Err(format!("no computer of the person runs the Magnus app, so {title} cannot be reached"));
    }
    let on = |v: &DeviceView| v.programs.iter().find(|p| p.program == program).map_or(ProgramState::NotInstalled, |p| p.state);
    let chosen: Vec<&DeviceView> = match named {
        Some(n) => views.iter().filter(|v| v.name == n).collect(),
        None => views.iter().filter(|v| on(v) == ProgramState::Ready).collect(),
    };
    match chosen.as_slice() {
        [] if named.is_some() => Err(format!("no computer of the person is called {}", named.unwrap_or_default())),
        [] => {
            let where_ = views.iter().map(|v| format!("{} on {}", on(v).words(), v.name)).collect::<Vec<_>>().join(", ");
            Err(format!("{title} is not ready on any of the person's computers ({where_}). The person enables it in Apps on that computer, and opens it"))
        }
        [one] => match on(one) {
            ProgramState::Ready => Ok(one.id.clone()),
            ProgramState::NotEnabled => Err(format!("{title} is not enabled on {}: the person turns it on in Apps there", one.name)),
            s => Err(format!("{title} is {} on {}", s.words(), one.name)),
        },
        many => Err(format!("{title} is ready on more than one computer: name one in computer ({})", many.iter().map(|v| v.name.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// What a policy decision means for a program's tool. Policy's own ask
/// stands here as a person's ask: the default asks before code runs, and
/// a person's forbid of `execute` makes a program's tool ask them first.
pub fn ask_of(outcome: &Outcome, f: &Facts) -> Option<std::result::Result<(String, Option<String>), String>> {
    let p = program_of_app(&f.app)?;
    match outcome {
        Outcome::Allow { .. } => None,
        Outcome::Deny { reason, .. } => Some(Err(reason.clone())),
        Outcome::Ask { .. } if executes_code(p, &f.remote) => Some(Ok((format!("{} runs code inside {} on the person's computer", f.remote, p.title), Some(ASK_RULE.to_string())))),
        Outcome::Ask { .. } => Some(Ok((format!("policy asks before {} in {}", f.remote, p.title), Some(ASK_RULE.to_string())))),
    }
}

impl Kernel {
    pub(crate) fn desktop_views(&self, person: &str) -> Vec<DeviceView> {
        self.read(|s| s.local.desktop.of(person)).iter().map(|d| view(d, online(&d.id), &line(&d.id, |l| l.reported.clone()))).collect()
    }

    /// For a program's tool: no computer to take it refuses it, and the
    /// policy's ask asks the person.
    pub(crate) fn desktop_gate(&self, a: &Action, f: &Facts, person: &Person) -> Option<std::result::Result<(String, Option<String>), String>> {
        let p = program_of_app(&f.app)?;
        if let Err(why) = pick(&self.desktop_views(&person.id), p.id, a.input["computer"].as_str()) {
            return Some(Err(why));
        }
        let run = self.read(|s| s.runs.get(&a.run).cloned())?;
        let policies = self.read(|s| s.policies.clone());
        match crate::policy::decide(&policies, person, Some(&run.task), &ToolFacts::from(f), run.untrusted, run.private) {
            Ok(outcome) => ask_of(&outcome, f),
            Err(e) => Some(Err(format!("policy could not decide: {e:#}"))),
        }
    }

    /// Carry out a program's tool, or say where each program stands.
    pub(crate) async fn desktop_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let Some(person) = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone())) else { return Ok(("no such run".into(), true, None)) };
        if a.tool == "desktop_programs" {
            return Ok((programs_text(&self.desktop_views(&person)), false, None));
        }
        let Some((app, remote)) = a.tool.split_once("__") else { return Ok((format!("{} is not a program's tool", a.tool), true, None)) };
        let Some(p) = program_of_app(app) else { return Ok((format!("{app} is not a supported program"), true, None)) };
        Ok(self.desktop_call(&person, p, remote, &a.input).await)
    }

    /// Route one call to the right computer's bridge and wait for it.
    pub(crate) async fn desktop_call(&self, person: &str, p: &Program, tool: &str, input: &Value) -> (String, bool, Option<Value>) {
        let device = match pick(&self.desktop_views(person), p.id, input["computer"].as_str()) {
            Ok(d) => d,
            Err(why) => return (why, true, None),
        };
        let mut arguments = input.clone();
        if let Some(o) = arguments.as_object_mut() {
            o.remove("computer");
        }
        let job = json!({ "program": p.id, "tool": tool, "arguments": arguments });
        let name = self.read(|s| s.local.desktop.get(&device).map(|d| d.name.clone())).unwrap_or_default();
        match run_on(&device, job, JOB_WAIT).await {
            (Some((ok, text)), _) => (text, !ok, None),
            (None, true) => (format!("{name} did not take the call, so nothing ran in {}", p.title), true, None),
            (None, false) => (format!("{name} did not answer, so this may or may not have happened in {}", p.title), true, None),
        }
    }

    /// The tools of each program ready on one of the person's computers,
    /// and the one that says where each stands.
    pub(crate) fn desktop_specs(&self, person: &str) -> Vec<ToolSpec> {
        let views = self.desktop_views(person);
        if views.is_empty() {
            return vec![];
        }
        let mut v = vec![ToolSpec {
            name: "desktop_programs".into(),
            description: "Where each supported program stands on each of the person's computers: not installed, not enabled, closed or ready.".into(),
            input_schema: obj(json!({}), &[]),
        }];
        v.extend(specs(&views));
        v
    }
}

/// One spec per ready program's tool, across computers, with `computer`
/// added to name one.
pub fn specs(views: &[DeviceView]) -> Vec<ToolSpec> {
    let mut by_name: BTreeMap<String, (ToolSpec, Vec<String>)> = BTreeMap::new();
    for v in views {
        for p in v.programs.iter().filter(|p| p.state == ProgramState::Ready) {
            for t in &p.tools {
                let name = format!("{APP_PREFIX}{}__{}", p.program, t.name);
                let entry = by_name.entry(name.clone()).or_insert_with(|| {
                    let mut schema = t.input_schema.clone();
                    if schema.get("type").is_none() {
                        schema["type"] = json!("object");
                    }
                    let description = format!("In {} on the person's computer: {}", p.title, t.description);
                    (ToolSpec { name, description, input_schema: schema }, vec![])
                });
                entry.1.push(v.name.clone());
            }
        }
    }
    by_name
        .into_values()
        .map(|(mut s, computers)| {
            if let Some(props) = s.input_schema.as_object_mut().map(|o| o.entry("properties").or_insert_with(|| json!({}))) {
                if !props.is_object() {
                    *props = json!({});
                }
                props["computer"] = json!({ "type": "string", "enum": computers, "description": "which computer, when it is ready on more than one" });
            }
            s
        })
        .collect()
}

pub fn programs_text(views: &[DeviceView]) -> String {
    if views.is_empty() {
        return "No computer of the person runs the Magnus app.".into();
    }
    views
        .iter()
        .map(|v| {
            let ps = v.programs.iter().map(|p| format!("{}: {}", p.title, p.state.words())).collect::<Vec<_>>().join("; ");
            format!("{}{}: {ps}", v.name, if v.online { "" } else { " (not connected now)" })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
