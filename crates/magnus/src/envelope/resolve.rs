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
use crate::run_words::{RunWords, Source};
use crate::state::Run;
use anyhow::Result;
use magnus_adapter_proto::{Block, Message, Role, ToolSpec};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::Duration;

const EXTRACT_TIMEOUT_SECS: Duration = Duration::from_secs(15);
/// Whose words compile an envelope: the person's own, and a schedule they
/// set up, which "authorizes the work it describes" (spec rev 140). Never
/// the model's (a chat task's goal) nor anyone else's (2026-11-20: "only the
/// user's own messages, never tool results or assistant turns").
const TRUSTS: &[Source] = &[Source::Person, Source::Schedule];
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

/// Returns `true` if `candidate` literally appears in `prompt` (case-insensitive,
/// ignoring surrounding punctuation or URL scheme), preventing the extraction
/// model from hallucinating destinations the user never wrote (M5).
pub(crate) fn prompt_contains_literal(prompt: &str, candidate: &str) -> bool {
    let needle = candidate.trim().trim_matches(|c: char| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | '.')).trim_end_matches('/').to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    let hay = prompt.to_ascii_lowercase();
    if hay.contains(&needle) {
        return true;
    }
    // Also match when the model strips `http://` or `https://` from a prompt URL.
    if let Some(bare) = needle.strip_prefix("https://").or_else(|| needle.strip_prefix("http://")) {
        return !bare.is_empty() && hay.contains(bare);
    }
    false
}

/// Deterministic extraction of literal emails, `owner/repo`s, Slack channel IDs,
/// and URL hosts from the user's own prompt text.
pub fn extract_literal_destinations(prompt: &str) -> (Vec<String>, BTreeSet<Destination>) {
    let mut named = Vec::new();
    let mut dests = BTreeSet::new();
    for raw in prompt.split_whitespace() {
        let tok = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '.' | '`'));
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
        } else if let Some(d) = parse_literal_github_repo(tok) {
            named.push(tok.to_string());
            dests.insert(d);
        } else if is_literal_slack_channel_id(tok) {
            if let Some(d) = Destination::parse_slack_channel("default", tok) {
                named.push(tok.to_string());
                dests.insert(d);
            }
        }
    }
    (named, dests)
}

fn parse_literal_github_repo(tok: &str) -> Option<Destination> {
    if tok.contains("://") || tok.contains('@') || tok.starts_with('/') || tok.starts_with('.') {
        return None;
    }
    let (owner, repo) = tok.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    let valid_owner = owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let valid_repo = repo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid_owner || !valid_repo {
        return None;
    }
    Destination::parse_github_repo(tok)
}

fn is_literal_slack_channel_id(tok: &str) -> bool {
    tok.len() >= 9 && matches!(tok.as_bytes().first(), Some(b'C' | b'G' | b'D')) && tok.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn validate_model_candidate(prompt: &str, candidate: &str) -> Option<Destination> {
    if !prompt_contains_literal(prompt, candidate) {
        return None;
    }
    Destination::parse_email(candidate).or_else(|| Destination::parse_github_repo(candidate)).or_else(|| Destination::parse_slack_channel("default", candidate)).or_else(|| {
        if candidate.contains('.') && !candidate.contains(' ') {
            Destination::parse_host(candidate)
        } else {
            None
        }
    })
}

fn parsed_from_model(blocks: &[Block]) -> Option<(Vec<String>, Vec<String>)> {
    let input = blocks.iter().find_map(|b| if let Block::ToolCall { name, input, .. } = b { (name == "extract_targets").then_some(input) } else { None })?;
    let dests = input["destinations"].as_array()?.iter().filter_map(Value::as_str).map(String::from).collect();
    let apps = input["apps"].as_array()?.iter().filter_map(Value::as_str).map(String::from).collect();
    Some((dests, apps))
}

/// The prompt an envelope compiles from, and the destinations written in
/// it: only the words of the sources it trusts, never a chat task's goal.
fn literal_from(words: &RunWords) -> (String, Vec<String>, BTreeSet<Destination>) {
    let prompt = words.asked_text(TRUSTS);
    let (mut user_named, mut allowed) = extract_literal_destinations(&prompt);
    for addr in words.named_by_person() {
        if let Some(d) = Destination::parse_email(&addr) {
            user_named.push(addr);
            allowed.insert(d);
        }
    }
    (prompt, user_named, allowed)
}

impl Kernel {
    fn is_registered_app(&self, app: &str) -> bool {
        if crate::tools::BUILTINS.iter().any(|b| b.app.eq_ignore_ascii_case(app)) || self.cfg.connectors.iter().any(|c| c.name.eq_ignore_ascii_case(app)) {
            return true;
        }
        self.read(|s| s.apps.connectors().keys().any(|name| name.eq_ignore_ascii_case(app)))
    }

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

        let words = self.run_words(run).await.unwrap_or_default();
        let (prompt, mut user_named, mut allowed_destinations) = literal_from(&words);
        let mut allowed_write_apps = BTreeSet::new();

        // Model-assisted extraction on user-only prompt text (never tool results),
        // validated deterministically by the kernel against literal prompt tokens.
        if !prompt.trim().is_empty() {
            let messages = vec![Message { role: Role::User, content: vec![Block::Text { text: prompt.clone() }], state: None }];
            if let Ok(Some(blocks)) = self.reviewer_call_within(run, "envelope_extract", EXTRACT_SYSTEM, messages, vec![extract_tool()], Some(EXTRACT_TIMEOUT_SECS)).await {
                if let Some((candidates, apps)) = parsed_from_model(&blocks) {
                    for c in candidates {
                        if let Some(d) = validate_model_candidate(&prompt, &c) {
                            user_named.push(c);
                            allowed_destinations.insert(d);
                        }
                    }
                    for app in apps {
                        let a = app.trim().to_ascii_lowercase();
                        if !a.is_empty() && self.is_registered_app(&a) {
                            allowed_write_apps.insert(a);
                        }
                    }
                }
            }
        }

        // Include explicit standing grants and accepted allow rules held by this person.
        // Never a once rule: the envelope is kept for the whole task, and a once
        // rule covers exactly one action, so it is checked per action as the
        // person's answer (`person_cover`) and is spent by that action.
        let mut grant_ids = Vec::new();
        for r in self.rules_for(&run.person) {
            if r.live() && r.effect() == crate::rules::Effect::Allow {
                if let Some(compiled) = r.compiled.as_ref().filter(|c| !c.once) {
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
    fn literal_extractor_parses_emails_repos_channels_and_hosts_only_from_prompt() {
        let (names, dests) = extract_literal_destinations(
            "Send the report to Jane <jane@company.com>, review AtonoRobotics/magnus-teams, post to C0123456789, and check https://docs.example.org/spec.",
        );
        assert!(names.iter().any(|n| n.contains("jane@company.com")));
        assert!(dests.contains(&Destination::parse_email("jane@company.com").unwrap()));
        assert!(dests.contains(&Destination::parse_github_repo("AtonoRobotics/magnus-teams").unwrap()));
        assert!(dests.contains(&Destination::parse_slack_channel("default", "C0123456789").unwrap()));
        assert!(dests.contains(&Destination::parse_host("docs.example.org").unwrap()));
        assert!(!dests.contains(&Destination::parse_email("attacker@evil.com").unwrap()));
    }

    /// 2026-11-20: an envelope compiles "from only the user's own messages
    /// (never tool results or assistant turns)". A chat task's text is the
    /// goal the model wrote when it started the task: an address there is
    /// not one the person named. Before, the envelope read it as theirs.
    #[test]
    fn a_chat_tasks_goal_allows_no_destination_the_person_did_not_write() {
        let (prompt, named, allowed) = literal_from(&crate::run_words::tests::words("chat"));
        let eve = Destination::parse_email("eve@evil.example").unwrap();
        assert!(!allowed.contains(&eve), "the model's goal allowed {eve:?}: {prompt}");
        assert!(!named.iter().any(|n| n.contains("eve@") || n.contains("old@")), "{named:?}");
        assert!(!allowed.contains(&Destination::parse_email("old@corp.example").unwrap()), "an earlier request authorized a destination");
        assert!(allowed.contains(&Destination::parse_email("bob@corp.example").unwrap()), "the person's own words in the conversation were lost");
        // A sub-agent's goal, a teammate's lane and the server's text name nothing either.
        for shape in ["sub", "lane", "opener", "review"] {
            let (_, _, allowed) = literal_from(&crate::run_words::tests::words(shape));
            assert!(!allowed.iter().any(|d| d.display().contains("evil") || d.display().contains("server")), "{shape}: {allowed:?}");
        }
        // A schedule the person set up still authorizes what it names (spec rev 140).
        let (_, _, allowed) = literal_from(&crate::run_words::tests::words("schedule"));
        assert!(allowed.contains(&Destination::parse_email("cat@corp.example").unwrap()));
        // One a run set (`create_schedule`) is the model's words: its addresses allow nothing (2026-11-23).
        let (prompt, _, allowed) = literal_from(&crate::run_words::tests::words("run_schedule"));
        assert!(allowed.is_empty(), "a run's schedule allowed {allowed:?}: {prompt}");
    }

    #[test]
    fn model_candidates_not_literally_in_prompt_are_rejected() {
        let prompt = "Send an update to Jane about the release.";
        assert!(validate_model_candidate(prompt, "jane@company.com").is_none(), "guessed email must be rejected");
        assert!(validate_model_candidate(prompt, "AtonoRobotics/magnus-teams").is_none());
        assert!(validate_model_candidate(prompt, "C0123456789").is_none());

        let prompt2 = "Post to C0123456789 and open PR on AtonoRobotics/magnus-teams";
        assert_eq!(validate_model_candidate(prompt2, "C0123456789"), Some(Destination::parse_slack_channel("default", "C0123456789").unwrap()));
        assert_eq!(validate_model_candidate(prompt2, "AtonoRobotics/magnus-teams"), Some(Destination::parse_github_repo("AtonoRobotics/magnus-teams").unwrap()));
    }
}
