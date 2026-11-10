//! What only the developer view shows (spec rev 140, What users and
//! developers see): why an ask asked, the rule, and the reviewer's reason.
//! The card a person sees carries none of it, so a test that checks them
//! reads them here, as a developer.

use magnus_invariants::appliance::{Api, Person, Spec};
use serde_json::Value;

/// The developer the tests read the run inspector as.
pub const DEVELOPER: &str = "devon";

/// `s` with a developer who answers nothing and sees every run's reasoning.
pub fn with_developer(mut s: Spec) -> Spec {
    s.people.push(Person { id: DEVELOPER.into(), name: "Devon".into(), token: "tok-devon-developer".into(), groups: vec!["staff".into()], roles: vec!["developer".into()] });
    s
}

/// Why the ask `item` on `run` asked, as the run inspector shows it:
/// `reason` (which of the four), `rule`, `why` and `taint`.
pub async fn ask_reasoning(dev: &Api, run: &str, item: &str) -> Value {
    let (st, v) = dev.get(&format!("/v1/dev/runs/{run}")).await;
    assert!(st.is_success(), "the run inspector refused: {st} {v}");
    let rows = v["harness"].as_array().cloned().unwrap_or_default();
    rows.into_iter()
        .find(|h| h["item"] == item && h["reasoning"].is_object())
        .map(|h| h["reasoning"].clone())
        .unwrap_or_else(|| panic!("the inspector shows no reasoning for {item}: {v}"))
}
