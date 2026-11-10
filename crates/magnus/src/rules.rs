//! Rules a person accepted, compiled to Cedar. A rule covers an action only
//! by a deterministic match on its connector, tool and every destination:
//! neither the reviewer nor the working model decides that. A rule the model
//! proposed covers nothing until its person accepts it, and words with no
//! compiled match cover nothing at all. A rule allows automatically, asks
//! first or blocks; ask first wins over allow, and block over both. The
//! hard denials are here too, because they are what no rule or grant can
//! reach.
use crate::api::{err, internal, who};
use crate::boundary::{Action, Facts, Issuer, Shown};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::policy::{self, ToolFacts};
use crate::state::Run;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A vault value shorter than this could be an ordinary word.
const MIN_SECRET_BYTES: usize = 8;
/// Input fields that name whose account an action works on.
const ACCOUNT_FIELDS: [&str; 7] = ["account", "account_id", "as_user", "on_behalf_of", "impersonate", "user_id", "mailbox"];

/// What a matching rule does. A rule recorded before effects existed allowed.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    Allow,
    Ask,
    Block,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Compiled {
    pub connector: String,
    /// Empty matches every tool of the connector (ask and block only).
    pub tool: String,
    /// Empty matches every destination (ask and block only).
    pub destinations: Vec<String>,
    pub once: bool,
    pub cedar: String,
    #[serde(default)]
    pub effect: Effect,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Proposed {
        id: String,
        person: String,
        compiled: Option<Compiled>,
        /// The person's own words; content, so it may be erased.
        #[serde(default)]
        words: Option<String>,
    },
    Accepted {
        id: String,
    },
    /// Its person changed what it says or matches. It keeps its acceptance.
    Edited {
        id: String,
        person: String,
        compiled: Option<Compiled>,
        #[serde(default)]
        words: Option<String>,
    },
    Used {
        id: String,
        run: String,
        call_id: String,
    },
    Removed {
        id: String,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct Rule {
    pub id: String,
    pub person: String,
    pub compiled: Option<Compiled>,
    pub words: Option<String>,
    pub accepted: bool,
    pub used: u32,
}

impl Rule {
    pub(crate) fn live(&self) -> bool {
        self.accepted && self.compiled.as_ref().is_some_and(|c| !(c.once && self.used > 0))
    }

    /// What it does when it matches; words alone allow nothing.
    pub fn effect(&self) -> Effect {
        self.compiled.as_ref().map_or(Effect::Allow, |c| c.effect)
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Ledger(BTreeMap<String, Rule>);

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Proposed { id, person, compiled, words } => {
                self.0.insert(id.clone(), Rule { id: id.clone(), person: person.clone(), compiled: compiled.clone(), words: words.clone(), accepted: false, used: 0 });
            }
            Change::Edited { id, compiled, words, .. } => {
                if let Some(r) = self.0.get_mut(id) {
                    r.compiled = compiled.clone();
                    r.words = words.clone();
                    r.used = 0;
                }
            }
            Change::Accepted { id } => {
                if let Some(r) = self.0.get_mut(id) {
                    r.accepted = true;
                }
            }
            Change::Used { id, .. } => {
                if let Some(r) = self.0.get_mut(id) {
                    r.used += 1;
                }
            }
            Change::Removed { id } => {
                self.0.remove(id);
            }
        }
    }

    pub fn of(&self, person: &str) -> Vec<Rule> {
        self.0.values().filter(|r| r.person == person).cloned().collect()
    }

    fn get(&self, id: &str) -> Option<&Rule> {
        self.0.get(id)
    }
}

/// An address as a rule and an action both name it.
fn normalized(destination: &str) -> String {
    crate::tools::addresses_in(destination).into_iter().next().unwrap_or_else(|| destination.trim().to_lowercase())
}

/// A string as Cedar source.
fn quoted(s: &str) -> Result<String, String> {
    serde_json::to_string(s).map_err(|e| e.to_string())
}

/// The match, as Cedar. An allow is narrow: one tool, named destinations.
/// An ask or a block may be as wide as one connector, because widening
/// either only makes the agent do less.
pub fn compile(person: &str, connector: &str, tool: &str, destinations: &[String], once: bool, effect: Effect) -> Result<Compiled, String> {
    if connector.is_empty() {
        return Err("a rule names a connector".to_string());
    }
    if effect == Effect::Allow && (tool.is_empty() || destinations.is_empty()) {
        return Err("a rule that allows names a connector, a tool and at least one destination".to_string());
    }
    let destinations: Vec<String> = destinations.iter().map(|d| normalized(d)).filter(|d| !d.is_empty()).collect();
    if effect == Effect::Allow && destinations.is_empty() {
        return Err("a rule that allows names at least one destination".to_string());
    }
    let scope = if tool.is_empty() { format!("resource in App::{}", quoted(connector)?) } else { format!("resource == Tool::{}", quoted(tool)?) };
    let mut when = vec![format!("resource in App::{}", quoted(connector)?)];
    if !destinations.is_empty() {
        let list = destinations.iter().map(|d| quoted(d)).collect::<Result<Vec<_>, _>>()?.join(", ");
        when.push(format!("[{list}].contains(context.destination)"));
    }
    let cedar = format!("permit(principal == Person::{}, action == Action::\"execute\", {scope}) when {{ {} }};", quoted(person)?, when.join(" && "));
    policy::validate(&cedar).map_err(|e| e.to_string())?;
    let once = once && effect == Effect::Allow;
    Ok(Compiled { connector: connector.to_string(), tool: tool.to_string(), destinations, once, cedar, effect })
}

/// Where an action goes, as a rule would name it. Empty means it cannot match.
/// A new source is named by its host.
pub(crate) fn destinations_of(a: &Action, shown: Option<&Shown>) -> Vec<String> {
    if a.tool == crate::rule_effects::SOURCE_TOOL {
        return a.input["host"].as_str().map(normalized).filter(|h| !h.is_empty()).into_iter().collect();
    }
    match shown {
        Some(s) => s.recipients.values().flatten().map(|r| normalized(r)).collect(),
        None => crate::tools::addresses(&a.input),
    }
}

fn strings<'a>(v: &'a Value, key: &'a str, out: &mut Vec<(&'a str, &'a str)>) {
    match v {
        Value::String(s) => out.push((key, s)),
        Value::Array(a) => a.iter().for_each(|x| strings(x, key, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| strings(x, k, out)),
        _ => {}
    }
}

fn discloses(text: &str, secret: &str) -> bool {
    secret.len() >= MIN_SECRET_BYTES && crate::vault::redact(text, secret) != text
}

/// The tokens inside a stored value: the whole of it, or a sign-in's tokens.
fn secret_values(raw: &str) -> Vec<String> {
    let mut out = vec![raw.to_string()];
    if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(raw) {
        out.extend(o.iter().filter(|(k, _)| k.contains("token")).filter_map(|(_, v)| v.as_str().map(String::from)));
    }
    out
}

impl Kernel {
    /// The accepted rule that covers this action, if one matches exactly.
    pub(crate) fn covering_rule(&self, a: &Action, run: &Run, f: &Facts, shown: Option<&Shown>) -> Option<String> {
        let destinations = destinations_of(a, shown);
        if destinations.is_empty() {
            return None;
        }
        let person = self.person(&run.person)?;
        let facts = ToolFacts { name: &f.name, app: &f.app, risk: f.risk, audience: f.audience };
        let rules = self.read(|s| s.rules.of(&run.person));
        rules.into_iter().filter(|r| r.live() && r.effect() == Effect::Allow).find_map(|r| {
            let cedar = r.compiled.as_ref()?.cedar.clone();
            destinations.iter().all(|d| policy::rule_covers(&cedar, &person, &facts, d).unwrap_or(false)).then_some(r.id)
        })
    }

    /// Denied whatever a policy, grant, rule or approval says, and never
    /// asked: disclosing a credential, or reaching another person's account
    /// or computer. Only what the model asked for is judged here.
    pub(crate) fn hard_denial(&self, a: &Action, run: &Run) -> Option<String> {
        if let Some(why) = self.team_refusal(a, run) {
            return Some(why);
        }
        if a.issuer != Issuer::Model {
            return None;
        }
        let text = a.input.to_string();
        for name in self.vault.names().unwrap_or_default() {
            let Ok(Some(secret)) = self.vault.get(&name) else { continue };
            if secret_values(secret.expose()).iter().any(|v| discloses(&text, v)) {
                return Some(format!("{} would disclose a stored credential, which nothing can permit", a.tool));
            }
        }
        let mut found = Vec::new();
        strings(&a.input, "", &mut found);
        for (key, value) in found {
            let foreign_computer = self.read(|s| s.computers.get(value).is_some_and(|c| matches!(&c.owner, crate::computer::Owner::Person(p) if *p != run.person)));
            if foreign_computer {
                return Some(format!("{} reaches another person's computer, which nothing can permit", a.tool));
            }
            if ACCOUNT_FIELDS.contains(&key) && value != run.person && self.person(value).is_some() {
                return Some(format!("{} reaches another person's account, which nothing can permit", a.tool));
            }
        }
        None
    }
}

/// The longest plain-word rule kept.
const MAX_WORDS_CHARS: usize = 2000;

#[derive(Deserialize)]
struct NewRule {
    connector: Option<String>,
    tool: Option<String>,
    #[serde(default)]
    destinations: Vec<String>,
    #[serde(default)]
    once: bool,
    #[serde(default)]
    effect: Effect,
    /// What the person wrote, in their own words.
    words: Option<String>,
    /// Leaves it waiting for its person's acceptance.
    #[serde(default)]
    proposed: bool,
}

impl NewRule {
    /// Its words and its match, or why it cannot be kept.
    fn parts(&self, person: &str) -> Result<(Option<String>, Option<Compiled>), String> {
        let words = self.words.as_deref().map(str::trim).filter(|w| !w.is_empty()).map(String::from);
        if words.as_ref().is_some_and(|w| w.chars().count() > MAX_WORDS_CHARS) {
            return Err(format!("a rule is at most {MAX_WORDS_CHARS} characters"));
        }
        let connector = self.connector.as_deref().map(str::trim).filter(|c| !c.is_empty());
        let compiled = match connector {
            Some(c) => Some(compile(person, c, self.tool.as_deref().unwrap_or_default().trim(), &self.destinations, self.once, self.effect)?),
            None => None,
        };
        if words.is_none() && compiled.is_none() {
            return Err("a rule says something in words or names what it matches".to_string());
        }
        Ok((words, compiled))
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/rules", get(list).post(add)).route("/v1/rules/{id}", put(edit).delete(remove)).route("/v1/rules/{id}/accept", post(accept))
}

async fn list(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(json!({ "rules": k.read(|s| s.rules.of(&p.id)) })).into_response(),
        Err(r) => r,
    }
}

/// Words alone are recorded with nothing compiled, and so cover nothing.
async fn add(State(k): State<Arc<Kernel>>, h: HeaderMap, Json(b): Json<NewRule>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (words, compiled) = match b.parts(&p.id) {
        Ok(x) => x,
        Err(m) => return err(StatusCode::BAD_REQUEST, m),
    };
    let id = crate::kernel::new_id("rule");
    let actor = format!("person:{}", p.id);
    let mut evs = vec![(actor.as_str(), Ev::Rule(Change::Proposed { id: id.clone(), person: p.id.clone(), compiled, words }))];
    if !b.proposed {
        evs.push((actor.as_str(), Ev::Rule(Change::Accepted { id: id.clone() })));
    }
    match k.emit_all(evs).await {
        Ok(_) => Json(json!({ "id": id })).into_response(),
        Err(e) => internal(e),
    }
}

async fn change(k: Arc<Kernel>, h: HeaderMap, id: String, make: impl FnOnce(String) -> Change) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !k.read(|s| s.rules.get(&id).is_some_and(|r| r.person == p.id)) {
        return err(StatusCode::NOT_FOUND, "no such rule");
    }
    match k.emit(&format!("person:{}", p.id), Ev::Rule(make(id.clone()))).await {
        Ok(_) => Json(json!({ "id": id })).into_response(),
        Err(e) => internal(e),
    }
}

/// The person rewrites their rule: new words, a new match, or both.
async fn edit(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>, Json(b): Json<NewRule>) -> Response {
    let person = match who(&k, &h).await {
        Ok(p) => p.id,
        Err(r) => return r,
    };
    let (words, compiled) = match b.parts(&person) {
        Ok(x) => x,
        Err(m) => return err(StatusCode::BAD_REQUEST, m),
    };
    change(k, h, id, |id| Change::Edited { id, person, compiled, words }).await
}

async fn accept(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>) -> Response {
    change(k, h, id, |id| Change::Accepted { id }).await
}

async fn remove(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>) -> Response {
    change(k, h, id, |id| Change::Removed { id }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Audience, Risk};

    fn person(id: &str) -> crate::config::Person {
        serde_json::from_value(json!({ "id": id, "groups": [] })).unwrap()
    }

    fn facts<'a>(name: &'a str, app: &'a str) -> ToolFacts<'a> {
        ToolFacts { name, app, risk: Risk::Irreversible, audience: Audience::Public }
    }

    #[test]
    fn a_rule_matches_only_its_person_connector_tool_and_destination() {
        let c = compile("alice", "drafts", "drafts__send", &["Bob <bob@corp.example>".to_string()], true, Effect::Allow).unwrap();
        assert_eq!(c.destinations, vec!["bob@corp.example".to_string()]);
        let alice = person("alice");
        assert!(policy::rule_covers(&c.cedar, &alice, &facts("drafts__send", "drafts"), "bob@corp.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &alice, &facts("drafts__send", "drafts"), "eve@evil.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &alice, &facts("drafts__wipe", "drafts"), "bob@corp.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &person("bob"), &facts("drafts__send", "drafts"), "bob@corp.example").unwrap());
    }

    #[test]
    fn a_quote_in_a_destination_cannot_widen_the_rule() {
        let c = compile("alice", "drafts", "drafts__send", &["x\" || true || \"".to_string()], false, Effect::Allow).unwrap();
        assert!(!policy::rule_covers(&c.cedar, &person("alice"), &facts("drafts__send", "drafts"), "eve@evil.example").unwrap());
    }

    /// Allow stays narrow; ask and block may cover a whole connector.
    #[test]
    fn only_ask_and_block_may_be_as_wide_as_a_connector() {
        assert!(compile("alice", "slack", "", &[], false, Effect::Allow).is_err(), "an allow covered a whole connector");
        assert!(compile("alice", "slack", "slack__post", &[], false, Effect::Allow).is_err(), "an allow covered every destination");
        let alice = person("alice");
        let block = compile("alice", "slack", "", &[], false, Effect::Block).unwrap();
        assert!(policy::rule_covers(&block.cedar, &alice, &facts("slack__post", "slack"), "").unwrap());
        assert!(policy::rule_covers(&block.cedar, &alice, &facts("slack__delete", "slack"), "#general").unwrap());
        assert!(!policy::rule_covers(&block.cedar, &alice, &facts("gmail__send", "gmail"), "").unwrap(), "a block reached another connector");
        assert!(!policy::rule_covers(&block.cedar, &person("bob"), &facts("slack__post", "slack"), "").unwrap(), "a block reached another person");
        let ask = compile("alice", "gmail", "gmail__send", &["Eve <eve@rival.example>".to_string()], true, Effect::Ask).unwrap();
        assert!(!ask.once, "only an allow is spent");
        assert!(policy::rule_covers(&ask.cedar, &alice, &facts("gmail__send", "gmail"), "eve@rival.example").unwrap());
        assert!(!policy::rule_covers(&ask.cedar, &alice, &facts("gmail__send", "gmail"), "bob@corp.example").unwrap());
    }

    /// A rule written before effects existed reads as an allow, and its
    /// words are content the log may have lost.
    #[test]
    fn an_old_rule_reads_as_an_allow_with_no_words() {
        let old = json!({ "change": "proposed", "id": "r", "person": "alice",
            "compiled": { "connector": "d", "tool": "d__s", "destinations": ["b@c.example"], "once": false, "cedar": "permit(principal, action, resource);" } });
        let mut l = Ledger::default();
        l.apply(&serde_json::from_value(old).unwrap());
        let r = &l.of("alice")[0];
        assert_eq!((r.effect(), r.words.clone()), (Effect::Allow, None));
    }

    #[test]
    fn an_edit_changes_the_match_and_keeps_the_acceptance() {
        let mut l = Ledger::default();
        let allow = compile("alice", "d", "d__s", &["b@c.example".to_string()], false, Effect::Allow).unwrap();
        l.apply(&Change::Proposed { id: "r".into(), person: "alice".into(), compiled: Some(allow), words: Some("let it mail Bob".into()) });
        l.apply(&Change::Accepted { id: "r".into() });
        let block = compile("alice", "d", "", &[], false, Effect::Block).unwrap();
        l.apply(&Change::Edited { id: "r".into(), person: "alice".into(), compiled: Some(block), words: Some("never use d".into()) });
        let r = &l.of("alice")[0];
        assert!(r.accepted && r.live());
        assert_eq!((r.effect(), r.words.as_deref()), (Effect::Block, Some("never use d")));
    }

    #[test]
    fn an_unaccepted_or_spent_rule_is_not_live() {
        let compiled = compile("alice", "d", "d__s", &["b@c.example".to_string()], true, Effect::Allow).unwrap();
        let mut l = Ledger::default();
        l.apply(&Change::Proposed { id: "r".into(), person: "alice".into(), compiled: Some(compiled), words: None });
        assert!(!l.of("alice")[0].live(), "a proposal covered something");
        l.apply(&Change::Accepted { id: "r".into() });
        assert!(l.of("alice")[0].live());
        l.apply(&Change::Used { id: "r".into(), run: "x".into(), call_id: "y".into() });
        assert!(!l.of("alice")[0].live(), "a once rule ran twice");
        l.apply(&Change::Proposed { id: "w".into(), person: "alice".into(), compiled: None, words: Some("be careful".into()) });
        l.apply(&Change::Accepted { id: "w".into() });
        assert!(!l.of("alice")[1].live(), "words alone covered something");
    }

    #[test]
    fn a_short_value_is_not_treated_as_a_credential() {
        assert!(!discloses("a test of the word", "test"));
        assert!(discloses("key=sk-long-secret-value", "sk-long-secret-value"));
        assert!(secret_values(r#"{"access_token":"tok-abc","expires_at":1}"#).contains(&"tok-abc".to_string()));
    }
}
