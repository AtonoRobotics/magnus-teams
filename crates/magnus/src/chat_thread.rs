//! A person's thread as they see it, derived from the log: its turns, the
//! tasks they started with their results, the sub-agents at work in it, the
//! cards Magnus put to the person with what they decided, and a branch's
//! copy of the conversation it was made from. Each entry is typed
//! (`entry::Entry`) until it leaves the server.
pub mod entry;

pub use entry::{Agent, Body, Entry, Kind, SubAgents, TaskResult, Turn};

use crate::chat_entry::{ChatEntry, EntryFile, May, TaskRow};
use crate::kernel::Kernel;
use crate::recall::Words;
use crate::state::{Run, State, Status, Task};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// An entry with where it goes: when, then its place among those at that time.
type Placed = (DateTime<Utc>, i64, Entry);

impl Kernel {
    /// A conversation in the trash or removed from it. The tasks it started
    /// stay, and name it still.
    pub(crate) fn thread_gone(&self, thread: &str) -> bool {
        self.read(|s| s.trash.contains_key(thread) || s.removed_threads.contains_key(thread))
    }

    /// A thread's turns and tasks, in the order they were created.
    pub(crate) fn thread(&self, person: &str, thread: &str) -> Vec<(Task, Run)> {
        let mut v: Vec<(Task, Run)> = self.read(|s| {
            s.tasks
                .values()
                .filter(|t| t.person == person && t.chat.as_ref().is_some_and(|c| c.thread == thread))
                .filter_map(|t| s.runs.get(&t.run_id).map(|r| (t.clone(), r.clone())))
                .collect()
        });
        v.sort_by_key(|(t, _)| t.created_at);
        v
    }

    /// The thread as the person sees it, from the log.
    /// Each entry has an `id`, which the message actions name; entries the
    /// person deleted (in the trash or removed from it) or replaced are left out.
    /// What the person said in it is recalled for them.
    pub async fn thread_entries(&self, person: &str, thread: &str) -> anyhow::Result<Option<Vec<Entry>>> {
        if self.thread_gone(thread) {
            return Ok(None);
        }
        let w = self.person_words(person).await?;
        Ok(self.entries_with(person, thread, &w))
    }

    pub(crate) fn entries_with(&self, person: &str, thread: &str, w: &Words) -> Option<Vec<Entry>> {
        let runs = self.thread(person, thread);
        let branch = self.read(|s| s.thread_extras.branches().get(thread).filter(|b| b.person == person).cloned());
        if runs.is_empty() && branch.is_none() {
            return None;
        }
        let mut out: Vec<Placed> = vec![];
        // A branch starts with the conversation it was made from, as it was then.
        if let Some(b) = &branch {
            for (n, e) in b.entries.iter().enumerate() {
                let e = Entry::copied(e.clone());
                out.push((e.at.unwrap_or(b.at), n as i64 - i64::MAX / 2, e));
            }
        }
        // The tasks one message started, as one card in the order they began.
        let mut started: BTreeMap<String, (DateTime<Utc>, Vec<TaskRow>)> = Default::default();
        // A task's result follows the reply of the message that started it, even when the task finished first.
        let answered: HashMap<String, DateTime<Utc>> = runs.iter().filter(|(t, _)| t.is_chat_turn()).filter_map(|(_, r)| r.ended_at.map(|e| (r.id.clone(), e))).collect();
        for (t, r) in &runs {
            if t.is_chat_turn() {
                out.extend(self.turn_message(t, w));
                out.extend(self.turn_cards(person, t, r));
                out.extend(self.sub_agents_entry(t, r, w));
                out.push(self.turn_reply(t, r));
            } else {
                let title = crate::api::title_of(t, Some(r), w);
                let from = t.chat.as_ref().and_then(|c| c.from_run.clone()).unwrap_or_else(|| t.task_id.clone());
                let e = started.entry(from).or_insert((t.created_at, vec![]));
                e.1.push(TaskRow {
                    task_id: t.task_id.clone(),
                    title: title.clone(),
                    text: w.text(t.seq).to_string(),
                    status: r.status,
                    waiting_on: r.reason.clone(),
                    waiting_item: r.waiting_item.clone(),
                    status_line: crate::evidence::status_line(r),
                    requester: None,
                    steps: None,
                    doing: None,
                    long: None,
                    thread: None,
                    started: None,
                });
                if let Some(end) = r.ended_at {
                    let after = t.chat.as_ref().and_then(|c| c.from_run.as_ref()).and_then(|f| answered.get(f)).map_or(end, |a| end.max(*a));
                    out.push((after, i64::MAX, self.result_entry(t, r, title, w, end)));
                }
            }
        }
        for (from, (at, tasks)) in started {
            let e = ChatEntry { id: format!("t:{from}"), kind: Kind::Tasks.as_str(), at: Some(at), from_run: Some(from), tasks: Some(tasks), ..Default::default() };
            out.push((at, 1, Entry::said(e)));
        }
        out.sort_by_key(|x| (x.0, x.1));
        let (hidden, trashed) = self.read(|s| {
            (
                s.thread_extras.hidden(thread).cloned().unwrap_or_default(),
                s.trash.iter().filter(|(_, t)| t.thread() == Some(thread)).map(|(id, _)| id.clone()).collect::<HashSet<String>>(),
            )
        });
        // What a hidden answer started is hidden with it, whenever it shows
        // up (a task's result after the hiding), so its words and taint
        // never come back into the thread.
        let hidden_turns: HashSet<String> = hidden.iter().filter_map(|id| id.strip_prefix("a:").map(String::from)).collect();
        let shown = |e: &Entry| {
            let gone = e.has_id() && (hidden.contains(&e.id) || trashed.contains(&e.id));
            !gone && !self.origin_turn(e).is_some_and(|t| hidden_turns.contains(&t))
        };
        let with_may = |mut e: Entry| {
            if e.kind != Kind::SubAgents && e.has_id() {
                e.may = Some(May::personal(e.kind));
            }
            e
        };
        Some(out.into_iter().map(|(.., e)| e).filter(shown).map(with_may).collect())
    }

    /// What the turn's message said: the person's words, or why Magnus
    /// opened the conversation. An outcome turn's text is the server's: the
    /// result above it shows instead.
    fn turn_message(&self, t: &Task, w: &Words) -> Option<Placed> {
        let e = if let Some(o) = t.chat.as_ref().and_then(|c| c.opener.as_deref()) {
            // Magnus opened it: the server's instruction is not shown as anyone's message.
            ChatEntry {
                id: format!("m:{}", t.task_id),
                kind: Kind::Opener.as_str(),
                app: o.app.clone(),
                app_title: o.app_title.clone(),
                text: crate::openers::title(o),
                at: Some(t.created_at),
                task_id: Some(t.task_id.clone()),
                ..Default::default()
            }
        } else if self.read(|s| s.outcomes.of(&t.task_id).is_none()) {
            let files: Vec<EntryFile> = self.read(|s| {
                t.files
                    .iter()
                    .filter_map(|id| s.library.files().get(id))
                    .map(|f| EntryFile { id: f.id.clone(), name: f.name.clone(), media_type: Some(f.media_type.clone()), size: Some(f.size) })
                    .collect()
            });
            ChatEntry {
                id: format!("m:{}", t.task_id),
                kind: Kind::Message.as_str(),
                text: w.text(t.seq).to_string(),
                at: Some(t.created_at),
                task_id: Some(t.task_id.clone()),
                files: Some(files),
                ..Default::default()
            }
        } else {
            return None;
        };
        Some((t.created_at, 0, Entry::said(e)))
    }

    /// The cards a turn put to the person, in the order it made them, each
    /// with its answer.
    fn turn_cards(&self, person: &str, t: &Task, r: &Run) -> Vec<Placed> {
        let mut cards: Vec<_> = r.results.values().filter_map(|res| res.data.as_ref().filter(|d| d["card"].is_string()).map(|d| (res.seq, d.clone()))).collect();
        cards.sort_by_key(|(seq, _)| *seq);
        let mut out = vec![];
        for (seq, d) in cards {
            let card = d["card"].as_str().unwrap_or_default().to_string();
            let id = format!("c:{card}");
            // An app's card: the app as the person stands with it now.
            if d["connector"].is_string() {
                let e = fields(self.offered_card(person, &d));
                out.push((t.created_at, seq, Entry::card(id, Kind::ConnectorCard, t.task_id.clone(), e)));
                continue;
            }
            // Its answer: the schedule or task it made, or the reason it was declined.
            let (state, made) = crate::suggestions::state_of(self.read(|s| s.arrangements.cards().get(&card).cloned()).as_ref());
            let kind = Kind::parse(crate::chat::card_kind(&d, state));
            let mut e = d.clone();
            if d["proposal"] == true {
                // A task proposal recorded before rev 140 reads as a suggestion.
                e["plan"] = d["text"].clone();
                e["permissions"] = d["needs"]["permissions"].clone();
            }
            e["state"] = json!(state);
            e["schedule"] = made["schedule"].clone();
            e["started"] = made["started"].clone();
            e["reason"] = made["reason"].clone();
            // The grant it set, when accepted (#235).
            e["granted"] = made["granted"].clone();
            if d["proposal"] == true || d["grant"] == true {
                if let Some(g) = self.read(|s| s.arrangements.grants().values().find(|g| g.card.as_deref() == Some(card.as_str())).map(|g| g.id.clone())) {
                    e["granted"] = json!(g);
                }
                // What a grant form offers: the person's apps.
                if state == "open" && d["grant"] == true {
                    e["your_apps"] = self.grant_apps(person);
                }
            }
            e["acts_for"] = json!(t.person);
            e["taint"] = json!({ "untrusted": r.untrusted, "private": r.private });
            out.push((t.created_at, seq, Entry::card(id, kind, t.task_id.clone(), fields(e))));
        }
        out
    }

    /// The turn's answer, or where the turn stands while it has none.
    fn turn_reply(&self, t: &Task, r: &Run) -> Placed {
        let at = r.ended_at.unwrap_or(t.created_at);
        if r.status == Status::Completed {
            let e = ChatEntry {
                id: format!("a:{}", t.task_id),
                kind: Kind::Answer.as_str(),
                text: self.read(|s| crate::ui::docs::current_text(s, &t.person, &r.summary)).unwrap_or_default(),
                view: r.outcome_view.clone(),
                spoken: r.spoken.clone(),
                at: r.ended_at,
                task_id: Some(t.task_id.clone()),
                model: Some(json!({ "adapter": r.config.adapter, "model": r.config.model })),
                verdict: t.verdict.as_ref().map(|v| json!(v)),
                ..Default::default()
            };
            return (at, i64::MAX, Entry::said(e));
        }
        let turn = Turn { status: crate::api::status_str(r.status), waiting_on: r.reason.clone(), waiting_item: r.waiting_item.clone(), text: r.summary.clone() };
        (
            at,
            i64::MAX,
            Entry { id: format!("a:{}", t.task_id), kind: Kind::Turn, at: r.ended_at, task_id: Some(t.task_id.clone()), may: None, reaction: None, body: Body::Turn(turn) },
        )
    }

    /// A task's result, once it ended.
    fn result_entry(&self, t: &Task, r: &Run, title: String, w: &Words, end: DateTime<Utc>) -> Entry {
        let files = r.outputs.iter().map(|(id, name, mt, size)| EntryFile { id: id.clone(), name: name.clone(), media_type: Some(mt.clone()), size: Some(*size) }).collect();
        let result = TaskResult {
            title,
            text: w.text(t.seq).to_string(),
            status: crate::api::status_str(r.status),
            summary: r.summary.clone(),
            view: r.outcome_view.clone(),
            files,
            verdict: t.verdict.as_ref().map(|v| json!(v)),
            evidence: self.evidence_of(r),
        };
        Entry { id: format!("r:{}", t.task_id), kind: Kind::Result, at: Some(end), task_id: Some(t.task_id.clone()), may: None, reaction: None, body: Body::Result(result) }
    }

    /// The chat turn (its task id) that started what an entry shows: a
    /// task's result, a "started tasks" card or its sub-agents. None for the turn's own
    /// entries, and for anything not started from a turn.
    pub(crate) fn origin_turn(&self, e: &Entry) -> Option<String> {
        self.read(|s| {
            let from_run = match e.kind {
                Kind::Result => s.tasks.get(e.task_id.as_deref()?)?.chat.as_ref()?.from_run.clone()?,
                Kind::Tasks | Kind::SubAgents => e.from_run()?.to_string(),
                _ => return None,
            };
            s.runs.get(&from_run).map(|r| r.task.clone())
        })
    }

    /// Entries as the model reads them: who said what, in the form a
    /// conversation is stored in (`ChatOrigin.conversation`).
    pub(crate) fn transcript_of(entries: &[Entry]) -> Vec<Value> {
        entries.iter().filter_map(Entry::transcript_line).map(|(role, said)| json!({ "role": role, "text": said })).collect()
    }

    pub(crate) fn sub_agents_entry(&self, turn: &Task, r: &Run, w: &Words) -> Option<Placed> {
        self.read(|s| sub_agents_entry(s, turn, r, w))
    }
}

/// A card's fields: the model's JSON object.
fn fields(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// The sub-agents a chat turn started, nested ones too, as one entry of its
/// conversation placed when the first began: each one's goal, status, what
/// it waits on and its result.
pub(crate) fn sub_agents_entry(s: &State, turn: &Task, r: &Run, w: &Words) -> Option<Placed> {
    let mut first: Option<DateTime<Utc>> = None;
    let agents: Vec<Agent> = s
        .sub_agents
        .below(&r.id)
        .into_iter()
        .filter_map(|(depth, c)| {
            let cr = s.runs.get(&c.child)?;
            let ct = s.tasks.get(&cr.task)?;
            first = Some(first.map_or(ct.created_at, |f| f.min(ct.created_at)));
            Some(Agent {
                run: c.child,
                parent: c.parent,
                depth,
                task_id: ct.task_id.clone(),
                goal: w.text(ct.seq).to_string(),
                status: crate::api::status_str(cr.status),
                waiting_on: cr.reason.clone(),
                waiting_item: cr.waiting_item.clone(),
                summary: cr.summary.clone(),
                tools: c.tools,
                at: ct.created_at,
                ended_at: cr.ended_at,
            })
        })
        .collect();
    let at = first?;
    let body = Body::SubAgents(SubAgents { from_run: r.id.clone(), agents });
    Some((at, 1, Entry { id: format!("s:{}", turn.task_id), kind: Kind::SubAgents, at: Some(at), task_id: Some(turn.task_id.clone()), may: None, reaction: None, body }))
}

#[cfg(test)]
pub(crate) mod golden_tests;

#[cfg(test)]
mod tests;
