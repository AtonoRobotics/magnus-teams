use super::*;
use crate::log::Event;
use serde_json::json;

fn said(kind: Kind, text: &str) -> Entry {
    Entry::said(ChatEntry { id: format!("{}:t1", kind.as_str()), kind: kind.as_str(), text: text.into(), task_id: Some("t1".into()), ..Default::default() })
}

fn row(task: &str, title: &str, text: &str) -> TaskRow {
    TaskRow {
        task_id: task.into(),
        title: title.into(),
        text: text.into(),
        status: Status::Running,
        waiting_on: None,
        waiting_item: None,
        status_line: None,
        requester: None,
        steps: None,
        doing: None,
        long: None,
        thread: None,
        started: None,
    }
}

/// What the message actions, the reactions and the next turn read from an
/// entry are its methods, with no kernel and no JSON.
#[test]
fn an_entry_says_what_it_shows_without_a_kernel() {
    let m = said(Kind::Message, "Book a table");
    assert_eq!((m.text(), m.speaker(), m.is_magnus()), ("Book a table".to_string(), "You", false));
    assert_eq!(m.transcript_line(), Some(("person", "Book a table".to_string())));
    assert_eq!(m.task_ids(), ["t1"]);
    assert!(m.file_name().ends_with(".md") && m.markdown().contains("Book a table"));
    let mut a = said(Kind::Answer, "Booked for eight.");
    if let Body::Said(c) = &mut a.body {
        c.view = Some(json!([{ "type": "table" }]));
    }
    assert_eq!(a.speaker(), "Magnus");
    assert!(a.is_magnus());
    assert_eq!(a.transcript_line().map(|l| l.1), Some("Booked for eight.\n[Shown below the answer: [{\"type\":\"table\"}]]".to_string()));
    let tasks = Entry::said(ChatEntry {
        id: "t:r1".into(),
        kind: Kind::Tasks.as_str(),
        from_run: Some("r1".into()),
        tasks: Some(vec![row("t2", "Flights", "Find flights"), row("t3", "Hotel", "Find a hotel")]),
        ..Default::default()
    });
    assert_eq!(tasks.task_ids(), ["t2", "t3"]);
    assert_eq!(tasks.from_run(), Some("r1"));
    assert_eq!(tasks.text(), "- Flights: Find flights\n- Hotel: Find a hotel");
    assert_eq!(tasks.transcript_line().map(|l| l.1), Some("Started tasks: Flights (Find flights); Hotel (Find a hotel)".to_string()));
    let result = TaskResult {
        title: "Flights".into(),
        text: "Find flights".into(),
        status: "completed",
        summary: Some("Three found.".into()),
        view: None,
        files: vec![],
        verdict: None,
        evidence: Value::Null,
    };
    let r = Entry { id: "r:t2".into(), kind: Kind::Result, at: None, task_id: Some("t2".into()), may: None, reaction: None, body: Body::Result(result) };
    assert_eq!((r.text(), r.speaker()), ("Flights (completed)\n\nThree found.".to_string(), "Task result"));
    assert_eq!(r.transcript_line(), Some(("task result", "Flights (completed): Three found.".to_string())));
    let mut d = Map::new();
    d.insert("card".into(), json!("c1"));
    d.insert("title".into(), json!("Book it"));
    d.insert("plan".into(), json!("Book the table"));
    d.insert("state".into(), json!("declined"));
    d.insert("reason".into(), json!("too late"));
    let card = Entry::card("c:c1".into(), Kind::SuggestionCard, "t1".into(), d);
    assert_eq!(card.transcript_line().map(|l| l.1), Some("Suggested Book it (declined: \"too late\", card c1): Book the table".to_string()));
    let turn = Turn { status: "running", waiting_on: None, waiting_item: None, text: None };
    let t = Entry { id: "a:t4".into(), kind: Kind::Turn, at: None, task_id: Some("t4".into()), may: None, reaction: None, body: Body::Turn(turn) };
    assert_eq!((t.transcript_line(), t.said_text()), (None, None), "a turn with no answer says nothing yet");
}

/// The list of Magnus's kinds is one: what takes a reaction is what the
/// person may judge, and the person's own messages are neither.
#[test]
fn magnus_has_one_list_of_its_kinds() {
    for k in Kind::ALL {
        assert_eq!(May::personal(k).feedback, k.is_magnus(), "{k:?}");
        assert_eq!(Kind::parse(k.as_str()), k, "{k:?} does not travel as itself");
    }
    let magnus: Vec<&str> = Kind::ALL.into_iter().filter(|k| k.is_magnus()).map(Kind::as_str).collect();
    assert_eq!(magnus, ["opener", "answer", "turn", "result", "tasks", "suggestion_card", "schedule_card", "grant_card", "connector_card"]);
    assert_eq!(Kind::parse("work"), Kind::Other);
}

/// A branch stored before entries were typed, or before they carried what
/// the person may do, replays as it was stored, marked as copied: its
/// fields stay (a null one too), and its text, speaker and tasks read as
/// they did.
#[test]
fn old_thread_branched_entries_still_replay() {
    let stored = json!({ "id": "r:t0", "kind": "result", "title": "Flights", "status": "failed", "summary": null, "task_id": "t0", "at": "2026-11-01T10:00:00Z", "legacy": 1 });
    let e = Entry::copied(stored.clone());
    assert_eq!((e.kind, e.id.as_str(), e.task_ids()), (Kind::Result, "r:t0", vec!["t0".to_string()]));
    assert_eq!(e.text(), "Flights (failed)\n\n");
    assert_eq!(e.at_text(), "2026-11-01T10:00:00Z");
    let mut want = stored;
    want["copied"] = json!(true);
    assert_eq!(e.value(), want);
    let tasks = Entry::copied(json!({ "id": "t:r0", "kind": "tasks", "from_run": "r0", "tasks": [{ "task_id": "t5", "title": "A", "text": "b" }] }));
    assert_eq!((tasks.task_ids(), tasks.from_run()), (vec!["t5".to_string()], Some("r0")));
    let retired = Entry::copied(json!({ "id": "x:1", "kind": "some_retired_kind", "text": "kept" }));
    assert_eq!((retired.kind, retired.text(), retired.transcript_line()), (Kind::Other, "kept".to_string(), None));
    // One stored without an id cannot be named, so it takes no actions.
    assert!(!Entry::copied(json!({ "kind": "message" })).has_id());
}

fn ev(seq: i64, actor: &str, ev: Value) -> Event {
    Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
}

fn created(seq: i64, task: &str, run: &str, chat: Value) -> Event {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    ev(seq, "person:alice", json!({ "kind": "task_created", "task": task, "run": run, "person": "alice", "text": "x", "config": config, "chat": chat }))
}

/// A sub-agent a chat turn starts, and one it starts in turn, show in that
/// chat as one entry: each one's depth, status and result.
#[test]
fn a_turns_sub_agents_are_one_entry_of_its_conversation() {
    let mut s = State::default();
    s.apply(&created(1, "turn", "rt", json!({ "thread": "th", "turn": true })));
    s.apply(&created(2, "c1", "r1", Value::Null));
    s.apply(&ev(3, "kernel", json!({ "kind": "sub_agent", "child": "r1", "parent": "rt", "tools": [] })));
    s.apply(&created(4, "c2", "r2", Value::Null));
    s.apply(&ev(5, "kernel", json!({ "kind": "sub_agent", "child": "r2", "parent": "r1", "tools": ["web_fetch"] })));
    s.apply(&ev(6, "kernel", json!({ "kind": "completed", "run": "r2", "summary": "Found three flights." })));
    s.apply(&created(7, "job", "rj", Value::Null));
    s.apply(&created(8, "c3", "r3", Value::Null));
    s.apply(&ev(9, "kernel", json!({ "kind": "sub_agent", "child": "r3", "parent": "rj", "tools": [] })));
    let (_, _, e) = sub_agents_entry(&s, &s.tasks["turn"], &s.runs["rt"], &Words::default()).expect("no entry for the turn's sub-agents");
    assert_eq!((e.kind, e.from_run()), (Kind::SubAgents, Some("rt")));
    let Body::SubAgents(a) = &e.body else { panic!("not a sub-agents entry") };
    assert_eq!(a.agents.iter().map(|a| (a.run.as_str(), a.depth)).collect::<Vec<_>>(), [("r1", 1), ("r2", 2)]);
    assert_eq!((a.agents[0].status, a.agents[1].status), ("running", "completed"));
    assert_eq!(a.agents[1].summary.as_deref(), Some("Found three flights."));
    let job = sub_agents_entry(&s, &s.tasks["job"], &s.runs["rj"], &Words::default());
    assert!(job.is_some_and(|(_, _, e)| matches!(e.body, Body::SubAgents(ref a) if a.agents.len() == 1)));
}
