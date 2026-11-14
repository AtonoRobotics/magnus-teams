//! The state projection's canonical form, pinned (decisions/2026-11-14-state-folds-per-domain.md).
//!
//! The state is rebuilt by replay and never stored, so moving a field into
//! a domain's own projection risks only drift in memory: the same events
//! folding to something else. This replays one broad synthetic sequence
//! (made up here, never production data) and compares every field of the
//! domains being moved, rendered canonically, with the pinned rendering.
//! A field keeps its name here wherever it now lives, so every step of the
//! move must leave the rendering as it is.

use crate::log::Event;
use crate::state::State;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

/// The pinned rendering of the sequence below.
const PINNED: &str = include_str!("state_canonical_pinned.txt");

fn at(seq: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 14, 9, 0, 0).single().unwrap_or_default() + chrono::Duration::minutes(seq)
}

/// Every kind of event the moved domains fold, in an order that exercises
/// each arm's branches, with the cross-domain ones among them: erasure,
/// the trash, and backup scrubs that write the audit.
fn sequence() -> Vec<(&'static str, Value)> {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    let label = json!({ "untrusted": false, "private": true });
    let crm = json!({ "name": "crm", "url": "https://crm.example/mcp", "audience": "crm", "label": "CRM", "credential": "key", "tools": { "read": { "risk": "read", "reach": "self" } } });
    let mail = json!({ "name": "mail", "url": "https://mail.example/mcp", "audience": "mail", "label": "Mail", "credential": "tokens", "auth": "oauth" });
    let w = |p: &str, ids: Vec<&str>, full: bool| json!({ "kind": "watched_seen", "person": p, "app": "mail", "tool": "list_inbox", "ids": ids, "full": full });
    vec![
        ("kernel", json!({ "kind": "kernel_boot", "version": "1.0", "pid": 1, "instructions": "i1" })),
        ("kernel", json!({ "kind": "kernel_boot", "version": "1.0", "pid": 2, "instructions": "i1" })),
        ("kernel", json!({ "kind": "kernel_boot", "version": "1.1", "pid": 3, "instructions": "i1" })),
        ("person:bob", json!({ "kind": "secret_set", "name": "connector_mail__bob" })),
        ("person:alice", json!({ "kind": "secret_set", "name": "connector_mail__alice" })),
        ("admin:root", json!({ "kind": "secret_set", "name": "api_key" })),
        ("admin:root", json!({ "kind": "policy_set", "layer": "org", "cedar": "permit(principal, action, resource);" })),
        ("admin:ann", json!({ "kind": "policy_set", "layer": "org", "cedar": "forbid(principal, action, resource);" })),
        ("admin:root", json!({ "kind": "policy_set", "layer": "team", "cedar": "permit(principal, action, resource);" })),
        ("admin:root", json!({ "kind": "module_set", "name": "lost_reset", "on": true })),
        ("admin:root", json!({ "kind": "module_set", "name": "escalate", "on": false })),
        ("admin:root", json!({ "kind": "module_set", "name": "lost_reset", "on": null })),
        ("admin:root", json!({ "kind": "provider_set", "id": "openai", "enabled": false })),
        ("admin:root", json!({ "kind": "provider_set", "id": "anthropic", "enabled": false })),
        ("admin:root", json!({ "kind": "provider_set", "id": "anthropic", "enabled": true })),
        ("admin:root", json!({ "kind": "desktop_channel_set", "channel": "beta" })),
        ("admin:root", json!({ "kind": "console_settings", "idle_minutes": 30, "session_hours": 8, "allow": ["10.0.0.0/8"] })),
        ("admin:root", json!({ "kind": "policy_drafted", "layer": "org", "cedar": "permit(principal, action, resource) when { true };" })),
        ("admin:root", json!({ "kind": "policy_drafted", "layer": "team", "cedar": "forbid(principal, action, resource);" })),
        ("admin:root", json!({ "kind": "policy_drafted", "layer": "team", "cedar": null })),
        ("admin:root", json!({ "kind": "model_set", "adapter": "anthropic", "model": "m1" })),
        ("kernel", json!({ "kind": "adapter_verified", "adapter_id": "a1", "adapter": "anthropic", "version": "1", "model": "m1", "passed": true, "report": { "cases": 3 } })),
        ("kernel", json!({ "kind": "adapter_verified", "adapter_id": "a2", "adapter": "openai", "version": "2", "model": "m2", "passed": false, "report": {} })),
        ("admin:root", json!({ "kind": "admin_action", "admin": "sub-root", "name": "Root", "action": "policy_set", "target": "org", "before": null, "after": { "v": 2 } })),
        ("admin:root", json!({ "kind": "kill_switch", "engaged": true })),
        ("kernel", json!({ "kind": "vault_sealed", "revoking": ["connector_mail__bob", "connector_mail__alice"], "kept": ["api_key"] })),
        ("kernel", json!({ "kind": "secret_revoked", "name": "connector_mail__bob", "adapter": "mail", "at_provider": true, "detail": "revoked" })),
        ("admin:ann", json!({ "kind": "kill_switch", "engaged": false })),
        ("person:alice", json!({ "kind": "secret_set", "name": "connector_mail__alice" })),
        ("kernel", json!({ "kind": "update_started", "from": "1.0", "to": "1.1", "rollback": false })),
        ("kernel", json!({ "kind": "update_finished", "from": "1.0", "to": "1.1" })),
        ("kernel", json!({ "kind": "update_started", "from": "1.1", "to": "1.2", "rollback": false })),
        ("kernel", json!({ "kind": "update_rolled_back", "from": "1.2", "to": "1.1", "reason": "it did not start" })),
        ("kernel", json!({ "kind": "update_failed", "to": "1.3", "reason": "bad signature" })),
        ("kernel", json!({ "kind": "update_started", "from": "1.1", "to": "1.0", "rollback": true })),
        ("kernel", json!({ "kind": "backup_made", "file": "b1", "through_seq": 30, "events": 30, "bytes": 100 })),
        ("kernel", json!({ "kind": "backup_checked", "file": "b1", "ok": true, "detail": "restores" })),
        ("kernel", json!({ "kind": "backup_copied", "file": "b1", "ok": false, "detail": "no route" })),
        ("admin:root", json!({ "kind": "connector_added", "connector": crm })),
        ("admin:root", json!({ "kind": "connector_added", "connector": mail })),
        ("admin:root", json!({ "kind": "connector_settings", "name": "crm", "enabled": false, "access_groups": ["sales"] })),
        ("admin:root", json!({ "kind": "connector_settings", "name": "mail", "enabled": true, "blocked_groups": ["interns"] })),
        ("person:bob", json!({ "kind": "app_connected", "app": "mail", "person": "bob", "account": "bob@mail.example" })),
        ("person:bob", json!({ "kind": "app_connected", "app": "mail", "person": "bob" })),
        ("person:alice", json!({ "kind": "app_connected", "app": "mail", "person": "alice" })),
        ("person:bob", json!({ "kind": "app_disconnected", "app": "mail", "person": "bob", "at_provider": true, "detail": "" })),
        ("admin:root", json!({ "kind": "credential_set", "connector": "crm", "set": false })),
        ("admin:root", json!({ "kind": "registration_set", "provider": "google", "set": true })),
        ("admin:root", json!({ "kind": "registration_set", "provider": "microsoft", "set": true })),
        ("admin:root", json!({ "kind": "registration_set", "provider": "microsoft", "set": false })),
        ("admin:root", json!({ "kind": "tool_classified", "connector": "crm", "tool": "read", "risk": "write" })),
        ("admin:root", json!({ "kind": "tool_classified", "connector": "crm", "tool": "list", "risk": "read" })),
        ("admin:root", json!({ "kind": "tool_classified", "connector": "crm", "tool": "list", "risk": null })),
        ("admin:root", json!({ "kind": "tool_classified", "connector": "absent", "tool": "x", "risk": "read" })),
        ("person:bob", json!({ "kind": "tools", "change": "consented", "app": "mail", "person": "bob", "terms": "reads mail" })),
        ("person:alice", json!({ "kind": "tools", "change": "consented", "app": "crm", "person": "alice", "terms": "reads deals" })),
        ("admin:root", json!({ "kind": "connector_added", "connector": { "name": "wiki", "url": "https://wiki.example/mcp", "audience": "wiki", "label": "Wiki" } })),
        ("admin:root", json!({ "kind": "connector_removed", "name": "crm" })),
        ("person:bob", w("bob", vec!["a"], true)),
        ("person:bob", w("bob", vec!["b"], false)),
        ("person:bob", w("bob", vec!["a", "c"], false)),
        ("person:alice", w("alice", vec!["x"], true)),
        ("person:alice", w("alice", vec!["y"], false)),
        ("kernel", json!({ "kind": "watch_polled", "person": "bob", "app": "mail", "tool": "list_inbox" })),
        ("kernel", json!({ "kind": "watch_polled", "person": "alice", "app": "mail", "tool": "list_inbox" })),
        ("person:bob", json!({ "kind": "task_created", "task": "t-bob", "run": "r-bob", "person": "bob", "text": "Read this.", "config": config })),
        ("person:alice", json!({ "kind": "task_created", "task": "t-alice", "run": "r-alice", "person": "alice", "text": "Read this.", "config": config })),
        ("person:bob", json!({ "kind": "chat_hidden", "person": "bob", "thread": "thread_1", "entries": ["e1", "e2"] })),
        ("person:alice", json!({ "kind": "chat_hidden", "person": "alice", "thread": "thread_2", "entries": ["e3"] })),
        ("person:bob", json!({ "kind": "chat_hidden", "person": "bob", "thread": "thread_1", "entries": ["e2", "e9"] })),
        ("person:bob", json!({ "kind": "reacted", "person": "bob", "thread": "thread_1", "entry": "e4", "reaction": "great" })),
        ("person:bob", json!({ "kind": "reacted", "person": "bob", "thread": "thread_1", "entry": "e4" })),
        ("person:alice", json!({ "kind": "reacted", "person": "alice", "thread": "thread_2", "entry": "e5", "reaction": "annoying" })),
        ("kernel", json!({ "kind": "disagreement_recorded", "id": "d1", "person": "bob", "run": "r-bob", "position": "wait", "decision": "send now" })),
        ("kernel", json!({ "kind": "disagreement_recorded", "id": "d2", "person": "alice", "run": "r-alice", "position": "call", "decision": "email" })),
        ("kernel", json!({ "kind": "disagreement_outcome", "id": "d1", "person": "bob", "run": "r-bob", "outcome": "it bounced" })),
        ("kernel", json!({ "kind": "disagreement_outcome", "id": "d1", "person": "bob", "run": "r-bob", "outcome": "later" })),
        ("kernel", json!({ "kind": "disagreement_outcome", "id": "d9", "person": "bob", "run": "r-bob", "outcome": "none" })),
        (
            "person:bob",
            json!({ "kind": "thread_branched", "person": "bob", "thread": "thread_b", "from": "thread_1", "upto": "e1", "entries": [{ "id": "e1", "text": "hi" }], "title": "B" }),
        ),
        ("person:alice", json!({ "kind": "thread_branched", "person": "alice", "thread": "thread_c", "from": "thread_2", "upto": "e3", "entries": [], "title": "C" })),
        ("person:alice", json!({ "kind": "thread_branched", "person": "alice", "thread": "thread_d", "from": "thread_2", "upto": "e3", "title": "D" })),
        ("person:alice", json!({ "kind": "trashed", "item": "thread_c", "person": "alice" })),
        ("person:alice", json!({ "kind": "restored", "item": "thread_c" })),
        ("person:alice", json!({ "kind": "trashed", "item": "thread_d", "person": "alice" })),
        ("person:bob", json!({ "kind": "trashed", "item": "thread_b", "person": "bob" })),
        ("kernel", json!({ "kind": "playbook", "item": "p1", "op": "add", "person": "alice", "run": "r-alice", "text": "Be brief.", "label": label })),
        ("kernel", json!({ "kind": "playbook", "item": "p1", "op": "edit", "person": "alice", "run": "r-alice", "text": "Be briefer.", "label": label })),
        ("admin:root", json!({ "kind": "subject_bound", "person": "bob", "subject": "sub-bob" })),
        ("admin:root", json!({ "kind": "subject_bound", "person": "carl", "subject": "sub-carl" })),
        ("admin:root", json!({ "kind": "person_erased", "person": "bob" })),
        ("admin:root", json!({ "kind": "removed", "item": "person:bob", "seqs": [] })),
        ("admin:root", json!({ "kind": "removed", "item": "person:carl", "seqs": [] })),
        ("kernel", json!({ "kind": "backup_scrub_pending", "from": 80, "through": 120, "files": ["b1"], "detail": "disk full" })),
        ("kernel", json!({ "kind": "backups_scrubbed", "through": 100 })),
        ("kernel", json!({ "kind": "backup_scrub_pending", "from": 80, "through": 130, "files": ["b1", "b2"], "detail": "disk still full" })),
        ("kernel", json!({ "kind": "backups_scrubbed", "through": 130 })),
        ("admin:root", json!({ "kind": "admin_action", "admin": "sub-root", "name": "Root", "action": "kill_switch", "target": null })),
        ("admin:root", json!({ "kind": "model_set", "adapter": "openai", "model": "m2" })),
        ("kernel", json!({ "kind": "kernel_boot", "version": "1.1", "pid": 4, "instructions": "i2" })),
        ("person:alice", json!({ "kind": "chat_hidden", "person": "alice", "thread": "thread_d", "entries": ["e7"] })),
        ("person:alice", json!({ "kind": "tools", "change": "consented", "app": "wiki", "person": "alice", "terms": "reads pages" })),
        ("kernel", json!({ "kind": "disagreement_outcome", "id": "d2", "person": "alice", "run": "r-alice", "outcome": "it worked" })),
        ("kernel", json!({ "kind": "disagreement_outcome", "id": "d2", "person": "alice", "run": "r-alice", "outcome": "again" })),
        ("admin:root", json!({ "kind": "kill_switch", "engaged": true })),
        ("kernel", json!({ "kind": "vault_sealed", "revoking": ["connector_mail__alice", "connector_wiki__alice"], "kept": ["api_key", "push_key"] })),
        ("kernel", json!({ "kind": "secret_revoked", "name": "connector_mail__alice", "adapter": "mail", "at_provider": false, "detail": "no answer" })),
        ("person:alice", json!({ "kind": "reacted", "person": "alice", "thread": "thread_2", "entry": "e5", "reaction": "great" })),
        ("person:alice", json!({ "kind": "watch_polled", "person": "alice", "app": "wiki", "tool": "list_pages" })),
    ]
}

/// The state after the sequence.
fn replayed() -> State {
    let mut s = State::default();
    for (i, (actor, ev)) in sequence().into_iter().enumerate() {
        let seq = i as i64 + 1;
        let ev = serde_json::from_value(ev.clone()).unwrap_or_else(|err| panic!("event {seq} does not load: {err}: {ev}"));
        s.apply(&Event { seq, at: at(seq), actor: actor.into(), ev });
    }
    s
}

/// Each moved field by its name, as `{:?}` renders it.
fn fields(s: &State) -> Vec<(&'static str, String)> {
    let d = |v: &dyn std::fmt::Debug| format!("{v:?}");
    let mut v = s.watch.canonical_fields();
    v.extend([
        ("updating", d(&s.updating)),
        ("last_update", d(&s.last_update)),
        ("skipped_releases", d(&s.skipped_releases)),
        ("last_backup", d(&s.last_backup)),
        ("last_copy", d(&s.last_copy)),
        ("scrub_pending", d(&s.scrub_pending)),
        ("erasures", d(&s.erasures)),
        ("secrets", d(&s.secrets)),
        ("revoked", d(&s.revoked)),
        ("sealed", d(&s.sealed)),
        ("kill_switch", d(&s.kill_switch)),
        ("kill_switch_by", d(&s.kill_switch_by)),
        ("policies", d(&s.policies)),
        ("policy_history", d(&s.policy_history)),
        ("policy_drafts", d(&s.policy_drafts)),
        ("module_settings", d(&s.module_settings)),
        ("providers_off", d(&s.providers_off)),
        ("desktop_channel", d(&s.desktop_channel)),
        ("console_settings", d(&s.console_settings)),
        ("model", d(&s.model)),
        ("conformance", d(&s.conformance)),
        ("audit", d(&s.audit)),
        ("version_changes", d(&s.version_changes)),
        ("playbook_version", d(&s.playbook_version)),
        ("hidden", d(&s.hidden)),
        ("reactions", d(&s.reactions)),
        ("disagreements", d(&s.disagreements)),
        ("branches", d(&s.branches)),
        ("trash", d(&s.trash.keys().collect::<Vec<_>>())),
        ("connectors", d(&s.connectors)),
        ("connector_settings", d(&s.connector_settings)),
        ("app_connections", d(&s.app_connections)),
        ("credential_changed", d(&s.credential_changed)),
        ("registrations", d(&s.registrations)),
        ("tools.consents", d(&s.tools.consents)),
    ]);
    v
}

/// The state's moved fields, one a line, sorted by name, each with its
/// unordered collections (hash maps and sets) in sorted order.
pub(crate) fn render(s: &State) -> String {
    let mut lines: Vec<String> = fields(s).into_iter().map(|(name, v)| format!("{name} = {}", canonical(&v))).collect();
    lines.sort();
    lines.join("\n") + "\n"
}

/// A `{:?}` rendering with each map's and set's entries sorted, so a hash
/// collection's order does not count. A brace after a name opens a struct,
/// whose fields keep their order; any other opens a collection.
pub(crate) fn canonical(debug: &str) -> String {
    let c: Vec<char> = debug.chars().collect();
    let mut i = 0;
    let mut out = String::new();
    copy(&c, &mut i, &mut out, &[]);
    out
}

/// Copies from `i` up to one of `stops` at this depth, which is left unread.
fn copy(c: &[char], i: &mut usize, out: &mut String, stops: &[char]) {
    while let Some(&ch) = c.get(*i) {
        if stops.contains(&ch) {
            return;
        }
        *i += 1;
        match ch {
            '"' => {
                out.push('"');
                while let Some(&x) = c.get(*i) {
                    *i += 1;
                    out.push(x);
                    if x == '\\' {
                        if let Some(&y) = c.get(*i) {
                            *i += 1;
                            out.push(y);
                        }
                    } else if x == '"' {
                        break;
                    }
                }
            }
            '(' | '[' => {
                let close = if ch == '(' { ')' } else { ']' };
                out.push(ch);
                copy(c, i, out, &[close]);
                *i += 1;
                out.push(close);
            }
            '{' if out.trim_end().chars().last().is_some_and(|p| p.is_alphanumeric() || p == '_') => {
                out.push('{');
                copy(c, i, out, &['}']);
                *i += 1;
                out.push('}');
            }
            '{' => {
                let mut entries = Vec::new();
                loop {
                    let mut entry = String::new();
                    copy(c, i, &mut entry, &[',', '}']);
                    let entry = entry.trim().to_string();
                    if !entry.is_empty() {
                        entries.push(entry);
                    }
                    let end = c.get(*i).is_none_or(|x| *x == '}');
                    *i += 1;
                    if end {
                        break;
                    }
                }
                entries.sort();
                out.push('{');
                out.push_str(&entries.join(", "));
                out.push('}');
            }
            _ => out.push(ch),
        }
    }
}

#[test]
fn the_canonical_form_sorts_collections_and_keeps_structs_and_lists() {
    let set = r#"{"b", "a"}"#;
    assert_eq!(canonical(set), r#"{"a", "b"}"#);
    assert_eq!(canonical(r#"Foo { z: 1, a: {"y": [2, 1], "x": Some({3, 1})} }"#), r#"Foo { z: 1, a: {"x": Some({1, 3}), "y": [2, 1]} }"#);
    assert_eq!(canonical(r#"{"a{,}": 1, "\"}": 2}"#), r#"{"\"}": 2, "a{,}": 1}"#);
    assert_eq!(canonical("{}"), "{}");
}

/// Every step of moving the state's fields into domains folds the same
/// events to the same state.
#[test]
fn the_state_folds_the_sequence_to_the_pinned_form() {
    let got = render(&replayed());
    assert!(got == PINNED, "the state folds the sequence differently now; its rendering:\n----\n{got}----");
}
