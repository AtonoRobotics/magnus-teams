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

/// Who set the rule, as its ask or denial says it: the workspace's, or `own`.
fn whose(r: &Rule, own: &'static str) -> &'static str {
    if r.id.starts_with(crate::admin_app::settings::WORKSPACE_RULE) {
        "the workspace's rule"
    } else {
        own
    }
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
    /// The person's own block or ask-first rule that takes in this action,
    /// where it goes read as its card reads it (`destinations_of`), a site
    /// taking in the sites below it. It holds wherever the action runs, an
    /// `OpenWeb` worker's web reads included.
    pub(crate) fn stopping_rule(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>) -> Option<Rule> {
        let person = self.person(&run.person)?;
        let facts = ToolFacts::from(f);
        let destinations = crate::asks::with_parent_sites(&self.destinations_of(a, f, shown));
        strongest(self.rules_for(&run.person), &person, &facts, &destinations)
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
        if self.person(&run.person).is_some_and(|p| crate::people::rights(&p).held_to_guest_limits()) {
            let why = format!("{} is refused: a guest reads and chats only", a.tool);
            return Ok(Err(self.deny(a, "policy", &why, false, Some(crate::admin_app::roles::GUEST_RULE.into())).await?));
        }
        let found = self.stopping_rule(a, run, f, shown);
        if let Some(r) = found.as_ref().filter(|r| r.effect() == Effect::Block) {
            let why = format!("{} is blocked by {}{}", a.tool, whose(r, "the person's own rule"), quoted_words(r));
            return Ok(Err(self.deny(a, "policy", &why, false, Some(format!("{PERSON_RULE}{}", r.id))).await?));
        }
        if ask.as_ref().is_some_and(|(_, rule)| rule.as_deref().is_some_and(|r| r.starts_with("kernel:"))) {
            return Ok(Ok(ask));
        }
        if let Some(r) = found {
            // A web research worker puts no card to anyone (2026-11-22): there
            // the person's ask first stops the action, and the model is told why.
            if self.read(|s| s.compartments.of(&run.id).is_open_web()) {
                let why = format!(
                    "{} is stopped: {}{} says to ask the person first, and a web research worker asks no one. Leave it out, or say in your answer that the person's rule stopped it",
                    a.tool,
                    whose(&r, "the person's own rule"),
                    quoted_words(&r)
                );
                return Ok(Err(self.deny(a, "policy", &why, false, Some(format!("{PERSON_RULE}{}", r.id))).await?));
            }
            return Ok(Ok(Some((format!("{} says to ask first{}", whose(&r, "your rule"), quoted_words(&r)), Some(format!("{PERSON_RULE}{}", r.id))))));
        }
        Ok(Ok(ask))
    }

    /// The person's rules as Auto Review reads them, words and match.
    pub(crate) fn rule_texts(&self, person: &str) -> Vec<String> {
        let rules = self.rules_for(person);
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
        let f = crate::boundary::Facts {
            name: SOURCE_TOOL.into(),
            app: "computer".into(),
            risk: crate::tools::Risk::ReversibleWrite,
            label: crate::tools::Label::PRINCIPAL,
            audience: crate::tools::Audience::Public,
            connector: None,
            remote: SOURCE_TOOL.into(),
            reaches_self: false,
        };
        assert_eq!(crate::asks::destinations(&a, &f, None), vec!["data.example".to_string()]);
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

    /// A workspace rule caps a person's own: its block stops what their allow
    /// covers, its ask first asks over their allow, and it is named as the
    /// workspace's.
    #[test]
    fn a_workspace_rule_caps_a_persons_own() {
        use crate::admin_app::settings::{workspace_rule, WorkspaceRule};
        let alice: crate::config::Person = serde_json::from_value(serde_json::json!({ "id": "alice", "groups": [] })).unwrap();
        let post = ToolFacts { name: "slack__post", app: "slack", risk: crate::tools::Risk::Irreversible, audience: crate::tools::Audience::Internal };
        let to = vec!["#general".to_string()];
        let own = rule("own", Effect::Allow, "slack__post", &["#general"], true);
        let ws = |effect| workspace_rule("alice", "w", &WorkspaceRule { effect, connector: "slack".into(), ..Default::default() });
        let picked = strongest(vec![own.clone(), ws(Effect::Block)], &alice, &post, &to).unwrap();
        assert_eq!((picked.id.as_str(), picked.effect()), ("workspace:w", Effect::Block), "a person's allow beat the workspace's block");
        assert_eq!(whose(&picked, "your rule"), "the workspace's rule");
        assert_eq!(strongest(vec![own.clone(), ws(Effect::Ask)], &alice, &post, &to).map(|r| r.effect()), Some(Effect::Ask), "a person's allow beat the workspace's ask first");
        let bob: crate::config::Person = serde_json::from_value(serde_json::json!({ "id": "bob", "groups": [] })).unwrap();
        let for_bob = workspace_rule("bob", "w", &WorkspaceRule { effect: Effect::Block, connector: "slack".into(), ..Default::default() });
        assert!(strongest(vec![for_bob], &bob, &post, &to).is_some(), "a workspace rule did not reach everyone");
        assert_eq!(whose(&own, "your rule"), "your rule");
    }

    /// A person's block on a site stops a web fetch there wherever it runs:
    /// in their own run and in an `OpenWeb` research worker. Their ask first
    /// on a site asks in their own run, and stops the fetch in the worker,
    /// which asks no one, telling the model why. Before, a rule read only
    /// email-style fields, so a rule on a site never fired (2026-11-23).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rule_on_a_site_holds_in_a_private_run_and_an_openweb_worker() {
        use crate::compartment::{Change as Started, Compartment};
        use crate::events::Ev;
        use serde_json::json;
        let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
        for (id, effect, site) in [("block", Effect::Block, "https://evil.example"), ("ask", Effect::Ask, "ask.example")] {
            let compiled = Some(crate::rules::compile("mo", "web", "", &[site.into()], false, effect).unwrap());
            s.k.emit("person:mo", Ev::Rule(crate::rules::Change::Proposed { id: id.into(), person: "mo".into(), compiled, words: None })).await.unwrap();
            s.k.emit("person:mo", Ev::Rule(crate::rules::Change::Accepted { id: id.into() })).await.unwrap();
        }
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
        let created =
            |t: &str| serde_json::from_value(json!({ "kind": "task_created", "task": format!("t_{t}"), "run": format!("r_{t}"), "person": "mo", "config": config })).unwrap();
        s.k.emit("person:mo", created("own")).await.unwrap();
        let worker = Compartment::OpenWeb { link_following_only: false };
        s.k.emit("kernel", Ev::Compartment(Started::Started { run: "r_web".into(), parent: Some("r_own".into()), compartment: worker })).await.unwrap();
        s.k.emit("kernel", Ev::SubAgent(crate::sub_agents::SubAgent { child: "r_web".into(), parent: "r_own".into(), tools: vec![] })).await.unwrap();
        s.k.emit("run:r_own", created("web")).await.unwrap();
        let fetch = |run: &str, n: usize, url: &str| Action {
            run: run.into(),
            call_id: format!("c{n}"),
            step_id: format!("s{n}.0"),
            tool: "web_fetch".into(),
            input: json!({ "url": url }),
            issuer: crate::boundary::Issuer::Model,
        };
        for (n, run) in [(1, "r_own"), (2, "r_web")] {
            match s.k.execute(fetch(run, n, "https://www.evil.example/page")).await.unwrap() {
                crate::boundary::Verdict::Done { text, is_error: true, .. } => assert!(text.contains("blocked by the person's own rule"), "{run}: {text}"),
                _ => panic!("{run}: the block on evil.example did not stop the fetch"),
            }
        }
        match s.k.execute(fetch("r_web", 3, "https://ask.example/")).await.unwrap() {
            crate::boundary::Verdict::Done { text, is_error: true, .. } => assert!(text.contains("asks no one"), "{text}"),
            _ => panic!("the worker did not stop at the person's ask first"),
        }
        assert!(matches!(s.k.execute(fetch("r_own", 4, "https://ask.example/")).await.unwrap(), crate::boundary::Verdict::Suspended), "the person's own run did not ask");
        s.done().await;
    }
}
