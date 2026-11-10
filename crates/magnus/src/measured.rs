//! The signals a rule is judged on, each a query over the log, shown to
//! developers beside the other panels so a rule is revised on evidence.
use crate::state::{Answer, CardState, Run, State};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The verdicts that say an outcome should not have happened.
const REGRET_VERDICTS: [&str; 2] = ["wrong", "undone"];
const REVERSED_VERDICT: &str = "undone";
const COMPLAINT_REACTION: &str = "annoying";

/// Panels with no source on this appliance yet, and why.
const NOT_BUILT: [(&str, &str, &str); 2] = [
    ("ontology", "Ontology check failures", "There is no ontology on this server yet (O1-O4), so nothing is checked against one."),
    ("ablations", "Per-module ablation results", "No removable module has an ablation run yet; the verifier is not built (A4, A10)."),
];

#[derive(Default)]
struct Tally(BTreeMap<String, (usize, BTreeSet<String>)>);

impl Tally {
    fn add(&mut self, label: String, run: &str) {
        let row = self.0.entry(label).or_default();
        row.0 += 1;
        row.1.insert(run.to_string());
    }

    fn panel(&self, id: &str, title: &str, value: Value, unit: String) -> Value {
        let runs: BTreeSet<&String> = self.0.values().flat_map(|(_, r)| r).collect();
        let breakdown: Vec<Value> = self.0.iter().map(|(label, (n, r))| json!({ "label": label, "value": n, "runs": r })).collect();
        json!({ "id": id, "title": title, "value": value, "unit": unit, "runs": runs, "breakdown": breakdown })
    }
}

fn task_regretted(s: &State, task: &str) -> bool {
    s.tasks.get(task).is_some_and(|t| t.verdict.as_ref().is_some_and(|v| REGRET_VERDICTS.contains(&v.verdict.as_str())) || s.stopped_by_person.contains(&t.run_id))
}

fn card_label(s: &State, card: Option<&CardState>) -> &'static str {
    let Some(card) = card else { return "no answer yet" };
    let regretted = match &card.answer {
        Answer::Later => return "not now",
        Answer::Declined(_) => return "dismissed",
        Answer::Started(task) => task_regretted(s, task),
        Answer::Confirmed(schedule) => !s.schedules.contains_key(schedule),
        Answer::Granted(grant) => s.grants.get(grant).is_none_or(|g| g.revoked.is_some()),
    };
    if regretted {
        "accepted, later regretted"
    } else {
        "accepted"
    }
}

fn suggestions(s: &State) -> Value {
    let mut tally = Tally::default();
    for r in s.runs.values() {
        for data in r.results.values().filter_map(|res| res.data.as_ref()) {
            if let (Some(id), false) = (data["card"].as_str(), data["connector"].is_string()) {
                tally.add(card_label(s, s.cards.get(id)).to_string(), &r.id);
            }
        }
    }
    let total: usize = tally.0.values().map(|(n, _)| n).sum();
    tally.panel("suggestions", "Suggestions accepted, dismissed and regretted", json!(total), "suggestions".into())
}

/// Each run a person marked with this reaction on the answer it gave.
fn complained_runs(s: &State) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for ((_, entry), history) in s.reactions.values().flat_map(|mine| mine.iter()) {
        if history.last().and_then(|(r, _)| r.as_deref()) != Some(COMPLAINT_REACTION) {
            continue;
        }
        let Some((_, id)) = entry.split_once(':') else { continue };
        if let Some(run) = s.runs.get(id).or_else(|| s.tasks.get(id).and_then(|t| s.runs.get(&t.run_id))) {
            out.insert(run.id.clone());
        }
    }
    out
}

/// The tools this run acted with and no one was asked about.
fn unasked_tools(r: &Run) -> BTreeSet<&str> {
    let asked = |call: &str| r.decisions.iter().any(|d| d.call_id == call && (d.outcome == "ask" || d.stage == "approval"));
    r.intents.values().filter(|i| !asked(&i.call_id)).map(|i| i.tool.as_str()).collect()
}

fn contested_actions(s: &State) -> Value {
    let complained = complained_runs(s);
    let mut tally = Tally::default();
    let mut acted = 0;
    for r in s.runs.values() {
        let tools = unasked_tools(r);
        if tools.is_empty() {
            continue;
        }
        acted += 1;
        let verdict = s.tasks.get(&r.task).and_then(|t| t.verdict.as_ref()).map(|v| v.verdict.as_str());
        let how = match verdict {
            Some(REVERSED_VERDICT) => "reversed",
            Some("wrong") => "complained about",
            _ if complained.contains(&r.id) => "complained about",
            _ => continue,
        };
        for tool in tools {
            tally.add(format!("{how} · {tool}"), &r.id);
        }
    }
    let contested: BTreeSet<&String> = tally.0.values().flat_map(|(_, r)| r).collect();
    let value = json!(contested.len());
    tally.panel("contested_actions", "Actions taken without asking, then reversed or complained about", value, format!("of {acted} runs that acted without asking"))
}

fn approved_unchanged(s: &State) -> Value {
    let mut tally = Tally::default();
    let (mut asked, mut unchanged) = (0, 0);
    for i in s.items.values().filter(|i| i.kind == "approval") {
        asked += 1;
        let (Some(run), Some((decision, _, text, _))) = (&i.run, &i.resolution) else { continue };
        if decision == "approve" && text.as_deref().is_none_or(str::is_empty) {
            unchanged += 1;
            let rule = i.details["rule"].as_str().or(i.details["risk"].as_str()).unwrap_or("not recorded");
            tally.add(format!("{} · {rule}", i.details["tool"].as_str().unwrap_or("not recorded")), run);
        }
    }
    tally.panel("approved_unchanged", "Approvals asked and approved unchanged", json!(unchanged), format!("of {asked} asked"))
}

fn takeovers(s: &State) -> Value {
    let mut tally = Tally::default();
    let mut total = 0;
    for (cause, runs) in s.computers.takeovers() {
        total += 1;
        for run in runs {
            tally.add(cause.clone().unwrap_or_else(|| "not recorded".into()), run);
        }
        if runs.is_empty() {
            tally.0.entry(cause.clone().unwrap_or_else(|| "not recorded".into())).or_default().0 += 1;
        }
    }
    let per_task = if s.tasks.is_empty() { 0.0 } else { total as f64 / s.tasks.len() as f64 };
    tally.panel("takeovers", "Takeovers per task, by cause", json!(per_task), format!("per task ({total} takeovers)"))
}

pub fn panels(s: &State) -> Vec<Value> {
    let mut out = vec![suggestions(s), contested_actions(s), approved_unchanged(s), takeovers(s)];
    out.extend(NOT_BUILT.iter().map(|(id, title, why)| json!({ "id": id, "title": title, "value": null, "unit": "", "unavailable": why, "runs": [], "breakdown": [] })));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Event;

    fn ev(seq: i64, actor: &str, ev: Value) -> Event {
        let Ok(ev) = serde_json::from_value(ev) else { panic!("not an event") };
        Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev }
    }

    fn task(s: &mut State, seq: i64, id: &str) {
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(
            seq,
            "person:alice",
            json!({ "kind": "task_created", "task": format!("t{id}"), "run": format!("r{id}"), "person": "alice", "text": "Do it.", "config": config }),
        ));
    }

    fn acted(s: &mut State, seq: i64, id: &str) {
        let (run, call) = (format!("r{id}"), format!("c{id}"));
        s.apply(&ev(seq, "kernel", json!({ "kind": "decision", "run": run, "call_id": call, "tool": "send", "outcome": "allow", "stage": "policy", "reason": "permitted" })));
        s.apply(&ev(seq + 1, "kernel", json!({ "kind": "intent", "run": run, "call_id": call, "step_id": "s", "key": call, "tool": "send", "input": {}, "issuer": "model" })));
    }

    fn row<'a>(panel: &'a Value, label: &str) -> Option<&'a Value> {
        panel["breakdown"].as_array()?.iter().find(|r| r["label"] == label)
    }

    #[test]
    fn an_action_taken_without_asking_counts_once_a_person_contests_it() {
        let mut s = State::default();
        for (n, id) in ["1", "2", "3"].iter().enumerate() {
            task(&mut s, n as i64 * 10 + 1, id);
            acted(&mut s, n as i64 * 10 + 2, id);
        }
        s.apply(&ev(40, "person:alice", json!({ "kind": "verdict", "task": "t1", "person": "alice", "verdict": "undone" })));
        s.apply(&ev(41, "person:alice", json!({ "kind": "verdict", "task": "t2", "person": "alice", "verdict": "right" })));
        s.apply(&ev(42, "person:alice", json!({ "kind": "reacted", "person": "alice", "thread": "th", "entry": "t:r3", "reaction": "annoying" })));
        let p = contested_actions(&s);
        assert_eq!(p["value"], 2);
        assert_eq!(p["unit"], "of 3 runs that acted without asking");
        assert_eq!(row(&p, "reversed · send").map(|r| r["runs"].clone()), Some(json!(["r1"])));
        assert_eq!(row(&p, "complained about · send").map(|r| r["runs"].clone()), Some(json!(["r3"])));
    }

    #[test]
    fn an_action_someone_was_asked_about_is_not_counted() {
        let mut s = State::default();
        task(&mut s, 1, "1");
        s.apply(&ev(2, "kernel", json!({ "kind": "decision", "run": "r1", "call_id": "c1", "tool": "send", "outcome": "ask", "stage": "approval", "reason": "asks" })));
        s.apply(&ev(3, "kernel", json!({ "kind": "decision", "run": "r1", "call_id": "c1", "tool": "send", "outcome": "allow", "stage": "approval", "reason": "approved" })));
        s.apply(&ev(4, "kernel", json!({ "kind": "intent", "run": "r1", "call_id": "c1", "step_id": "s", "key": "c1", "tool": "send", "input": {}, "issuer": "model" })));
        s.apply(&ev(5, "person:alice", json!({ "kind": "verdict", "task": "t1", "person": "alice", "verdict": "undone" })));
        assert_eq!(contested_actions(&s)["unit"], "of 0 runs that acted without asking");
    }

    #[test]
    fn an_approval_with_a_note_is_not_approved_unchanged() {
        let mut s = State::default();
        task(&mut s, 1, "1");
        for (n, id) in ["i1", "i2", "i3"].iter().enumerate() {
            s.apply(&ev(n as i64 + 2, "kernel", json!({ "kind": "item", "id": id, "item_kind": "approval", "task": "t1", "run": "r1", "person": "alice", "summary": "Send?", "details": { "tool": "send", "rule": "ask-sends" } })));
        }
        s.apply(&ev(10, "person:alice", json!({ "kind": "item_resolved", "id": "i1", "decision": "approve" })));
        s.apply(&ev(11, "person:alice", json!({ "kind": "item_resolved", "id": "i2", "decision": "approve", "text": "only to Bob" })));
        let p = approved_unchanged(&s);
        assert_eq!(p["value"], 1);
        assert_eq!(p["unit"], "of 3 asked");
        assert!(row(&p, "send · ask-sends").is_some());
    }

    #[test]
    fn a_suggestion_is_regretted_when_the_person_stops_what_it_started() {
        let mut s = State::default();
        task(&mut s, 1, "1");
        s.apply(&ev(2, "kernel", json!({ "kind": "tool_result", "run": "r1", "call_id": "c1", "status": "ok", "is_error": false, "label": { "private": false, "untrusted": false }, "data": { "card": "k1", "suggestion": true } })));
        s.apply(&ev(3, "person:alice", json!({ "kind": "card_accepted", "card": "k1", "person": "alice", "task": "t2" })));
        task(&mut s, 4, "2");
        assert!(row(&suggestions(&s), "accepted").is_some());
        s.apply(&ev(5, "kernel", json!({ "kind": "cancelled", "run": "r2" })));
        assert!(row(&suggestions(&s), "accepted").is_some(), "the system stopping it is no regret");
        let mut s2 = State::default();
        task(&mut s2, 1, "1");
        s2.apply(&ev(2, "kernel", json!({ "kind": "tool_result", "run": "r1", "call_id": "c1", "status": "ok", "is_error": false, "label": { "private": false, "untrusted": false }, "data": { "card": "k1", "suggestion": true } })));
        s2.apply(&ev(3, "person:alice", json!({ "kind": "card_accepted", "card": "k1", "person": "alice", "task": "t2" })));
        task(&mut s2, 4, "2");
        s2.apply(&ev(5, "person:alice", json!({ "kind": "cancelled", "run": "r2" })));
        assert!(row(&suggestions(&s2), "accepted, later regretted").is_some());
    }

    #[test]
    fn takeovers_are_counted_per_task_by_cause() {
        let mut s = State::default();
        task(&mut s, 1, "1");
        task(&mut s, 2, "2");
        let at = chrono::Utc::now();
        s.apply(&ev(
            3,
            "person:alice",
            json!({ "kind": "computer", "change": "takeover_started", "id": "c", "by": "alice", "at": at, "cause": "the person stepped in", "runs": ["r1"] }),
        ));
        s.apply(&ev(4, "person:alice", json!({ "kind": "computer", "change": "takeover_started", "id": "c", "by": "alice", "at": at })));
        let p = takeovers(&s);
        assert_eq!(p["value"], 1.0);
        assert_eq!(row(&p, "the person stepped in").map(|r| r["runs"].clone()), Some(json!(["r1"])));
        assert!(row(&p, "not recorded").is_some(), "a takeover from before the cause was kept went missing");
    }
}
