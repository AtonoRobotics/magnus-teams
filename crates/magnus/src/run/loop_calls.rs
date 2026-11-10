//! The agent loop's own model calls, apart from the kernel's modules.
//!
//! A run's `max_model_calls` bounds the agent's steps. The verifier, the
//! reviewer, the memory pass, knowledge extraction, read aloud and scoped
//! drafting are optional extras (#319): they are charged to the run's
//! dollars and skip themselves once the cap is reached, but they never use
//! up the steps the agent needs to finish. Before, a scheduled run with a
//! three-call budget was stopped after two of its own calls because the
//! verifier's criteria review took the third.

use crate::state::Run;

impl Run {
    /// Model calls the agent loop itself made, retries included.
    pub fn loop_calls(&self) -> u32 {
        self.model_calls.saturating_sub(self.aux_calls)
    }

    /// Whether the agent loop has used every call its budget allows.
    pub fn loop_calls_spent(&self) -> bool {
        self.loop_calls() >= self.config.budget.max_model_calls
    }
}

#[cfg(test)]
mod tests {
    use crate::log::Event;
    use crate::state::State;
    use serde_json::json;

    fn ev(seq: i64, actor: &str, ev: serde_json::Value) -> Event {
        Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
    }

    fn ctx() -> serde_json::Value {
        json!({ "through_seq": 1, "instructions": "agent.md", "tools_hash": "h" })
    }

    /// The scheduled smoke test of 2026-11-10: two agent calls and the
    /// verifier's criteria review against a three-call budget. The agent
    /// still has its third call; the verifier's cost stays on the run.
    #[test]
    fn a_verifier_call_does_not_use_up_the_agents_steps() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 3, "max_actions": 1, "max_usd": 0.5 }, "verifier": true, "instructions": "", "harness": "" });
        s.apply(&ev(1, "schedule:s1", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say it ran.", "config": config })));
        let usage = json!({ "input_tokens": 10, "output_tokens": 1, "cost_usd": 0.01 });
        s.apply(&ev(2, "kernel", json!({ "kind": "model_call", "run": "r1", "step": 0, "attempt": 0, "context": ctx() })));
        s.apply(&ev(3, "verifier", json!({ "kind": "verifier_reply", "run": "r1", "purpose": "criteria", "model": "a/m", "usage": usage })));
        s.apply(&ev(4, "kernel", json!({ "kind": "model_call", "run": "r1", "step": 1, "attempt": 0, "context": ctx() })));
        let r = &s.runs["r1"];
        assert_eq!((r.model_calls, r.loop_calls(), r.usd), (3, 2, 0.01));
        assert!(!r.loop_calls_spent(), "the verifier's call stopped the agent before its last step");
        s.apply(&ev(5, "kernel", json!({ "kind": "model_call", "run": "r1", "step": 2, "attempt": 0, "context": ctx() })));
        assert!(s.runs["r1"].loop_calls_spent(), "the agent's own third call reaches the cap");
    }
}
