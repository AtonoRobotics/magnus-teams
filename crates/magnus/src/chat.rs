//! Chat. A person's message is a chat turn: a run without a contract that
//! ends with its answer. The model decides what the message calls for (an
//! answer, a task, a schedule for the person to confirm, or a question);
//! no code classifies it. Whatever a turn does goes through `execute()`.
//! A thread is derived from the log: its turns, the tasks they started with
//! their results, and the schedule cards with what the person decided.

use crate::boundary::Action;
use crate::chat_thread::{Entry, Kind};
use crate::events::{ChatOrigin, Ev};
use crate::kernel::{new_id, Kernel, NewRun};
use crate::state::{Run, Status, Task};
use crate::tools::Label;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

/// An id fixed by the action's idempotency key, so a call resent after a
/// restart finds what the first one made.
pub(crate) fn derived(prefix: &str, key: &str) -> String {
    use sha2::Digest;
    format!("{prefix}_{}", &hex::encode(sha2::Sha256::digest(key.as_bytes()))[..32])
}

/// A schedule's own thread: where the runs of one set up outside a
/// conversation post (`schedule_thread::of`). The person can reply in it
/// like any other.
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
        v.sort_by_key(|x| std::cmp::Reverse(x.1 .1));
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
            .filter(|e| e.kind == Kind::SuggestionCard && e.task_id.as_deref() == Some(turn))
            .filter_map(|e| e.card_data().filter(|d| d.get("state").is_some_and(|s| s == "open")))
            .map(|d| {
                let s = |k: &str| d.get(k).and_then(Value::as_str);
                json!({ "card": d.get("card"), "kind": Kind::SuggestionCard, "title": s("title").filter(|t| !t.is_empty()).or(s("text")) })
            })
            .collect()
    }

    /// The taint of what the thread shows, up to and including entry `upto`
    /// (all of it when none): that of the runs that wrote each entry. The
    /// person's own messages carry none.
    pub fn thread_label(&self, person: &str, thread: &str, upto: Option<&str>) -> Label {
        // Which entries, not what they say: no words are recalled.
        let entries = if self.thread_gone(thread) { vec![] } else { self.entries_with(person, thread, &Default::default()).unwrap_or_default() };
        let end = upto.and_then(|u| entries.iter().position(|e| e.id == u)).map_or(entries.len(), |i| i + 1);
        // The person's own messages, and Magnus's openings, carry none.
        let tasks: Vec<String> = entries[..end].iter().filter(|e| !matches!(e.kind, Kind::Message | Kind::Opener)).flat_map(Entry::task_ids).collect();
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
        Ok(Self::transcript_of(&self.thread_entries(person, thread).await?.unwrap_or_default()))
    }

    /// `create_task` from a chat turn: the task starts at once for the
    /// person with its one-line goal, carries the conversation, the turn's
    /// taint and the turn's model, and posts its result to the thread.
    pub(crate) async fn chat_task(self: &Arc<Self>, a: &Action, key: &str) -> Result<(String, bool, Option<Value>)> {
        let text = match crate::task_goal::goal_of(&a.input) {
            Ok(goal) => goal,
            Err(why) => return Ok((why, true, None)),
        };
        let title = a.input["title"].as_str().map(str::trim).unwrap_or_default().to_string();
        if title.is_empty() {
            return Ok(("create_task needs a short title. A retry without one will not help.".into(), true, None));
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
            // An opener's text is the appliance's instruction, not something the person asked.
            let asked = if chat.opener.is_none() && self.read(|s| s.outcomes.of(&task.task_id).is_none()) { Some(self.asked(run).await?) } else { None };
            let said: Vec<String> = run.replies.iter().map(|r| r.text()).filter(|t| !t.is_empty()).collect();
            let conversation = crate::task_goal::carried(chat.conversation.clone(), asked, &said.join("\n\n"));
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

    /// The person who started the thread answers a card: accept (one
    /// action, which grants the work it describes), later ("Not now"), or
    /// decline with their reason. An accepted schedule acts for them (K54),
    /// within the server's per-task budget, and its runs start with the
    /// taint the turn had; an accepted suggestion without timing starts a
    /// task for them as `create_task` would. A task proposal recorded before
    /// rev 140 is answered as a suggestion. A grant card (`propose_grant`, retired in rev 147) is
    /// accepted, with any changes of the person's in `body`, setting the
    /// grant, or declined with their reason (#235).
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
        let decision = match (d["grant"] == true, decision) {
            (true, "accept" | "confirm" | "approve") => "grant",
            (true, "decline" | "dismiss") => "decline",
            (true, other) => return Ok(Err(CardRefusal::Invalid(format!("a grant card is accepted or declined (with a reason), not {other}")))),
            (false, "accept" | "confirm" | "approve") => "accept",
            (false, "dismiss") => "decline",
            (false, d) => d,
        };
        if self.read(|s| s.cards.contains_key(card)) {
            return Ok(Err(CardRefusal::Decided));
        }
        // The card's words are the model's, carrying the turn's taint; a
        // title the person wrote is theirs.
        let turn_label = Label { untrusted: run.untrusted, private: run.private };
        let label_for = || if body["title"].as_str().is_some_and(|t| t.trim() != d["title"].as_str().unwrap_or_default().trim()) { None } else { Some(turn_label) };
        match decision {
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
            "accept" if !crate::suggestions::timed(&d) => {
                let plan = d["plan"].as_str().or(d["text"].as_str()).unwrap_or_default().to_string();
                let title = d["title"].as_str().unwrap_or_default().to_string();
                let more =
                    NewRun { budget: Some(crate::task_budget::approved(&self.cfg.budget, &d["budget"])), scope: d["scope"].as_str().map(String::from), ..Default::default() };
                let started = self.task_from_turn(&actor, &task, &run, &format!("card:{card}"), plan, title, more).await?;
                self.emit(&actor, Ev::CardAccepted { card: card.into(), person: person.into(), task: started.clone() }).await?;
                Ok(Ok(json!({ "task": started })))
            }
            "accept" => {
                let id = new_id("sched");
                // No budget of its own: each run takes the server's per-task
                // limit when it fires, never the figure on the card.
                let budget = None;
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
                        // Its runs post in this conversation (owner, 2026-11-10).
                        thread: task.chat.as_ref().filter(|c| c.turn).map(|c| c.thread.clone()),
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

/// The kind a card's entry has in the thread. A schedule set up at the
/// person's request shows as set; every other card that is not a grant is a
/// suggestion, including the cards recorded before rev 140: a task proposal,
/// and a schedule card still waiting to be confirmed.
pub(crate) fn card_kind(d: &Value, state: &str) -> &'static str {
    if d["grant"] == true {
        "grant_card"
    } else if d["schedule"].is_string() || (d["suggestion"] != true && d["proposal"] != true && state == "confirmed") {
        "schedule_card"
    } else {
        "suggestion_card"
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
    eligible.sort_by_key(|x| std::cmp::Reverse(x.0));
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
