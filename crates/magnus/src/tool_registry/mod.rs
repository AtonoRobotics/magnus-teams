//! Every built-in tool of a run, a chat turn and a team thread, declared once
//! (decisions/2026-11-14-a-tool-is-declared-once.md): its name, what the
//! boundary knows of it, what carries it out, when it is offered, its input
//! schema, what a card says while it runs, and the facts the web reads, the
//! boundary's labels and the team agent read of it. `tools::BUILTINS`, the
//! specs, `tools_core::op`, the evidence wording, the web-read sources and
//! the team's tool table all derive from here. Its description is the one
//! under its name in `instructions/tools.json`, the versioned text (X11).
//!
//! A tool's place in its list is its place in the specs the model is sent,
//! which `tool_specs_pin` holds byte for byte.
use crate::team::TeamOp;
use crate::tools::{Audience, Builtin, Label, Risk};
use crate::tools_core::Op;
use magnus_adapter_proto::ToolSpec;
use serde_json::Value;
use std::sync::LazyLock;

mod chat_tools;
mod run_tools;
mod team_tools;
#[cfg(test)]
mod tests;

/// A description by its key in `instructions/tools.json`.
pub(crate) type Describe<'a> = dyn Fn(&str) -> String + 'a;

/// What a run's server must have for a run tool to be offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    Always,
    Search,
    Sandbox,
    Browser,
    Computer,
}

/// Which list of specs a tool is offered in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// `tools::builtin_specs`, when the server has what it needs.
    Run(Need),
    /// `tools::chat_specs`.
    Chat,
    /// `tools::find_tools_spec`, past `LOAD_ALL` app tools.
    FindTools,
    /// `team::specs`.
    Team,
}

/// What carries a tool out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exec {
    /// A run record the loop writes to the log itself (K21).
    Record,
    Core(Op),
    Team(TeamOp),
}

/// What a web or browser read sends and returns
/// (decisions/2026-11-12-web-reads-by-provenance.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebRead {
    No,
    /// A search: its query passes `query_gate`; its results hold addresses.
    Query,
    /// Isolated OpenWeb research bridge: query passes `query_gate` inside `research_web`.
    Research,
    /// Opens the address in its `url`; what it returns holds addresses.
    OpensUrl,
    /// Reads the page already open, and returns its text.
    OpenPage,
    /// A picture of the page already open.
    Picture,
}

/// Where a result's label comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taint {
    /// The tool's own label.
    Declared,
    /// What memory returned, as it was written (M1).
    Memory,
    /// What the result says of itself; one that says nothing is untrusted and private.
    Returned,
    /// The reference file's own (M1, M2).
    ProjectFile,
    /// What the listed runs read.
    Listed,
}

/// What the boundary knows of a tool: its app, risk class, inbound label and audience.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Class {
    pub app: &'static str,
    pub risk: Risk,
    pub label: Label,
    pub audience: Audience,
}

impl Class {
    pub(crate) const fn new(app: &'static str, risk: Risk, label: Label, audience: Audience) -> Class {
        Class { app, risk, label, audience }
    }
}

/// One tool, declared once.
pub(crate) struct Decl {
    pub name: &'static str,
    /// None for a run record, which never passes the boundary.
    pub class: Option<Class>,
    pub exec: Exec,
    pub offer: Offer,
    pub schema: fn(&Describe) -> Value,
    /// What a card says while this is the last thing done; else the
    /// name's own words.
    pub doing: Option<&'static str>,
    pub web: WebRead,
    pub taint: Taint,
    /// Serves a team thread (spec rev 142: nothing personal to a member does).
    pub in_team: bool,
    /// Its calls are its person's alone (#232, #233, #257).
    pub person_only: bool,
}

impl Decl {
    /// A tool that passes the boundary.
    pub(crate) const fn tool(name: &'static str, class: Class, exec: Exec, offer: Offer, schema: fn(&Describe) -> Value) -> Decl {
        Decl { name, class: Some(class), exec, offer, schema, doing: None, web: WebRead::No, taint: Taint::Declared, in_team: false, person_only: false }
    }
    /// A run record.
    pub(crate) const fn record(name: &'static str, offer: Offer, schema: fn(&Describe) -> Value) -> Decl {
        Decl { name, class: None, exec: Exec::Record, offer, schema, doing: None, web: WebRead::No, taint: Taint::Declared, in_team: false, person_only: false }
    }
    pub(crate) const fn doing(mut self, words: &'static str) -> Decl {
        self.doing = Some(words);
        self
    }
    pub(crate) const fn web(mut self, w: WebRead) -> Decl {
        self.web = w;
        self
    }
    pub(crate) const fn taint(mut self, t: Taint) -> Decl {
        self.taint = t;
        self
    }
    pub(crate) const fn in_team(mut self) -> Decl {
        self.in_team = true;
        self
    }
    pub(crate) const fn person_only(mut self) -> Decl {
        self.person_only = true;
        self
    }
}

/// Every declared tool, in the order its list offers it.
pub(crate) fn all() -> impl Iterator<Item = &'static Decl> {
    run_tools::DECLS.iter().chain(chat_tools::DECLS).chain(team_tools::DECLS)
}

/// The tool declared under this name.
pub(crate) fn decl(name: &str) -> Option<&'static Decl> {
    all().find(|d| d.name == name)
}

/// The core tools, with what the boundary knows of them.
pub(crate) static CORE_BUILTINS: LazyLock<Vec<Builtin>> = LazyLock::new(|| builtins(|e| matches!(e, Exec::Core(_))));
/// The team workspace's tools.
pub(crate) static TEAM_BUILTINS: LazyLock<Vec<Builtin>> = LazyLock::new(|| builtins(|e| matches!(e, Exec::Team(_))));

fn builtins(of: fn(Exec) -> bool) -> Vec<Builtin> {
    all().filter(|d| of(d.exec)).filter_map(|d| d.class.map(|c| Builtin { name: d.name, app: c.app, risk: c.risk, label: c.label, audience: c.audience })).collect()
}

/// The run records declared here.
pub(crate) fn records() -> impl Iterator<Item = &'static str> {
    all().filter(|d| d.exec == Exec::Record).map(|d| d.name)
}

/// The op that carries out a core tool.
pub(crate) fn op(name: &str) -> Option<Op> {
    match decl(name)?.exec {
        Exec::Core(op) => Some(op),
        _ => None,
    }
}

/// The op that carries out a workspace's tool.
pub(crate) fn team_op(name: &str) -> Option<TeamOp> {
    match decl(name)?.exec {
        Exec::Team(op) => Some(op),
        _ => None,
    }
}

pub(crate) fn web_read(name: &str) -> WebRead {
    decl(name).map_or(WebRead::No, |d| d.web)
}

pub(crate) fn taint(name: &str) -> Taint {
    decl(name).map_or(Taint::Declared, |d| d.taint)
}

/// Whether this is the browser opening an address, from which the page's host is read.
pub(crate) fn navigates(name: &str) -> bool {
    decl(name).is_some_and(|d| d.web == WebRead::OpensUrl && d.class.is_some_and(|c| c.app == "browser"))
}

/// `instructions/tools.json`. It is compiled in and parsed by a test; were it
/// broken, the tools would go out without descriptions rather than the server stop.
pub(crate) fn descriptions() -> Value {
    serde_json::from_str(include_str!("../../instructions/tools.json")).unwrap_or_else(|e| {
        tracing::error!("instructions/tools.json does not parse: {e}");
        Value::Null
    })
}

/// The specs of the tools offered in the lists `offered` admits, in order.
pub(crate) fn specs(offered: impl Fn(Offer) -> bool) -> Vec<ToolSpec> {
    let desc = descriptions();
    let d = |n: &str| desc[n].as_str().unwrap_or_default().to_string();
    all().filter(|t| offered(t.offer)).map(|t| ToolSpec { name: t.name.into(), description: d(t.name), input_schema: (t.schema)(&d) }).collect()
}

/// What a tool call is doing, in a few plain words: a card's live line and
/// a team thread's work row.
pub(crate) fn doing_words(tool: &str) -> String {
    if let Some(words) = decl(tool).and_then(|d| d.doing) {
        return words.to_string();
    }
    let (app, name) = tool.split_once("__").map_or((None, tool), |(a, r)| (Some(a), r));
    let verb = capitalised(&name.replace('_', " "));
    match app {
        Some(a) => format!("{verb} in {a}"),
        None => verb,
    }
}

/// The first letter upper case, whatever its width in bytes.
fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
