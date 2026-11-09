//! Rules a person accepted, compiled to Cedar. A rule covers an action only
//! by a deterministic match on its connector, tool and every destination:
//! neither the reviewer nor the working model decides that. A rule the model
//! proposed covers nothing until its person accepts it, and words with no
//! compiled match cover nothing at all. The hard denials are here too,
//! because they are what no rule or grant can reach.
use crate::api::{err, internal, who};
use crate::boundary::{Action, Facts, Issuer, Shown};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::policy::{self, ToolFacts};
use crate::state::Run;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A vault value shorter than this could be an ordinary word.
const MIN_SECRET_BYTES: usize = 8;
/// Input fields that name whose account an action works on.
const ACCOUNT_FIELDS: [&str; 7] = ["account", "account_id", "as_user", "on_behalf_of", "impersonate", "user_id", "mailbox"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Compiled {
    pub connector: String,
    pub tool: String,
    pub destinations: Vec<String>,
    pub once: bool,
    pub cedar: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Proposed { id: String, person: String, compiled: Option<Compiled> },
    Accepted { id: String },
    Used { id: String, run: String, call_id: String },
    Removed { id: String },
}

#[derive(Serialize, Clone, Debug)]
pub struct Rule {
    pub id: String,
    pub person: String,
    pub compiled: Option<Compiled>,
    pub accepted: bool,
    pub used: u32,
}

impl Rule {
    fn live(&self) -> bool {
        self.accepted && self.compiled.as_ref().is_some_and(|c| !(c.once && self.used > 0))
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Ledger(BTreeMap<String, Rule>);

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Proposed { id, person, compiled } => {
                self.0.insert(id.clone(), Rule { id: id.clone(), person: person.clone(), compiled: compiled.clone(), accepted: false, used: 0 });
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

pub fn compile(person: &str, connector: &str, tool: &str, destinations: &[String], once: bool) -> Result<Compiled, String> {
    if connector.is_empty() || tool.is_empty() || destinations.is_empty() {
        return Err("a rule names a connector, a tool and at least one destination".to_string());
    }
    let destinations: Vec<String> = destinations.iter().map(|d| normalized(d)).collect();
    let list = destinations.iter().map(|d| quoted(d)).collect::<Result<Vec<_>, _>>()?.join(", ");
    let cedar = format!(
        "permit(principal == Person::{}, action == Action::\"execute\", resource == Tool::{}) when {{ resource in App::{} && [{list}].contains(context.destination) }};",
        quoted(person)?,
        quoted(tool)?,
        quoted(connector)?
    );
    policy::validate(&cedar).map_err(|e| e.to_string())?;
    Ok(Compiled { connector: connector.to_string(), tool: tool.to_string(), destinations, once, cedar })
}

/// Where an action goes, as a rule would name it. Empty means it cannot match.
fn destinations_of(a: &Action, shown: Option<&Shown>) -> Vec<String> {
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
        rules.into_iter().filter(Rule::live).find_map(|r| {
            let cedar = r.compiled.as_ref()?.cedar.clone();
            destinations.iter().all(|d| policy::rule_covers(&cedar, &person, &facts, d).unwrap_or(false)).then_some(r.id)
        })
    }

    /// Denied whatever a policy, grant, rule or approval says, and never
    /// asked: disclosing a credential, or reaching another person's account
    /// or computer. Only what the model asked for is judged here.
    pub(crate) fn hard_denial(&self, a: &Action, run: &Run) -> Option<String> {
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

#[derive(Deserialize)]
struct NewRule {
    connector: Option<String>,
    tool: Option<String>,
    #[serde(default)]
    destinations: Vec<String>,
    #[serde(default)]
    once: bool,
    /// Leaves it waiting for its person's acceptance.
    #[serde(default)]
    proposed: bool,
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/rules", get(list).post(add)).route("/v1/rules/{id}", axum::routing::delete(remove)).route("/v1/rules/{id}/accept", post(accept))
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
    let compiled = match (&b.connector, &b.tool) {
        (Some(c), Some(t)) => match compile(&p.id, c, t, &b.destinations, b.once) {
            Ok(c) => Some(c),
            Err(m) => return err(StatusCode::BAD_REQUEST, m),
        },
        _ => None,
    };
    let id = crate::kernel::new_id("rule");
    let actor = format!("person:{}", p.id);
    let mut evs = vec![(actor.as_str(), Ev::Rule(Change::Proposed { id: id.clone(), person: p.id.clone(), compiled }))];
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
        let c = compile("alice", "drafts", "drafts__send", &["Bob <bob@corp.example>".to_string()], true).unwrap();
        assert_eq!(c.destinations, vec!["bob@corp.example".to_string()]);
        let alice = person("alice");
        assert!(policy::rule_covers(&c.cedar, &alice, &facts("drafts__send", "drafts"), "bob@corp.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &alice, &facts("drafts__send", "drafts"), "eve@evil.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &alice, &facts("drafts__wipe", "drafts"), "bob@corp.example").unwrap());
        assert!(!policy::rule_covers(&c.cedar, &person("bob"), &facts("drafts__send", "drafts"), "bob@corp.example").unwrap());
    }

    #[test]
    fn a_quote_in_a_destination_cannot_widen_the_rule() {
        let c = compile("alice", "drafts", "drafts__send", &["x\" || true || \"".to_string()], false).unwrap();
        assert!(!policy::rule_covers(&c.cedar, &person("alice"), &facts("drafts__send", "drafts"), "eve@evil.example").unwrap());
    }

    #[test]
    fn an_unaccepted_or_spent_rule_is_not_live() {
        let compiled = compile("alice", "d", "d__s", &["b@c.example".to_string()], true).unwrap();
        let mut l = Ledger::default();
        l.apply(&Change::Proposed { id: "r".into(), person: "alice".into(), compiled: Some(compiled) });
        assert!(!l.of("alice")[0].live(), "a proposal covered something");
        l.apply(&Change::Accepted { id: "r".into() });
        assert!(l.of("alice")[0].live());
        l.apply(&Change::Used { id: "r".into(), run: "x".into(), call_id: "y".into() });
        assert!(!l.of("alice")[0].live(), "a once rule ran twice");
        l.apply(&Change::Proposed { id: "w".into(), person: "alice".into(), compiled: None });
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
