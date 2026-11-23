//! One entry of a person's thread, typed until it leaves the server: what
//! the thread route, a branch's copy and a shared item carry is its
//! serialization (`Entry::value`), and everything the server reads from an
//! entry (its text, who said it, the tasks it shows, the line the model
//! reads) is a method here, not a field looked up in JSON.
//!
//! Two bodies stay JSON, each for its reason: a card's fields are the
//! model's (`suggest`, `offer_app`), with the server's answer added; and a
//! branch's copied entries are stored in `ThreadBranched` as they were shown
//! when the branch was made, so they replay as stored and are never typed
//! again (an event kind is never edited). Both are read by `map_text` and
//! `map_line`, the one reader for an entry in JSON form.
use crate::chat_entry::{ChatEntry, EntryFile, May};
use serde::{Serialize, Serializer};
use serde_json::{json, Map, Value};

type At = chrono::DateTime<chrono::Utc>;

/// What an entry is. It travels as today's strings (`as_str`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Message,
    Opener,
    Answer,
    /// A turn that has not answered: running, waiting, failed or stopped.
    Turn,
    Result,
    Tasks,
    SubAgents,
    SuggestionCard,
    ScheduleCard,
    GrantCard,
    ConnectorCard,
    /// A kind this server does not make: only in a branch's copy.
    Other,
}

impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::Message,
        Kind::Opener,
        Kind::Answer,
        Kind::Turn,
        Kind::Result,
        Kind::Tasks,
        Kind::SubAgents,
        Kind::SuggestionCard,
        Kind::ScheduleCard,
        Kind::GrantCard,
        Kind::ConnectorCard,
        Kind::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Message => "message",
            Kind::Opener => "opener",
            Kind::Answer => "answer",
            Kind::Turn => "turn",
            Kind::Result => "result",
            Kind::Tasks => "tasks",
            Kind::SubAgents => "sub_agents",
            Kind::SuggestionCard => "suggestion_card",
            Kind::ScheduleCard => "schedule_card",
            Kind::GrantCard => "grant_card",
            Kind::ConnectorCard => "connector_card",
            Kind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| *k != Kind::Other && k.as_str() == s).unwrap_or(Kind::Other)
    }

    /// Magnus's messages: its answers (or what it said when it could not
    /// answer), its tasks' results, the tasks a turn started, the cards it
    /// proposed (#267) and its own openings (#359). The one list: what takes
    /// a reaction and what the person may judge. The person's own messages
    /// are not Magnus's to be judged on.
    pub fn is_magnus(self) -> bool {
        matches!(self, Kind::Answer | Kind::Turn | Kind::Result | Kind::Tasks | Kind::GrantCard | Kind::SuggestionCard | Kind::ScheduleCard | Kind::ConnectorCard | Kind::Opener)
    }
}

impl Serialize for Kind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// A turn without its answer: where it stands.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Turn {
    pub status: &'static str,
    pub waiting_on: Option<String>,
    pub waiting_item: Option<String>,
    /// What it said so far.
    pub text: Option<String>,
}

/// A task's result, posted to the thread that started it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TaskResult {
    pub title: String,
    /// What was asked.
    pub text: String,
    pub status: &'static str,
    pub summary: Option<String>,
    pub view: Option<Value>,
    pub files: Vec<EntryFile>,
    /// The person's verdict on it.
    pub verdict: Option<Value>,
    /// What it did and what it rests on (`evidence.rs`).
    pub evidence: Value,
}

/// One sub-agent of a turn's, nested ones too.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Agent {
    pub run: String,
    pub parent: String,
    pub depth: usize,
    pub task_id: String,
    pub goal: String,
    pub status: &'static str,
    pub waiting_on: Option<String>,
    pub waiting_item: Option<String>,
    pub summary: Option<String>,
    pub tools: Vec<String>,
    pub at: At,
    pub ended_at: Option<At>,
}

/// The sub-agents a turn started.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SubAgents {
    pub from_run: String,
    pub agents: Vec<Agent>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    /// A message, an opener, an answer or the tasks a message started: the
    /// entry both chats share.
    Said(Box<ChatEntry>),
    Turn(Turn),
    Result(Box<TaskResult>),
    SubAgents(SubAgents),
    /// A card: the model's fields with the server's answer added.
    Card(Map<String, Value>),
    /// A branch's copy of an entry, as stored when the branch was made.
    Copied(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// What the message actions name.
    pub id: String,
    pub kind: Kind,
    /// When it was said, as it shows.
    pub at: Option<At>,
    pub task_id: Option<String>,
    /// What the person may do with it.
    pub may: Option<May>,
    /// The person's reaction, shown to them alone.
    pub reaction: Option<String>,
    pub body: Body,
}

impl Entry {
    /// A message, opener, answer or tasks entry.
    pub fn said(c: ChatEntry) -> Entry {
        Entry { id: c.id.clone(), kind: Kind::parse(c.kind), at: c.at, task_id: c.task_id.clone(), may: None, reaction: None, body: Body::Said(Box::new(c)) }
    }

    /// A card from its fields, the server's answer already in them.
    pub fn card(id: String, kind: Kind, task_id: String, data: Map<String, Value>) -> Entry {
        let at = data.get("at").and_then(Value::as_str).and_then(|a| a.parse().ok());
        Entry { id, kind, at, task_id: Some(task_id), may: None, reaction: None, body: Body::Card(data) }
    }

    /// A branch's stored entry, as it was then, marked as copied.
    pub fn copied(v: Value) -> Entry {
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        Entry {
            id: s("id").unwrap_or_default().to_string(),
            kind: s("kind").map_or(Kind::Other, Kind::parse),
            at: s("at").and_then(|a| a.parse().ok()),
            task_id: s("task_id").map(String::from),
            may: None,
            reaction: None,
            body: Body::Copied(v),
        }
    }

    /// The entry's fields in JSON, when it is JSON: a card or a copy.
    fn map(&self) -> Option<&Map<String, Value>> {
        match &self.body {
            Body::Card(m) => Some(m),
            Body::Copied(v) => v.as_object(),
            _ => None,
        }
    }

    /// Whether it is one of Magnus's messages.
    pub fn is_magnus(&self) -> bool {
        self.kind.is_magnus()
    }

    /// A copy stored with an id, which the message actions can name.
    pub fn has_id(&self) -> bool {
        match &self.body {
            Body::Copied(v) => v.get("id").is_some_and(Value::is_string),
            _ => true,
        }
    }

    /// What it says, as text for a person to read.
    pub fn text(&self) -> String {
        match &self.body {
            Body::Said(c) if self.kind == Kind::Tasks => c.tasks.iter().flatten().map(|t| format!("- {}: {}", t.title, t.text)).collect::<Vec<_>>().join("\n"),
            Body::Said(c) => c.text.clone(),
            Body::Turn(t) => t.text.clone().unwrap_or_default(),
            Body::Result(r) => format!("{} ({})\n\n{}", r.title, r.status, r.summary.as_deref().unwrap_or_default()),
            Body::SubAgents(_) => String::new(),
            Body::Card(m) => map_text(self.kind, m),
            Body::Copied(v) => v.as_object().map(|m| map_text(self.kind, m)).unwrap_or_default(),
        }
    }

    /// Who said it, for a person reading it.
    pub fn speaker(&self) -> &'static str {
        match self.kind {
            Kind::Message => "You",
            Kind::Result => "Task result",
            _ => "Magnus",
        }
    }

    /// When it was said, as the app shows it.
    pub fn at_text(&self) -> String {
        self.at.map(|a| a.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)).unwrap_or_default()
    }

    /// The entry as a Markdown file's contents.
    pub fn markdown(&self) -> String {
        crate::message_file::markdown(self.speaker(), &self.at_text(), &self.text())
    }

    /// The name that file takes.
    pub fn file_name(&self) -> String {
        crate::message_file::file_name(&self.text())
    }

    /// The tasks whose runs wrote what it shows.
    pub fn task_ids(&self) -> Vec<String> {
        match &self.body {
            Body::Said(c) if self.kind == Kind::Tasks => c.tasks.iter().flatten().map(|t| t.task_id.clone()).collect(),
            Body::Copied(v) if self.kind == Kind::Tasks => v["tasks"].as_array().into_iter().flatten().filter_map(|t| t["task_id"].as_str().map(String::from)).collect(),
            _ => self.task_id.iter().cloned().collect(),
        }
    }

    /// The turn's run that started what it shows: for started tasks and sub-agents.
    pub fn from_run(&self) -> Option<&str> {
        match &self.body {
            Body::Said(c) => c.from_run.as_deref(),
            Body::SubAgents(s) => Some(&s.from_run),
            Body::Copied(v) => v["from_run"].as_str(),
            _ => None,
        }
    }

    /// What an answer showed below its text.
    pub fn view(&self) -> Option<&Value> {
        match &self.body {
            Body::Said(c) => c.view.as_ref(),
            Body::Result(r) => r.view.as_ref(),
            _ => self.map().and_then(|m| m.get("view")),
        }
    }

    /// An answer's words written for listening, once written.
    pub fn spoken(&self) -> Option<&str> {
        match &self.body {
            Body::Said(c) => c.spoken.as_deref(),
            _ => self.map().and_then(|m| m.get("spoken")).and_then(Value::as_str),
        }
    }

    /// A card's fields.
    pub fn card_data(&self) -> Option<&Map<String, Value>> {
        match &self.body {
            Body::Card(m) => Some(m),
            _ => None,
        }
    }

    /// The words a search looks in: its text, else its summary.
    pub fn said_text(&self) -> Option<&str> {
        match &self.body {
            Body::Said(c) => Some(&c.text),
            Body::Turn(t) => t.text.as_deref(),
            Body::Result(r) => Some(&r.text),
            Body::SubAgents(_) => None,
            Body::Card(_) | Body::Copied(_) => {
                let m = self.map()?;
                m.get("text").and_then(Value::as_str).or(m.get("summary").and_then(Value::as_str))
            }
        }
    }

    /// The entry as the model reads it in the conversation: who said what.
    pub fn transcript_line(&self) -> Option<(&'static str, String)> {
        let shown = |text: &str, view: Option<&Value>| match view.filter(|v| v.is_array()) {
            Some(v) => format!("{text}\n[Shown below the answer: {v}]"),
            None => text.to_string(),
        };
        match &self.body {
            Body::Said(c) => match self.kind {
                Kind::Message => Some(("person", c.text.clone())),
                // Its instruction was the server's: only that Magnus opened it.
                Kind::Opener => Some(("you", format!("(You opened this conversation yourself: {}.)", c.text))),
                // What the answer showed below its text is part of what it said.
                Kind::Answer => Some(("you", shown(&c.text, c.view.as_ref()))),
                Kind::Tasks => {
                    let rows: Vec<String> = c.tasks.iter().flatten().map(|t| format!("{} ({})", t.title, t.text)).collect();
                    Some(("you", format!("Started tasks: {}", rows.join("; "))))
                }
                _ => None,
            },
            Body::Result(r) => Some(("task result", format!("{} ({}): {}", r.title, r.status, r.summary.as_deref().unwrap_or_default()))),
            Body::Turn(_) | Body::SubAgents(_) => None,
            Body::Card(m) => map_line(self.kind, m),
            Body::Copied(v) => map_line(self.kind, v.as_object()?),
        }
    }

    /// The entry as it travels: to the app, into a branch's copy.
    pub fn value(&self) -> Value {
        let to = |v: Value| match v {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let mut m = match &self.body {
            Body::Said(c) => to(c.value()),
            Body::Turn(t) => to(json!(t)),
            Body::Result(r) => to(json!(r)),
            Body::SubAgents(s) => to(json!(s)),
            Body::Card(d) => d.clone(),
            Body::Copied(v) => {
                let mut m = to(v.clone());
                m.insert("copied".into(), json!(true));
                m
            }
        };
        if !matches!(self.body, Body::Copied(_) | Body::Said(_)) {
            m.insert("id".into(), json!(self.id));
            m.insert("kind".into(), json!(self.kind));
            if let Some(t) = &self.task_id {
                m.insert("task_id".into(), json!(t));
            }
            if !matches!(self.body, Body::Card(_)) {
                m.insert("at".into(), json!(self.at));
            }
        }
        if let Some(may) = &self.may {
            m.insert("may".into(), json!(may));
        }
        if let Some(r) = &self.reaction {
            m.insert("reaction".into(), json!(r));
        }
        Value::Object(m)
    }
}

impl Serialize for Entry {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.value().serialize(s)
    }
}

/// An entry in JSON form as text for a person to read.
fn map_text(kind: Kind, m: &Map<String, Value>) -> String {
    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match kind {
        Kind::Result => format!("{} ({})\n\n{}", s("title"), s("status"), s("summary")),
        Kind::Tasks => m
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| format!("- {}: {}", t["title"].as_str().unwrap_or_default(), t["text"].as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => s("text"),
    }
}

/// An entry in JSON form as the model reads it: who said what.
fn map_line(kind: Kind, m: &Map<String, Value>) -> Option<(&'static str, String)> {
    let null = Value::Null;
    let e = |k: &str| m.get(k).unwrap_or(&null);
    let text = |k: &str| e(k).as_str().unwrap_or_default().to_string();
    Some(match kind {
        Kind::Message => ("person", text("text")),
        Kind::Opener => ("you", format!("(You opened this conversation yourself: {}.)", text("text"))),
        Kind::Answer if e("view").is_array() => ("you", format!("{}\n[Shown below the answer: {}]", text("text"), e("view"))),
        Kind::Answer => ("you", text("text")),
        Kind::Tasks => (
            "you",
            format!(
                "Started tasks: {}",
                e("tasks")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|t| format!("{} ({})", t["title"].as_str().unwrap_or_default(), t["text"].as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ),
        Kind::Result => ("task result", format!("{} ({}): {}", text("title"), text("status"), text("summary"))),
        Kind::ScheduleCard => ("you", format!("Set up the schedule {} at their request: {}", text("schedule"), text("text"))),
        Kind::SuggestionCard => {
            let changed = e("changed").as_str().map(|c| format!(" (again: {c})")).unwrap_or_default();
            ("you", format!("Suggested {} ({}, card {}): {}{changed}", text("title"), answered(m), text("card"), text("plan")))
        }
        Kind::GrantCard => {
            let list = |k: &str| e(k).as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
            let id = e("granted").as_str().map(|g| format!(", grant {g}")).unwrap_or_default();
            (
                "you",
                format!(
                    "Proposed a grant {} ({}, card {}{id}): apps {}; still asks before {}; up to ${} a {}",
                    text("title"),
                    answered(m),
                    text("card"),
                    list("apps"),
                    list("still_asks"),
                    e("max_usd"),
                    text("period")
                ),
            )
        }
        Kind::ConnectorCard => ("you", format!("Offered the app {} ({}): {}", text("title"), text("state"), text("why"))),
        Kind::Turn | Kind::SubAgents | Kind::Other => return None,
    })
}

/// A card's answer as the model reads it in the conversation.
fn answered(m: &Map<String, Value>) -> String {
    match (m.get("state").and_then(Value::as_str).unwrap_or_default(), m.get("reason").and_then(Value::as_str)) {
        ("declined", Some(r)) => format!("declined: \"{r}\""),
        ("later", _) => "not now".into(),
        (s, _) => s.into(),
    }
}
