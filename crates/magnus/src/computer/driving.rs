//! Who is driving a computer, and the agent asking its person to take over
//! (spec, User takeover and handback: status, and agent-requested takeover).
//!
//! The request is an inbox item, `takeover_request`, so it is in the log,
//! in the person's inbox and decided like any other. The run waits on it.
//! The person takes control and hands back, which closes it, or answers it
//! from the card: done without taking control, or declined.
use super::{takeover::runs_on, Computer};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{Item, Run, State, Status as RunStatus};
use crate::tools::obj;
use anyhow::Result;
use magnus_adapter_proto::ToolSpec;
use serde_json::{json, Value};

pub(crate) const REQUEST_TOOL: &str = "computer_request_takeover";
pub(crate) const REQUEST_KIND: &str = "takeover_request";
/// What the run waits with while its request is open.
pub(crate) const REQUEST_REASON: &str = "takeover_request";
/// Enough to say what is on the screen and what to do; no essay.
const MAX_WHY_CHARS: usize = 500;
/// The steps the spec names that only a person can do, and how a card says each.
const REASONS: [(&str, &str); 7] = [
    ("sso", "a single sign-on"),
    ("passkey", "a passkey"),
    ("captcha", "a CAPTCHA"),
    ("other_device", "an approval on another device"),
    ("security_key", "a hardware security key"),
    ("password_change", "a password change"),
    ("other", "a step only you can do"),
];

pub(crate) fn request_spec(d: &dyn Fn(&str) -> String) -> ToolSpec {
    let reasons: Vec<&str> = REASONS.iter().map(|(r, _)| *r).collect();
    ToolSpec {
        name: REQUEST_TOOL.into(),
        description: d(REQUEST_TOOL),
        input_schema: obj(json!({ "reason": { "type": "string", "enum": reasons }, "why": { "type": "string" } }), &["reason", "why"]),
    }
}

/// The run's open request, if it has one.
pub(super) fn open_request<'a>(s: &'a State, run: &str) -> Option<&'a Item> {
    s.items.values().find(|i| i.kind == REQUEST_KIND && i.resolution.is_none() && i.run.as_deref() == Some(run))
}

/// The request the agent makes, checked: a reason the spec names and a short why.
pub(super) fn request_item(id: &str, run: &Run, computer: &str, input: &Value) -> Result<Ev, String> {
    let reason = input["reason"].as_str().unwrap_or_default();
    let Some((_, said)) = REASONS.iter().find(|(r, _)| *r == reason) else {
        return Err(format!("reason must be one of {}", REASONS.map(|(r, _)| r).join(", ")));
    };
    let why = input["why"].as_str().unwrap_or_default().trim();
    if why.is_empty() || why.chars().count() > MAX_WHY_CHARS {
        return Err(format!("why says, in at most {MAX_WHY_CHARS} characters, what the person should do"));
    }
    Ok(Ev::Item {
        id: id.to_string(),
        item_kind: REQUEST_KIND.into(),
        task: Some(run.task.clone()),
        run: Some(run.id.clone()),
        person: run.person.clone(),
        summary: format!("The agent asks you to take over your computer for {said}."),
        details: json!({ "computer": computer, "reason": reason, "why": why }),
    })
}

/// Why a person took control, as the measured outcomes count it.
pub(super) fn cause_of(item: &Item) -> String {
    match item.kind.as_str() {
        REQUEST_KIND => format!("the agent asked: {}", item.details["reason"].as_str().unwrap_or("other")),
        kind => format!("the agent was waiting on {kind}"),
    }
}

/// The open requests a handback answers: those of the runs it resumes, and
/// any made on this computer.
pub(super) fn answered_by_handback(s: &State, computer: &str, runs: &[String]) -> Vec<String> {
    s.items
        .values()
        .filter(|i| i.kind == REQUEST_KIND && i.resolution.is_none())
        .filter(|i| i.details["computer"] == json!(computer) || i.run.as_ref().is_some_and(|r| runs.contains(r)))
        .map(|i| i.id.clone())
        .collect()
}

/// Who is driving: a person holding the screen, the agent while a run is
/// using it, or no one. With the runs on it and an open request, if any.
pub(super) fn driving(s: &State, c: &Computer) -> Value {
    let uses = |r: &Run| r.replies.iter().any(|reply| reply.calls().iter().any(|(_, name, _)| super::takeover::is_capture(name)));
    let on_it: Vec<&Run> = runs_on(s, &c.owner).filter(|r| !r.status.terminal() && uses(r)).collect();
    let agent = on_it.iter().any(|r| r.status == RunStatus::Running);
    let by = match (&c.controller, agent) {
        (Some(_), _) => "person",
        (None, true) => "agent",
        (None, false) => "no_one",
    };
    let request = s
        .items
        .values()
        .find(|i| i.kind == REQUEST_KIND && i.resolution.is_none() && i.details["computer"] == json!(c.id))
        .map(|i| json!({ "item": i.id, "reason": i.details["reason"], "why": i.details["why"], "run": i.run, "task": i.task, "at": i.created_at }));
    json!({
        "by": by,
        "person": c.controller,
        "runs": on_it.iter().map(|r| json!({ "run": r.id, "task": r.task, "status": r.status, "reason": r.reason })).collect::<Vec<_>>(),
        "request": request,
    })
}

impl Kernel {
    /// The agent hands the computer to its person for a step it cannot do.
    pub(super) async fn request_takeover(&self, run: &str, input: &Value) -> Result<(String, bool, Option<Value>)> {
        let Some(r) = self.read(|s| s.runs.get(run).cloned()) else { return Ok(("This run is gone.".into(), true, None)) };
        let Some(computer) = self.computer_of_run(&r) else {
            return Ok(("There is no running computer for a person to take over.".into(), true, None));
        };
        if self.is_background(run) {
            return Ok(("Taking over needs a person, and this work has no one to ask.".into(), true, None));
        }
        if let Some(open) = self.read(|s| open_request(s, run).map(|i| i.id.clone())) {
            return Ok((format!("You have already asked (item {open}). This run waits for the person."), false, None));
        }
        let id = crate::kernel::new_id("item");
        let ev = match request_item(&id, &r, &computer, input) {
            Ok(ev) => ev,
            Err(e) => return Ok((e, true, None)),
        };
        self.emit("kernel", ev).await?;
        Ok((
            format!(
                "The person has been asked to take over your computer (item {id}), and this run waits. When you continue, take a fresh screenshot: the screen will have changed."
            ),
            false,
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Event;

    fn ev(seq: i64, actor: &str, ev: Value) -> Event {
        let Ok(ev) = serde_json::from_value(ev.clone()) else { panic!("not an event: {ev}") };
        Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev }
    }

    /// Alice's computer `c`, running, and her task `t1` whose run `r1` has
    /// clicked on it.
    fn world() -> State {
        let mut s = State::default();
        s.apply(&ev(1, "person:alice", json!({ "kind": "computer", "change": "created", "id": "c", "owner": { "person": "alice" } })));
        s.apply(&ev(2, "person:alice", json!({ "kind": "computer", "change": "started", "id": "c" })));
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(3, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Book it.", "config": config })));
        let blocks = json!([{ "type": "tool_call", "id": "u1", "name": "computer_click", "input": { "x": 1, "y": 2 } }]);
        let usage = json!({ "input_tokens": 1, "output_tokens": 1, "cost_usd": 0.0 });
        s.apply(&ev(4, "kernel", json!({ "kind": "model_reply", "run": "r1", "step": 1, "blocks": blocks, "usage": usage, "served_model": "m", "stop": "tool_use" })));
        s
    }

    fn computer(s: &State) -> Computer {
        let Some(c) = s.computers.get("c") else { panic!("no computer") };
        c.clone()
    }

    #[test]
    fn the_agent_drives_until_a_person_takes_control() {
        let mut s = world();
        assert_eq!(driving(&s, &computer(&s))["by"], "agent", "{}", driving(&s, &computer(&s)));
        assert_eq!(driving(&s, &computer(&s))["runs"][0]["task"], "t1");
        let at = chrono::Utc::now();
        s.apply(&ev(5, "person:alice", json!({ "kind": "computer", "change": "takeover_started", "id": "c", "by": "alice", "at": at, "runs": ["r1"] })));
        let d = driving(&s, &computer(&s));
        assert_eq!((d["by"].clone(), d["person"].clone()), (json!("person"), json!("alice")));
        s.apply(&ev(6, "person:alice", json!({ "kind": "computer", "change": "takeover_ended", "id": "c", "by": "alice", "at": at })));
        s.apply(&ev(7, "kernel", json!({ "kind": "completed", "run": "r1" })));
        assert_eq!(driving(&s, &computer(&s))["by"], "no_one");
    }

    #[test]
    fn a_request_names_a_reason_the_spec_gives_and_what_to_do() {
        let s = world();
        let Some(run) = s.runs.get("r1") else { panic!("no run") };
        assert!(request_item("i", run, "c", &json!({ "reason": "lunch", "why": "x" })).is_err(), "an unnamed reason was taken");
        assert!(request_item("i", run, "c", &json!({ "reason": "captcha", "why": " " })).is_err(), "a request with no why was taken");
        assert!(request_item("i", run, "c", &json!({ "reason": "captcha", "why": "x".repeat(MAX_WHY_CHARS + 1) })).is_err());
        let Ok(Ev::Item { item_kind, person, details, summary, .. }) = request_item("i", run, "c", &json!({ "reason": "captcha", "why": "Solve the puzzle." })) else {
            panic!("a good request was refused")
        };
        assert_eq!((item_kind.as_str(), person.as_str()), (REQUEST_KIND, "alice"));
        assert_eq!(details, json!({ "computer": "c", "reason": "captcha", "why": "Solve the puzzle." }));
        assert!(summary.contains("a CAPTCHA"), "{summary}");
    }

    #[test]
    fn an_open_request_shows_on_the_computer_and_a_handback_answers_it() {
        let mut s = world();
        let Some(run) = s.runs.get("r1").cloned() else { panic!("no run") };
        let Ok(item) = request_item("i1", &run, "c", &json!({ "reason": "passkey", "why": "Touch your key." })) else { panic!("refused") };
        let Ok(item) = serde_json::to_value(item) else { panic!("unserializable") };
        s.apply(&ev(5, "kernel", item));
        assert!(open_request(&s, "r1").is_some(), "the run is not waiting on its request");
        let d = driving(&s, &computer(&s));
        assert_eq!((d["request"]["item"].clone(), d["request"]["reason"].clone()), (json!("i1"), json!("passkey")), "{d}");
        let Some(i) = s.items.get("i1") else { panic!("no item") };
        assert_eq!(cause_of(i), "the agent asked: passkey");
        assert_eq!(answered_by_handback(&s, "c", &[]), vec!["i1".to_string()]);
        assert_eq!(answered_by_handback(&s, "other", &["r1".to_string()]), vec!["i1".to_string()]);
        assert!(answered_by_handback(&s, "other", &[]).is_empty(), "a handback on another computer answered it");
        s.apply(&ev(6, "person:alice", json!({ "kind": "item_resolved", "id": "i1", "decision": "handed_back" })));
        assert!(open_request(&s, "r1").is_none() && driving(&s, &computer(&s))["request"].is_null());
    }

    #[test]
    fn the_request_tool_is_offered_with_its_reasons() {
        let Some(spec) = crate::tools::builtin_specs(false, false, false, true).into_iter().find(|t| t.name == REQUEST_TOOL) else { panic!("the agent cannot ask for a takeover") };
        assert!(!spec.description.is_empty(), "the tool has no description");
        assert_eq!(spec.input_schema["properties"]["reason"]["enum"].as_array().map(Vec::len), Some(REASONS.len()));
        assert!(crate::tools::builtin(REQUEST_TOOL).is_some(), "the tool has no risk");
    }
}
