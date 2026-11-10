//! What a person's own rules do at the boundary. A block stops the action
//! before anything is asked. An ask first goes to the person, whatever a
//! policy, a grant, an allow rule or Auto Review would say, because ask
//! first wins over allow (spec rev 140, Rules).
use crate::boundary::{Action, Facts, Shown, Verdict};
use crate::kernel::Kernel;
use crate::policy::{self, ToolFacts};
use crate::rules::{Effect, Rule};
use crate::state::Run;
use anyhow::Result;

/// An ask raised by the person's own rule: `person_rule:<id>`.
pub const PERSON_RULE: &str = "person_rule:";
/// The tool through which the agent asks for a new source: a boundary
/// action Auto Review judges like any other, never an ask of its own.
pub const SOURCE_TOOL: &str = "add_computer_destination";

type Ask = Option<(String, Option<String>)>;

/// " (their words)" when the rule has words.
fn quoted_words(r: &Rule) -> String {
    r.words.as_deref().map(|w| format!(": \"{w}\"")).unwrap_or_default()
}

/// Whether one live rule's match takes in this action. A rule naming
/// destinations matches when any of the action's does, since it only
/// stops or asks; one naming none matches wherever the action goes.
fn takes_in(r: &Rule, person: &crate::config::Person, facts: &ToolFacts, destinations: &[String]) -> bool {
    let Some(c) = r.compiled.as_ref() else { return false };
    if c.destinations.is_empty() {
        return policy::rule_covers(&c.cedar, person, facts, "").unwrap_or(false);
    }
    destinations.iter().any(|d| policy::rule_covers(&c.cedar, person, facts, d).unwrap_or(false))
}

/// The live rule that decides first among those that stop or ask: a block
/// over an ask. Allow rules are not here; they only cover asks.
fn strongest(rules: Vec<Rule>, person: &crate::config::Person, facts: &ToolFacts, destinations: &[String]) -> Option<Rule> {
    let live: Vec<Rule> = rules.into_iter().filter(|r| r.live() && r.effect() != Effect::Allow && takes_in(r, person, facts, destinations)).collect();
    live.iter().find(|r| r.effect() == Effect::Block).or_else(|| live.first()).cloned()
}

impl Kernel {
    /// The person's own block or ask-first rule that takes in this action.
    pub(crate) fn stopping_rule(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>) -> Option<Rule> {
        let person = self.person(&run.person)?;
        let facts = ToolFacts { name: &f.name, app: &f.app, risk: f.risk, audience: f.audience };
        strongest(self.read(|s| s.rules.of(&run.person)), &person, &facts, &crate::rules::destinations_of(a, shown))
    }

    /// The kernel's own ask, after the person's rules: a block denies, and
    /// an ask first is asked unless the kernel already asks.
    pub(crate) async fn person_rules(
        &self,
        a: &Action,
        run: &Run,
        f: &Facts,
        shown: Option<&Shown>,
        policy: std::result::Result<Ask, Verdict>,
    ) -> Result<std::result::Result<Ask, Verdict>> {
        let ask = match policy {
            Ok(ask) => ask,
            Err(v) => return Ok(Err(v)),
        };
        let found = self.stopping_rule(a, run, f, shown);
        if let Some(r) = found.as_ref().filter(|r| r.effect() == Effect::Block) {
            let why = format!("{} is blocked by the person's own rule{}", a.tool, quoted_words(r));
            return Ok(Err(self.deny(a, "policy", &why, false, Some(format!("{PERSON_RULE}{}", r.id))).await?));
        }
        if ask.as_ref().is_some_and(|(_, rule)| rule.as_deref().is_some_and(|r| r.starts_with("kernel:"))) {
            return Ok(Ok(ask));
        }
        if let Some(r) = found {
            return Ok(Ok(Some((format!("your rule says to ask first{}", quoted_words(&r)), Some(format!("{PERSON_RULE}{}", r.id))))));
        }
        Ok(Ok(ask))
    }

    /// The person's rules as Auto Review reads them, words and match.
    pub(crate) fn rule_texts(&self, person: &str) -> Vec<String> {
        let rules = self.read(|s| s.rules.of(person));
        rules
            .into_iter()
            .filter(|r| r.accepted)
            .map(|r| {
                let effect = match r.effect() {
                    Effect::Allow => "allow automatically",
                    Effect::Ask => "ask first",
                    Effect::Block => "block",
                };
                let matched = r.compiled.as_ref().map(|c| format!(" [{} {} {}]", c.connector, c.tool, c.destinations.join(","))).unwrap_or_default();
                format!("{effect}: {}{matched}", r.words.as_deref().unwrap_or(""))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rule names a source by its host, so an allow rule can cover it.
    #[test]
    fn a_source_is_matched_by_its_host() {
        let a = Action {
            run: "r".into(),
            call_id: "c".into(),
            step_id: "s".into(),
            tool: SOURCE_TOOL.into(),
            input: serde_json::json!({ "host": "Data.Example" }),
            issuer: crate::boundary::Issuer::Model,
        };
        assert_eq!(crate::rules::destinations_of(&a, None), vec!["data.example".to_string()]);
        let alice: crate::config::Person = serde_json::from_value(serde_json::json!({ "id": "alice", "groups": [] })).unwrap();
        let facts = ToolFacts { name: SOURCE_TOOL, app: "computer", risk: crate::tools::Risk::ReversibleWrite, audience: crate::tools::Audience::Public };
        let allow = crate::rules::compile("alice", "computer", SOURCE_TOOL, &["data.example".into()], false, Effect::Allow).unwrap();
        assert!(policy::rule_covers(&allow.cedar, &alice, &facts, "data.example").unwrap());
        assert!(!policy::rule_covers(&allow.cedar, &alice, &facts, "other.example").unwrap());
    }

    fn rule(id: &str, effect: Effect, tool: &str, to: &[&str], accepted: bool) -> Rule {
        let to: Vec<String> = to.iter().map(|s| s.to_string()).collect();
        let compiled = crate::rules::compile("alice", "slack", tool, &to, false, effect).unwrap();
        Rule { id: id.into(), person: "alice".into(), compiled: Some(compiled), words: None, accepted, used: 0 }
    }

    /// Block over ask, ask over allow; an unaccepted rule or one that does
    /// not match decides nothing.
    #[test]
    fn a_block_wins_over_an_ask_and_an_ask_over_an_allow() {
        let alice: crate::config::Person = serde_json::from_value(serde_json::json!({ "id": "alice", "groups": [] })).unwrap();
        let post = ToolFacts { name: "slack__post", app: "slack", risk: crate::tools::Risk::Irreversible, audience: crate::tools::Audience::Internal };
        let to = vec!["#general".to_string()];
        let allow = rule("allow", Effect::Allow, "slack__post", &["#general"], true);
        let ask = rule("ask", Effect::Ask, "", &[], true);
        let block = rule("block", Effect::Block, "slack__post", &["#general"], true);
        let picked = |rules: Vec<Rule>, to: &[String]| strongest(rules, &alice, &post, to).map(|r| r.id);
        assert_eq!(picked(vec![allow.clone(), ask.clone(), block.clone()], &to), Some("block".into()));
        assert_eq!(picked(vec![allow.clone(), ask.clone()], &to), Some("ask".into()), "an allow beat ask first");
        assert_eq!(picked(vec![allow.clone()], &to), None, "an allow stopped something");
        assert_eq!(picked(vec![block.clone()], &["#random".to_string()]), None, "a block reached another channel");
        assert_eq!(picked(vec![rule("p", Effect::Block, "", &[], false)], &to), None, "an unaccepted block decided");
    }
}
