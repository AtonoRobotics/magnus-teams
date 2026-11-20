//! Lazy, user-only envelope compilation.
//!
//! Runs on the first outbound write attempt of a turn, task, or schedule,
//! reading ONLY the user's own trusted prompt text (zero tool outputs, zero
//! assistant messages, zero untrusted content). Candidate names/addresses are
//! then deterministically validated by the kernel into typed `Destination`s;
//! anything that does not match is dropped.
use super::{Change, Destination, Envelope};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Run;
use anyhow::Result;
use magnus_adapter_proto::{Block, Message, Role, ToolSpec};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::Duration;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(15);
const EXTRACT_SYSTEM: &str = "You extract explicit write destinations (email addresses, person names, Slack channel IDs, GitHub owner/repo pairs, or hostnames) and apps mentioned in the user's request. Only include destinations the user explicitly asked to send to or write to. Call the extract_targets tool.";

fn extract_tool() -> ToolSpec {
    ToolSpec {
        name: "extract_targets".into(),
        description: "Return explicit destinations and apps mentioned in the user's request.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["destinations", "apps"],
            "properties": {
                "destinations": { "type": "array", "items": { "type": "string" } },
                "apps": { "type": "array", "items": { "type": "string" } }
            }
        }),
    }
}

/// Deterministic extraction of literal emails, `owner/repo`s, and URL hosts
/// from the user's own prompt text.
pub fn extract_literal_destinations(prompt: &str) -> (Vec<String>, BTreeSet<Destination>) {
    let mut named = Vec::new();
    let mut dests = BTreeSet::new();
    for raw in prompt.split_whitespace() {
        let tok = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '.'));
        if tok.is_empty() {
            continue;
        }
        if let Some(d) = Destination::parse_email(tok) {
            named.push(tok.to_string());
            dests.insert(d);
        } else if (tok.starts_with("http://") || tok.starts_with("https://")) && tok.len() > 10 {
            if let Some(d) = Destination::parse_host(tok) {
                named.push(tok.to_string());
                dests.insert(d);
            }
        }
    }
    (named, dests)
}

fn parsed_from_model(blocks: &[Block]) -> Option<(Vec<String>, Vec<String>)> {
    let input = blocks.iter().find_map(|b| if let Block::ToolCall { name, input, .. } = b { (name == "extract_targets").then_some(input) } else { None })?;
    let dests = input["destinations"].as_array()?.iter().filter_map(Value::as_str).map(String::from).collect();
    let apps = input["apps"].as_array()?.iter().filter_map(Value::as_str).map(String::from).collect();
    Some((dests, apps))
}

impl Kernel {
    /// Lazily compile (or return cached) write envelope for `run`.
    ///
    /// Sees ONLY the requester's original prompt text and active standing
    /// grants — never untrusted tool results or assistant messages.
    pub(crate) async fn ensure_envelope(&self, run: &Run) -> Result<Envelope> {
        let requester = self.requester_run(run);
        let scope_id = if requester.task.is_empty() { run.id.clone() } else { requester.task.clone() };
        if let Some(existing) = self.read(|s| s.envelopes.for_scope(&scope_id).cloned()) {
            return Ok(existing);
        }

        let prompt = if requester.task.is_empty() { String::new() } else { self.asked(&requester).await.unwrap_or_default() };
        let (mut user_named, mut allowed_destinations) = extract_literal_destinations(&prompt);
        if let Ok(person_addrs) = self.named_by_person(run).await {
            for addr in person_addrs {
                if let Some(d) = Destination::parse_email(&addr) {
                    user_named.push(addr);
                    allowed_destinations.insert(d);
                }
            }
        }
        let mut allowed_write_apps = BTreeSet::new();

        // Model-assisted extraction on user-only prompt text (never tool results),
        // validated deterministically by the kernel.
        if !prompt.trim().is_empty() {
            let messages = vec![Message { role: Role::User, content: vec![Block::Text { text: prompt.clone() }], state: None }];
            if let Ok(Ok(Some(blocks))) = tokio::time::timeout(EXTRACT_TIMEOUT, self.reviewer_call(run, "envelope_extract", EXTRACT_SYSTEM, messages, vec![extract_tool()])).await {
                if let Some((candidates, apps)) = parsed_from_model(&blocks) {
                    for c in candidates {
                        // Kernel validation: accept only concrete, normalized destinations
                        // (or names resolved against explicit user-named tokens).
                        if let Some(d) = Destination::parse_email(&c).or_else(|| Destination::parse_github_repo(&c)) {
                            user_named.push(c);
                            allowed_destinations.insert(d);
                        }
                    }
                    for app in apps {
                        let a = app.trim().to_ascii_lowercase();
                        if !a.is_empty() {
                            allowed_write_apps.insert(a);
                        }
                    }
                }
            }
        }

        // Include explicit standing grants and accepted allow rules held by this person.
        let mut grant_ids = Vec::new();
        for r in self.rules_for(&run.person) {
            if r.live() && r.effect() == crate::rules::Effect::Allow {
                if let Some(compiled) = &r.compiled {
                    grant_ids.push(r.id.clone());
                    if !compiled.connector.is_empty() {
                        allowed_write_apps.insert(compiled.connector.clone());
                    }
                    for raw_dest in &compiled.destinations {
                        if let Some(d) = Destination::parse_for_app(&compiled.connector, raw_dest) {
                            allowed_destinations.insert(d);
                        }
                    }
                }
            }
        }

        user_named.sort();
        user_named.dedup();
        grant_ids.sort();
        grant_ids.dedup();

        let change = Change::Compiled {
            run: run.id.clone(),
            scope_id: scope_id.clone(),
            user_named: user_named.clone(),
            grant_ids: grant_ids.clone(),
            allowed_destinations: allowed_destinations.iter().cloned().collect(),
            allowed_write_apps: allowed_write_apps.iter().cloned().collect(),
        };
        self.emit("kernel", Ev::Envelope(change)).await?;

        Ok(Envelope { scope_id, allowed_destinations, allowed_write_apps, user_named, grant_ids })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_extractor_parses_emails_and_hosts_only_from_prompt() {
        let (names, dests) = extract_literal_destinations("Send the report to Jane <jane@company.com> and check https://docs.example.org/spec.");
        assert!(names.iter().any(|n| n.contains("jane@company.com")));
        assert!(dests.contains(&Destination::parse_email("jane@company.com").unwrap()));
        assert!(dests.contains(&Destination::parse_host("docs.example.org").unwrap()));
        assert!(!dests.contains(&Destination::parse_email("attacker@evil.com").unwrap()));
    }
}
