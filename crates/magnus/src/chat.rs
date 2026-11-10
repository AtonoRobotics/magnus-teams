//! Chat. A person's message is a chat turn: a run without a contract that
//! ends with its answer. The model decides what the message calls for (an
//! answer, a task, a schedule for the person to confirm, or a question);
//! no code classifies it. Whatever a turn does goes through `execute()`.
//! A thread is derived from the log: its turns, the tasks they started with
//! their results, and the schedule cards with what the person decided.

use crate::boundary::Action;
use crate::events::{ChatOrigin, Ev};
use crate::kernel::{new_id, Kernel, NewRun};
use crate::state::{Run, Status, Task};
use crate::tools::Label;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

/// An id fixed by the action's idempotency key, so a call resent after a
/// restart finds what the first one made.
pub(crate) fn derived(prefix: &str, key: &str) -> String {
    use sha2::Digest;
    format!("{prefix}_{}", &hex::encode(sha2::Sha256::digest(key.as_bytes()))[..32])
}

/// The thread a schedule's runs post to: one of its own, so recurring output
/// never buries the conversation it was set up in (behavior spec, Tasks and
/// schedules in conversation). The person can reply in it like any other.
pub fn schedule_thread(schedule: &str) -> String {
    format!("schedule-{schedule}")
}

/// Why a card decision was not taken.
pub enum CardRefusal {
    NotFound,
    Decided,
    Invalid(String),
}

impl Kernel {
    /// The person writes in a thread of theirs, or starts one (in a project
    /// of theirs, if they chose one), with the model they chose for this
    /// message (the appliance's when none) and any files they added to it.
    pub async fn chat_say(
        self: &Arc<Self>,
        person: &str,
        text: String,
        thread: Option<String>,
        model: Option<(String, String)>,
        experiment: Option<crate::events::ExperimentTag>,
        files: Vec<String>,
        project: Option<String>,
    ) -> Result<Option<(String, String)>> {
        let (thread, project) = match thread {
            Some(t) if self.thread_gone(&t) || (self.thread(person, &t).is_empty() && !self.read(|s| s.branches.get(&t).is_some_and(|b| b.person == person))) => return Ok(None),
            Some(t) => {
                let p = self.read(|s| s.project_of(&t));
                (t, p)
            }
            None => (new_id("thread"), project),
        };
        let conversation = self.conversation(person, &thread).await?;
        // What the turn reads in the conversation carries the taint of the
        // runs that wrote it, as their tasks would (I2).
        let label = self.thread_label(person, &thread, None);
        let o = NewRun {
            chat: Some(ChatOrigin { thread: thread.clone(), turn: true, from_run: None, conversation, opener: None }),
            label: (label != Label::PRINCIPAL).then_some(label),
            model,
            experiment,
            files,
            project,
            ..Default::default()
        };
        let (task, _) = self.create_run(&format!("person:{person}"), person, text, vec![], o).await?;
        Ok(Some((thread, task)))
    }

    /// The person's threads, most recent first, each titled by its first
    /// message, recalled for them.
    pub async fn threads(&self, person: &str) -> anyhow::Result<Vec<Value>> {
        let w = self.person_words(person).await?;
        Ok(self.threads_with(person, &w))
    }

    /// Whether the person has this thread (no words are recalled).
    pub fn has_thread(&self, person: &str, thread: &str) -> bool {
        self.threads_with(person, &Default::default()).iter().any(|t| t["thread"] == thread)
    }

    fn threads_with(&self, person: &str, w: &crate::recall::Words) -> Vec<Value> {
        let mut by: std::collections::BTreeMap<String, (DateTime<Utc>, DateTime<Utc>, String)> = Default::default();
        // The conversations Magnus opened, with the turn that opened them.
        let mut opened: std::collections::HashMap<String, (crate::events::Opener, String)> = Default::default();
        self.read(|s| {
            for t in s.tasks.values().filter(|t| t.person == person) {
                let Some(c) = t.chat.as_ref() else { continue };
                // An opener is titled by why Magnus opened it, not by its instruction.
                let first = || c.opener.as_deref().map(crate::openers::title).unwrap_or_else(|| w.text(t.seq).to_string());
                if let Some(o) = c.opener.as_deref().filter(|_| c.turn) {
                    opened.insert(c.thread.clone(), (o.clone(), t.task_id.clone()));
                }
                let e = by.entry(c.thread.clone()).or_insert_with(|| (t.created_at, t.created_at, first()));
                if t.created_at < e.0 && c.turn {
                    (e.0, e.2) = (t.created_at, first());
                }
                e.1 = e.1.max(t.created_at);
            }
        });
        // A branch is a thread from the moment it is made, before its first turn.
        self.read(|s| {
            for (id, b) in s.branches.iter().filter(|(_, b)| b.person == person) {
                let e = by.entry(id.clone()).or_insert((b.at, b.at, b.title.clone()));
                (e.0, e.2) = (b.at, b.title.clone());
            }
        });
        let mut v: Vec<_> = by.into_iter().filter(|(id, _)| !self.thread_gone(id)).collect();
        v.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
        v.into_iter()
            .map(|(id, (started, updated, first))| {
                let project = self.read(|s| s.project_of(&id).and_then(|p| s.projects.get(&p)).map(|p| json!({ "id": p.id, "name": p.name })));
                let mut t = json!({ "thread": id, "title": first.chars().take(80).collect::<String>(), "started_at": started, "updated_at": updated, "project": project });
                if let Some((o, turn)) = opened.get(&id) {
                    t["opener"] = json!({ "app": o.app, "app_title": o.app_title });
                    t["open_cards"] = json!(self.open_cards(person, &id, turn));
                }
                t
            })
            .collect()
    }

    /// The cards an opener put to the person that still wait on their
    /// answer, as the thread shows them (one deleted from it is not here):
    /// what the empty Tasks and Schedules offer.
    fn open_cards(&self, person: &str, thread: &str, turn: &str) -> Vec<Value> {
        let entries = self.entries_with(person, thread, &Default::default()).unwrap_or_default();
        entries
            .iter()
            .filter(|e| e["task_id"] == turn && e["state"] == "open" && matches!(e["kind"].as_str(), Some("suggestion_card" | "proposal_card" | "schedule_card")))
            .map(|e| json!({ "card": e["card"], "kind": e["kind"], "title": e["title"].as_str().filter(|t| !t.is_empty()).or(e["text"].as_str()) }))
            .collect()
    }

    /// A conversation in the trash or removed from it. The tasks it started
    /// stay, and name it still.
    fn thread_gone(&self, thread: &str) -> bool {
        self.read(|s| s.trash.contains_key(thread) || s.removed_threads.contains(thread))
    }

    /// A thread's turns and tasks, in the order they were created.
    fn thread(&self, person: &str, thread: &str) -> Vec<(Task, Run)> {
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
    pub async fn thread_entries(&self, person: &str, thread: &str) -> anyhow::Result<Option<Vec<Value>>> {
        if self.thread_gone(thread) {
            return Ok(None);
        }
        let w = self.person_words(person).await?;
        Ok(self.entries_with(person, thread, &w))
    }

    fn entries_with(&self, person: &str, thread: &str, w: &crate::recall::Words) -> Option<Vec<Value>> {
        let runs = self.thread(person, thread);
        let branch = self.read(|s| s.branches.get(thread).filter(|b| b.person == person).cloned());
        if runs.is_empty() && branch.is_none() {
            return None;
        }
        let mut out: Vec<(DateTime<Utc>, i64, Value)> = vec![];
        // A branch starts with the conversation it was made from, as it was then.
        for (n, e) in branch.iter().flat_map(|b| b.entries.iter()).enumerate() {
            let mut e = e.clone();
            e["copied"] = json!(true);
            let at = e["at"].as_str().and_then(|a| a.parse::<DateTime<Utc>>().ok()).unwrap_or(branch.as_ref().unwrap().at);
            out.push((at, n as i64 - i64::MAX / 2, e));
        }
        // The tasks one message started, as one card in the order they began.
        let mut started: std::collections::BTreeMap<String, (DateTime<Utc>, Vec<Value>)> = Default::default();
        // A task's result follows the reply of the message that started it,
        // even when the task finished first.
        let answered: std::collections::HashMap<String, DateTime<Utc>> =
            runs.iter().filter(|(t, _)| t.is_chat_turn()).filter_map(|(_, r)| r.ended_at.map(|e| (r.id.clone(), e))).collect();
        for (t, r) in &runs {
            let turn = t.is_chat_turn();
            let status = crate::api::status_str(r.status);
            if let Some(o) = t.chat.as_ref().and_then(|c| c.opener.as_deref()).filter(|_| turn) {
                // Magnus opened it: the appliance's instruction is not shown as anyone's message.
                out.push((t.created_at, 0, json!({ "id": format!("m:{}", t.task_id), "kind": "opener", "app": o.app, "app_title": o.app_title, "text": crate::openers::title(o), "at": t.created_at, "task_id": t.task_id })));
            } else if turn && self.read(|s| s.outcomes.of(&t.task_id).is_none()) {
                // An outcome turn's text is the server's: the result above it shows instead.
                let files: Vec<Value> = self
                    .read(|s| t.files.iter().filter_map(|id| s.files.get(id)).map(|f| json!({ "id": f.id, "name": f.name, "media_type": f.media_type, "size": f.size })).collect());
                out.push((
                    t.created_at,
                    0,
                    json!({ "id": format!("m:{}", t.task_id), "kind": "message", "text": w.text(t.seq), "at": t.created_at, "task_id": t.task_id, "files": files }),
                ));
            }
            if turn {
                let mut cards: Vec<_> = r.results.values().filter_map(|res| res.data.as_ref().filter(|d| d["card"].is_string()).map(|d| (res.seq, d.clone()))).collect();
                cards.sort_by_key(|(seq, _)| *seq);
                for (seq, d) in cards {
                    let card = d["card"].as_str().unwrap_or_default().to_string();
                    // An app's card: the app as the person stands with it now.
                    if d["connector"].is_string() {
                        let mut e = self.offered_card(person, &d);
                        e["id"] = json!(format!("c:{card}"));
                        e["task_id"] = json!(t.task_id);
                        out.push((t.created_at, seq, e));
                        continue;
                    }
                    // Its answer: the schedule or task it made, or the reason it was declined.
                    let (state, made) = crate::suggestions::state_of(self.read(|s| s.cards.get(&card).cloned()).as_ref());
                    let mut e = d.clone();
                    e["id"] = json!(format!("c:{card}"));
                    e["task_id"] = json!(t.task_id);
                    e["kind"] = json!(if d["proposal"] == true {
                        "proposal_card"
                    } else if d["grant"] == true {
                        "grant_card"
                    } else if d["suggestion"] == true {
                        "suggestion_card"
                    } else {
                        "schedule_card"
                    });
                    e["state"] = json!(state);
                    e["schedule"] = made["schedule"].clone();
                    e["started"] = made["started"].clone();
                    e["reason"] = made["reason"].clone();
                    // The grant it set: accepted, or a proposal handled from now on (#235).
                    e["granted"] = made["granted"].clone();
                    if d["proposal"] == true || d["grant"] == true {
                        if let Some(g) = self.read(|s| s.grants.values().find(|g| g.card.as_deref() == Some(card.as_str())).map(|g| g.id.clone())) {
                            e["granted"] = json!(g);
                        }
                        // What a grant form offers: the person's apps, and for a proposal its defaults.
                        if state == "open" {
                            e["your_apps"] = self.grant_apps(person);
                            if d["proposal"] == true {
                                e["handle"] = self.handle_defaults(person, &d);
                            }
                        }
                    }
                    e["acts_for"] = json!(t.person);
                    e["taint"] = json!({ "untrusted": r.untrusted, "private": r.private });
                    out.push((t.created_at, seq, e));
                }
                match r.status {
                    Status::Completed => out.push((
                        r.ended_at.unwrap_or(t.created_at),
                        i64::MAX,
                        json!({ "id": format!("a:{}", t.task_id), "kind": "answer", "text": self.read(|s| crate::ui::docs::current_text(s, &t.person, &r.summary)), "view": r.outcome_view, "spoken": r.spoken, "at": r.ended_at, "task_id": t.task_id, "model": { "adapter": r.config.adapter, "model": r.config.model }, "verdict": t.verdict }),
                    )),
                    _ => out.push((
                        r.ended_at.unwrap_or(t.created_at),
                        i64::MAX,
                        json!({ "id": format!("a:{}", t.task_id), "kind": "turn", "status": status, "waiting_on": r.reason, "waiting_item": r.waiting_item, "text": r.summary, "task_id": t.task_id, "at": r.ended_at }),
                    )),
                }
            } else {
                let title = crate::api::title_of(t, Some(r), w);
                let from = t.chat.as_ref().and_then(|c| c.from_run.clone()).unwrap_or_else(|| t.task_id.clone());
                let e = started.entry(from).or_insert((t.created_at, vec![]));
                e.1.push(json!({ "task_id": t.task_id, "title": title, "text": w.text(t.seq), "status": status, "waiting_on": r.reason, "waiting_item": r.waiting_item }));
                if let Some(end) = r.ended_at {
                    let after = t.chat.as_ref().and_then(|c| c.from_run.as_ref()).and_then(|f| answered.get(f)).map_or(end, |a| end.max(*a));
                    let files: Vec<Value> = r.outputs.iter().map(|(id, name, mt, size)| json!({ "id": id, "name": name, "media_type": mt, "size": size })).collect();
                    out.push((after, i64::MAX, json!({ "id": format!("r:{}", t.task_id), "kind": "result", "task_id": t.task_id, "title": title, "text": w.text(t.seq), "status": status, "summary": r.summary, "view": r.outcome_view, "at": end, "files": files, "verdict": t.verdict })));
                }
            }
        }
        for (from, (at, tasks)) in started {
            out.push((at, 1, json!({ "id": format!("t:{from}"), "kind": "tasks", "from_run": from, "tasks": tasks, "at": at })));
        }
        out.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let (hidden, trashed) = self.read(|s| {
            (s.hidden.get(thread).cloned().unwrap_or_default(), s.trash.iter().filter(|(_, t)| t.thread() == Some(thread)).map(|(id, _)| id.clone()).collect::<HashSet<String>>())
        });
        // What a hidden answer started is hidden with it, whenever it shows
        // up (a task's result after the hiding), so its words and taint
        // never come back into the thread.
        let hidden_turns: HashSet<String> = hidden.iter().filter_map(|id| id.strip_prefix("a:").map(String::from)).collect();
        let from_hidden = |v: &Value| self.origin_turn(v).is_some_and(|t| hidden_turns.contains(&t));
        Some(out.into_iter().map(|(.., v)| v).filter(|v| !v["id"].as_str().is_some_and(|id| hidden.contains(id) || trashed.contains(id)) && !from_hidden(v)).collect())
    }

    /// The chat turn (its task id) that started what an entry shows: a
    /// task's result or a "started tasks" card. None for the turn's own
    /// entries, and for anything not started from a turn.
    pub(crate) fn origin_turn(&self, e: &Value) -> Option<String> {
        self.read(|s| {
            let from_run = match e["kind"].as_str()? {
                "result" => s.tasks.get(e["task_id"].as_str()?)?.chat.as_ref()?.from_run.clone()?,
                "tasks" => e["from_run"].as_str()?.to_string(),
                _ => return None,
            };
            s.runs.get(&from_run).map(|r| r.task.clone())
        })
    }

    /// The taint of what the thread shows, up to and including entry `upto`
    /// (all of it when none): that of the runs that wrote each entry. The
    /// person's own messages carry none.
    pub fn thread_label(&self, person: &str, thread: &str, upto: Option<&str>) -> Label {
        // Which entries, not what they say: no words are recalled.
        let entries = if self.thread_gone(thread) { vec![] } else { self.entries_with(person, thread, &Default::default()).unwrap_or_default() };
        let end = upto.and_then(|u| entries.iter().position(|e| e["id"] == u)).map_or(entries.len(), |i| i + 1);
        let mut tasks: Vec<String> = vec![];
        for e in &entries[..end] {
            match e["kind"].as_str() {
                Some("message" | "opener") | None => {}
                Some("tasks") => tasks.extend(e["tasks"].as_array().into_iter().flatten().filter_map(|t| t["task_id"].as_str().map(String::from))),
                _ => tasks.extend(e["task_id"].as_str().map(String::from)),
            }
        }
        self.read(|s| {
            tasks
                .iter()
                .filter_map(|t| s.tasks.get(t).and_then(|t| s.runs.get(&t.run_id)))
                .fold(Label::PRINCIPAL, |l, r| l.join(Label { untrusted: r.untrusted, private: r.private }))
        })
    }

    /// Magnus's recent messages to the person in their other conversations
    /// (behavior spec, Per-user relationship, "No repetition"), newest
    /// first: those `recent_runs` selects, a result's task title recalled
    /// for them through their wall.
    pub(crate) async fn recent_to(&self, person: &str, except: Option<&str>, before: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, String)>> {
        let picked = self.read(|s| recent_runs(s, person, except, before, crate::relationship::BUDGET));
        let w = self.words(picked.iter().filter(|r| !r.turn).map(|r| (person.to_string(), r.task.seq))).await?;
        Ok(picked
            .into_iter()
            .map(|r| {
                let said = if r.turn { r.summary } else { format!("Result of the task \"{}\" ({}): {}", crate::api::title_of(&r.task, r.run.as_ref(), &w), r.status, r.summary) };
                (r.at, said)
            })
            .collect())
    }

    /// The thread as the model reads it in the next turn: who said what.
    pub(crate) async fn conversation(&self, person: &str, thread: &str) -> Result<Vec<Value>> {
        Ok(Self::transcript_of(self.thread_entries(person, thread).await?.unwrap_or_default()))
    }

    /// Entries as the model reads them: who said what.
    pub(crate) fn transcript_of(entries: Vec<Value>) -> Vec<Value> {
        entries
            .into_iter()
            .filter_map(|e| {
                let text = |k: &str| e[k].as_str().unwrap_or_default().to_string();
                let (role, said) = match e["kind"].as_str()? {
                    "message" => ("person", text("text")),
                    // Its instruction was the appliance's: only that Magnus opened it.
                    "opener" => ("you", format!("(You opened this conversation yourself: {}.)", text("text"))),
                    // What the answer showed below its text is part of what it said.
                    "answer" if e["view"].is_array() => ("you", format!("{}\n[Shown below the answer: {}]", text("text"), e["view"])),
                    "answer" => ("you", text("text")),
                    "tasks" => (
                        "you",
                        format!(
                            "Started tasks: {}",
                            e["tasks"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|t| format!("{} ({})", t["title"].as_str().unwrap_or_default(), t["text"].as_str().unwrap_or_default()))
                                .collect::<Vec<_>>()
                                .join("; ")
                        ),
                    ),
                    "result" => ("task result", format!("{} ({}): {}", text("title"), text("status"), text("summary"))),
                    "schedule_card" => ("you", format!("Proposed a schedule ({}): {}", answered(&e), text("text"))),
                    "suggestion_card" => {
                        let changed = e["changed"].as_str().map(|c| format!(" (again: {c})")).unwrap_or_default();
                        ("you", format!("Suggested {} ({}, card {}): {}{changed}", text("title"), answered(&e), text("card"), text("plan")))
                    }
                    "proposal_card" => {
                        let changed = e["changed"].as_str().map(|c| format!(" (again: {c})")).unwrap_or_default();
                        let handled = e["granted"].as_str().map(|g| format!(" They chose to have things like this handled from now on: grant {g}.")).unwrap_or_default();
                        (
                            "you",
                            format!(
                                "Proposed the task {} ({}, card {}): {} Outcome: {}{changed}{handled}",
                                text("title"),
                                answered(&e),
                                text("card"),
                                text("text"),
                                text("outcome")
                            ),
                        )
                    }
                    "grant_card" => {
                        let list = |k: &str| e[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
                        let id = e["granted"].as_str().map(|g| format!(", grant {g}")).unwrap_or_default();
                        (
                            "you",
                            format!(
                                "Proposed a grant {} ({}, card {}{id}): apps {}; still asks before {}; up to ${} a {}",
                                text("title"),
                                answered(&e),
                                text("card"),
                                list("apps"),
                                list("still_asks"),
                                e["max_usd"],
                                text("period")
                            ),
                        )
                    }
                    "connector_card" => ("you", format!("Offered the app {} ({}): {}", text("title"), text("state"), text("why"))),
                    _ => return None,
                };
                Some(json!({ "role": role, "text": said }))
            })
            .collect()
    }

    /// `create_task` from a chat turn: the task starts at once for the
    /// person, carries the conversation, the turn's taint and the turn's
    /// model, and posts its result to the thread.
    pub(crate) async fn chat_task(self: &Arc<Self>, a: &Action, key: &str) -> Result<(String, bool, Option<Value>)> {
        let text = a.input["text"].as_str().map(str::trim).unwrap_or_default().to_string();
        let title = a.input["title"].as_str().map(str::trim).unwrap_or_default().to_string();
        if text.is_empty() || title.is_empty() {
            return Ok(("create_task needs the task's text and a short title. A retry without them will not help.".into(), true, None));
        }
        let Some((task, run)) = self.read(|s| s.runs.get(&a.run).and_then(|r| s.tasks.get(&r.task).map(|t| (t.clone(), r.clone())))) else {
            anyhow::bail!("unknown run {}", a.run)
        };
        if !task.is_chat_turn() {
            return Ok(("create_task is for chat.".into(), true, None));
        }
        // The person's authority, a grant of theirs (#235), or a retry's (outcomes.rs).
        let more = match self.task_authority(&run, &task, &a.input, key).await {
            Ok(more) => more,
            Err(why) => return Ok((format!("No task started: {why}."), true, None)),
        };
        let grant = more.grant.clone();
        let task_id = self.task_from_turn(&format!("chat:{}", a.run), &task, &run, key, text.clone(), title.clone(), more).await?;
        let under = grant.as_ref().map(|g| format!(" It runs under the grant {g}.")).unwrap_or_default();
        Ok((
            format!("Started task {task_id} ({title}): {text}. It is in the person's Tasks and will post its result to this thread.{under}"),
            false,
            Some(json!({ "task": task_id, "grant": grant })),
        ))
    }

    /// A task started from a chat turn, once for `key`: for the turn's
    /// person, with the conversation, the turn's taint and model, the
    /// message's files and the thread's project; it posts its result to
    /// the thread. `more` sets anything else it starts with.
    #[allow(clippy::too_many_arguments)]
    async fn task_from_turn(self: &Arc<Self>, actor: &str, task: &Task, run: &Run, key: &str, text: String, title: String, more: NewRun) -> Result<String> {
        let Some(chat) = task.chat.clone() else { anyhow::bail!("task {} is not from chat", task.task_id) };
        let (task_id, run_id) = (derived("task", key), derived("run", key));
        if !self.read(|s| s.tasks.contains_key(&task_id)) {
            let mut conversation = chat.conversation.clone();
            // An opener's text is the appliance's instruction, not something the person asked.
            if chat.opener.is_none() && self.read(|s| s.outcomes.of(&task.task_id).is_none()) {
                conversation.push(json!({ "role": "person", "text": self.asked(run).await? }));
            }
            let o = NewRun {
                chat: Some(ChatOrigin { thread: chat.thread.clone(), turn: false, from_run: Some(run.id.clone()), conversation, opener: None }),
                label: Some(Label { untrusted: run.untrusted, private: run.private }),
                ids: Some((task_id.clone(), run_id)),
                model: Some((run.config.adapter.clone(), run.config.model.clone())),
                title: Some(title),
                // The files of the message that started it, and the thread's project.
                files: task.files.clone(),
                project: self.read(|s| s.project_of(&chat.thread)),
                ..more
            };
            self.create_run(actor, &run.person, text, vec![], o).await?;
        }
        Ok(task_id)
    }

    /// `create_schedule` from a chat turn: a card in the thread with the
    /// timing, scope and budget. Nothing is scheduled until the person
    /// confirms it.
    pub(crate) fn chat_card(&self, a: &Action, key: &str) -> (String, bool, Option<Value>) {
        if !self.read(|s| s.runs.get(&a.run).and_then(|r| s.tasks.get(&r.task)).is_some_and(|t| t.is_chat_turn())) {
            return ("create_schedule is for chat; in a task, use propose_schedule.".into(), true, None);
        }
        let text = a.input["text"].as_str().map(str::trim).unwrap_or_default();
        let scope = a.input["scope"].as_str().map(str::trim).unwrap_or_default();
        if text.is_empty() || scope.is_empty() {
            return ("create_schedule needs text and scope. A retry without them will not help.".into(), true, None);
        }
        let (every, daily) = match crate::api::rule_for(&a.input, self.cfg.schedules.min_every_minutes) {
            Ok(r) => r,
            Err(m) => return (format!("{m}. A retry with the same values will not help."), true, None),
        };
        let budget = match self.card_budget(&a.input["budget"]) {
            Ok(b) => b,
            Err(m) => return (format!("{m}. A retry with the same budget will not help."), true, None),
        };
        let (until, max_runs) = match crate::api::end_for(&a.input) {
            Ok(r) => r,
            Err(m) => return (format!("{m}. A retry with the same values will not help."), true, None),
        };
        let card = derived("card", key);
        let title = a.input["title"].as_str().map(str::trim).filter(|t| !t.is_empty());
        // A suggestion too (#253): its plan, permissions, and the request it answers as its evidence.
        let turn = self.read(|s| s.runs.get(&a.run).map(|r| r.task.clone())).unwrap_or_default();
        let evidence = json!([{ "task": turn, "shows": "you asked for it" }]);
        let data = json!({ "card": card, "text": text, "title": title, "every_minutes": every, "daily_at": daily, "until": until, "max_runs": max_runs, "scope": scope, "budget": budget, "plan": text, "permissions": [scope], "evidence": evidence });
        (format!("A card for this schedule ({card}) is in the thread. Nothing is scheduled until the person confirms it."), false, Some(data))
    }

    /// A schedule's budget per run: the appliance's, tightened where the
    /// model asked, never loosened.
    pub(crate) fn card_budget(&self, b: &Value) -> std::result::Result<crate::config::Budget, String> {
        let mut out = self.cfg.budget.clone();
        if b.is_null() {
            return Ok(out);
        }
        if let Some(v) = b["max_usd"].as_f64() {
            if v <= 0.0 || v > out.max_usd {
                return Err(format!("max_usd must be above 0 and at most the server's {}", out.max_usd));
            }
            out.max_usd = v;
        }
        for (k, slot) in [("max_model_calls", &mut out.max_model_calls), ("max_actions", &mut out.max_actions)] {
            if let Some(v) = b[k].as_u64() {
                if v == 0 || v > *slot as u64 {
                    return Err(format!("{k} must be above 0 and at most the appliance's {slot}"));
                }
                *slot = v as u32;
            }
        }
        Ok(out)
    }

    /// The person who started the thread answers a card: accept (or
    /// confirm), later ("Not now"), or decline with their reason. An
    /// accepted schedule acts for them (K54), within the card's budget, and
    /// its runs start with the taint the turn had; an accepted suggestion
    /// without timing starts a task for them as `create_task` would. A task
    /// proposal is approved (it starts as such a suggestion does) or
    /// dismissed with their reason; it is not put off (#254).
    ///
    /// A proposal may instead be handled from now on (`handle`, "Handle
    /// things like this"): the person sets a grant over that kind of work,
    /// from the fields in `body` over the card's defaults, and its task
    /// starts under it. A grant card (`propose_grant`) is accepted, with
    /// any changes of the person's in `body`, setting the grant, or
    /// declined with their reason (#235).
    pub async fn decide_card(self: &Arc<Self>, person: &str, card: &str, decision: &str, reason: Option<&str>, body: &Value) -> Result<std::result::Result<Value, CardRefusal>> {
        let _decide = self.decide.lock().await;
        let found = self.read(|s| {
            s.runs.values().find_map(|r| {
                let t = s.tasks.get(&r.task).filter(|t| t.is_chat_turn() && t.person == person)?;
                let d = r.results.values().find_map(|res| res.data.clone().filter(|d| d["card"] == card))?;
                Some((t.clone(), r.clone(), d))
            })
        });
        let Some((task, run, d)) = found else { return Ok(Err(CardRefusal::NotFound)) };
        let actor = format!("person:{person}");
        let decision = match (d["proposal"] == true, d["grant"] == true, decision) {
            (true, _, "approve" | "accept" | "confirm") => "accept",
            (true, _, "handle") => "handle",
            (true, _, "dismiss" | "decline") => "decline",
            (true, _, other) => {
                return Ok(Err(CardRefusal::Invalid(format!(
                    "a task proposal is approved or dismissed (with a reason), or handled from now on under a grant (handle), not {other}"
                ))))
            }
            (false, true, "accept" | "confirm" | "approve") => "grant",
            (false, true, "decline" | "dismiss") => "decline",
            (false, true, other) => return Ok(Err(CardRefusal::Invalid(format!("a grant card is accepted or declined (with a reason), not {other}")))),
            (false, false, d) => d,
        };
        // A proposal handled whose grant was set but whose task had not
        // started (the appliance stopped between them) finishes now.
        let prior = self.read(|s| s.cards.get(card).map(|c| c.answer.clone()));
        let resumed = decision == "handle" && matches!(prior, Some(crate::state::Answer::Granted(_)));
        if prior.is_some() && !resumed {
            return Ok(Err(CardRefusal::Decided));
        }
        // The card's words are the model's, carrying the turn's taint; a
        // title the person wrote is theirs.
        let turn_label = Label { untrusted: run.untrusted, private: run.private };
        let label_for = || if body["title"].as_str().is_some_and(|t| t.trim() != d["title"].as_str().unwrap_or_default().trim()) { None } else { Some(turn_label) };
        match decision {
            "handle" => {
                let Some(who) = self.person(person) else { return Ok(Err(CardRefusal::NotFound)) };
                let id = derived("grant", &format!("card:{card}"));
                if !resumed {
                    let fields = match self.grant_fields(&who, &crate::grants::over(&self.handle_defaults(person, &d), body)) {
                        Ok(f) => f,
                        Err(why) => return Ok(Err(CardRefusal::Invalid(why))),
                    };
                    let label = label_for();
                    self.set_grant(&actor, person, &id, &fields, Some(card.into()), label).await?;
                }
                let left = match self.grant_for_task(person, &id) {
                    Ok((_, left)) => left,
                    Err(why) => return Ok(Err(CardRefusal::Invalid(why))),
                };
                let plan = d["text"].as_str().unwrap_or_default().to_string();
                let title = d["title"].as_str().unwrap_or_default().to_string();
                let mut budget: crate::config::Budget = serde_json::from_value(d["budget"].clone()).unwrap_or_else(|_| self.cfg.budget.clone());
                budget.max_usd = budget.max_usd.min(left);
                let more = NewRun { budget: Some(budget), scope: d["scope"].as_str().map(String::from), grant: Some(id.clone()), ..Default::default() };
                let started = self.task_from_turn(&actor, &task, &run, &format!("card:{card}"), plan, title, more).await?;
                self.emit(&actor, Ev::CardAccepted { card: card.into(), person: person.into(), task: started.clone() }).await?;
                Ok(Ok(json!({ "task": started, "grant": id })))
            }
            "grant" => {
                let Some(who) = self.person(person) else { return Ok(Err(CardRefusal::NotFound)) };
                let fields = match self.grant_fields(&who, &crate::grants::over(&d, body)) {
                    Ok(f) => f,
                    Err(why) => return Ok(Err(CardRefusal::Invalid(why))),
                };
                // A change to one of theirs keeps its id, while it is still theirs and live.
                let id = match d["replaces"].as_str() {
                    Some(r) if self.read(|s| s.grants.get(r).is_some_and(|g| g.person == person && g.revoked.is_none())) => r.to_string(),
                    Some(r) => return Ok(Err(CardRefusal::Invalid(format!("the grant it changes ({r}) was revoked; ask Magnus for a new one")))),
                    None => crate::kernel::new_id("grant"),
                };
                let label = label_for();
                self.set_grant(&actor, person, &id, &fields, Some(card.into()), label).await?;
                Ok(Ok(json!({ "grant": id })))
            }
            "confirm" | "accept" if !crate::suggestions::timed(&d) => {
                let plan = d["plan"].as_str().or(d["text"].as_str()).unwrap_or_default().to_string();
                let title = d["title"].as_str().unwrap_or_default().to_string();
                let more = NewRun { budget: serde_json::from_value(d["budget"].clone()).ok(), scope: d["scope"].as_str().map(String::from), ..Default::default() };
                let started = self.task_from_turn(&actor, &task, &run, &format!("card:{card}"), plan, title, more).await?;
                self.emit(&actor, Ev::CardAccepted { card: card.into(), person: person.into(), task: started.clone() }).await?;
                Ok(Ok(json!({ "task": started })))
            }
            "confirm" | "accept" => {
                let id = new_id("sched");
                let budget = serde_json::from_value(d["budget"].clone()).ok();
                self.emit(
                    &actor,
                    Ev::ScheduleSet {
                        id: id.clone(),
                        person: task.person.clone(),
                        text: d["text"].as_str().unwrap_or_default().into(),
                        every_minutes: d["every_minutes"].as_u64().map(|m| m as u32),
                        daily_at: d["daily_at"].as_str().map(String::from),
                        budget,
                        card: Some(card.into()),
                        label: Some(Label { untrusted: run.untrusted, private: run.private }),
                        title: d["title"].as_str().map(String::from),
                        until: serde_json::from_value(d["until"].clone()).ok().flatten(),
                        max_runs: d["max_runs"].as_u64().map(|m| m as u32),
                        // The person agreed to the card's scope, so each run is told it.
                        scope: d["scope"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from),
                    },
                )
                .await?;
                // A schedule confirmed in a project's conversation is in that project.
                if let Some(project) = self.read(|s| s.project_of(&task.chat.as_ref().map(|c| c.thread.clone()).unwrap_or_default())) {
                    self.emit(&actor, Ev::Moved { item: id.clone(), project: Some(project) }).await?;
                }
                Ok(Ok(json!({ "schedule": id })))
            }
            "later" => {
                self.emit(&actor, Ev::CardDeferred { card: card.into(), person: Some(person.into()) }).await?;
                Ok(Ok(json!({})))
            }
            "decline" => {
                let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) else {
                    return Ok(Err(CardRefusal::Invalid("a decline or dismissal needs the person's reason".into())));
                };
                self.emit(&actor, Ev::CardDeclined { card: card.into(), person: Some(person.into()), reason: Some(reason.into()) }).await?;
                Ok(Ok(json!({})))
            }
            other => Ok(Err(CardRefusal::Invalid(format!("decision is accept (or confirm), later or decline, not {other}")))),
        }
    }
}

/// A card's answer as the model reads it in the conversation.
fn answered(e: &Value) -> String {
    match (e["state"].as_str().unwrap_or_default(), e["reason"].as_str()) {
        ("declined", Some(r)) => format!("declined: \"{r}\""),
        ("later", _) => "not now".into(),
        (s, _) => s.into(),
    }
}

/// A message of Magnus's a run may see among its recent ones.
pub(crate) struct RecentRun {
    pub at: DateTime<Utc>,
    pub task: Task,
    /// For a result, the run its title may come from.
    pub run: Option<Run>,
    /// An answer in a conversation, or a task's result posted there.
    pub turn: bool,
    pub status: &'static str,
    pub summary: String,
}

/// Magnus's messages to `person` a run may see, newest first, chosen
/// straight from the runs rather than by rebuilding each conversation
/// (review of #245): its answers (chat turns that completed) and its
/// tasks' results, in a conversation of theirs other than `except`, ended
/// before `before`, as their conversations show them (a conversation in
/// the trash, an entry deleted or hidden, a result of a hidden answer, left
/// out). Only those of a run that read nothing from outside the
/// conversation: one carrying untrusted or private content would bring its
/// taint into every later run of theirs. It stops once their text fills
/// `budget`.
pub(crate) fn recent_runs(s: &crate::state::State, person: &str, except: Option<&str>, before: DateTime<Utc>, budget: usize) -> Vec<RecentRun> {
    let shown = |thread: &str, id: String| !s.hidden.get(thread).is_some_and(|h| h.contains(&id)) && !s.trash.contains_key(&id);
    let mut eligible: Vec<(DateTime<Utc>, &Task, &Run)> = s
        .tasks
        .values()
        .filter(|t| t.person == person)
        .filter_map(|t| {
            let c = t.chat.as_ref().filter(|c| Some(c.thread.as_str()) != except)?;
            if s.trash.contains_key(&c.thread) || s.removed_threads.contains(&c.thread) {
                return None;
            }
            let r = s.runs.get(&t.run_id).filter(|r| !r.untrusted && !r.private)?;
            let at = r.ended_at.filter(|at| *at < before)?;
            let ok = if c.turn {
                r.status == Status::Completed && shown(&c.thread, format!("a:{}", t.task_id))
            } else {
                let origin = c.from_run.as_ref().and_then(|f| s.runs.get(f)).map(|o| format!("a:{}", o.task));
                shown(&c.thread, format!("r:{}", t.task_id)) && origin.is_none_or(|o| shown(&c.thread, o))
            };
            ok.then_some((at, t, r))
        })
        .collect();
    eligible.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = vec![];
    let mut used = 0;
    for (at, t, r) in eligible {
        if used >= budget {
            break;
        }
        let turn = t.is_chat_turn();
        let summary = r.summary.as_deref().unwrap_or_default().trim().to_string();
        if turn && summary.is_empty() {
            continue;
        }
        used += summary.len().max(1);
        out.push(RecentRun { at, task: t.clone(), run: (!turn).then(|| r.clone()), turn, status: crate::api::status_str(r.status), summary });
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::log::Event;
    use crate::state::State;
    use serde_json::json;

    /// Review of #245: the recent messages are chosen newest first and the
    /// choosing stops at the budget, not after every conversation of the
    /// person is rebuilt.
    #[test]
    fn the_recent_messages_are_the_newest_and_their_choice_stops_at_the_budget() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        let t0 = chrono::Utc::now();
        let mut seq = 0;
        let mut ev = |at: chrono::DateTime<chrono::Utc>, actor: &str, e: serde_json::Value| {
            seq += 1;
            s.apply(&Event { seq, at, actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
        };
        // Fifty conversations of Carol's, each a 1KB answer, a minute apart, and one of Bob's.
        for i in 0..50i64 {
            let at = t0 + chrono::Duration::minutes(i);
            let chat = json!({ "thread": format!("th{i}"), "turn": true });
            ev(
                at,
                "person:carol",
                json!({ "kind": "task_created", "task": format!("t{i}"), "run": format!("r{i}"), "person": "carol", "text": "Hi.", "config": config, "chat": chat }),
            );
            ev(at, "kernel", json!({ "kind": "completed", "run": format!("r{i}"), "summary": format!("{i:02}{}", "x".repeat(1022)) }));
        }
        ev(
            t0,
            "person:bob",
            json!({ "kind": "task_created", "task": "tb", "run": "rb", "person": "bob", "text": "Hi.", "config": config, "chat": { "thread": "thb", "turn": true } }),
        );
        ev(t0, "kernel", json!({ "kind": "completed", "run": "rb", "summary": "to bob" }));
        let later = t0 + chrono::Duration::hours(1);
        let picked = super::recent_runs(&s, "carol", Some("th49"), later, 16 * 1024);
        let firsts: Vec<String> = picked.iter().map(|r| r.summary[..2].to_string()).collect();
        // The newest sixteen fill 16KB; the one in this conversation is not among them.
        assert_eq!(firsts, (33..49).rev().map(|i| format!("{i:02}")).collect::<Vec<_>>());
        assert!(picked.iter().all(|r| r.turn && r.at < later));
    }
}
