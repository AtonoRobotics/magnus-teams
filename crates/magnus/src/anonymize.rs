//! What a developer view shows of each person's work (owner, 2026-11-11:
//! "Make developer data anonymous"; `decisions/2026-11-11-anonymous-developer-data.md`).
//!
//! Every `/v1/dev` route asks this module, through a [`Lens`], how much of
//! a person it may show:
//!
//! - [`Sight::Full`]: the developer's own work, and a tester's (the Tester
//!   Terms, `crate::legal`). Shown as it is.
//! - [`Sight::Anonymous`]: someone who said yes to sharing anonymous usage
//!   data. Shown through [`shape`]: no words, inputs, results, names, file
//!   names, URL paths or memory; ids replaced by [`Lens::pseudonym`], a keyed
//!   hash whose key stays in the vault, so a developer cannot reverse it.
//!   What finds bugs stays: event kinds, times, counts, costs, tokens, model
//!   and adapter ids, tool names, error kinds, statuses and hostnames.
//! - [`Sight::Hidden`]: everyone else, those who said no and those not yet
//!   asked. Nothing of theirs is shown, not even anonymised.
//!
//! [`shape`] keeps by allowlist: a string survives only under a key named
//! in [`CODE_KEYS`] and only when it is a bare token ([`code`]), so a field
//! added later is withheld until someone decides it is safe.

use crate::config::Person;
use crate::kernel::Kernel;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The vault entry holding the key pseudonyms are made with.
pub const KEY_NAME: &str = "developer-view/pseudonym-key";
/// Bytes of the pseudonym key.
const KEY_BYTES: usize = 32;
/// Hex characters of a pseudonym: 48 bits, enough to tell people apart.
const PSEUDONYM_HEX: usize = 12;
/// The longest string kept as a token.
const MAX_CODE_CHARS: usize = 80;
/// The longest system-made label kept (a context source, a config key).
const MAX_LABEL_CHARS: usize = 160;

/// Keys whose string values are kept when they are bare tokens.
pub const CODE_KEYS: &[&str] = &[
    "kind",
    "what",
    "status",
    "stage",
    "outcome",
    "rule",
    "tool",
    "adapter",
    "model",
    "harness",
    "instructions",
    "served_model",
    "stop",
    "issuer",
    "phase",
    "verdict",
    "method",
    "failure",
    "class",
    "grader",
    "reason",
    "at",
    "created_at",
    "ended_at",
    "added_at",
    "last_at",
    "started",
    "run",
    "runs",
    "id",
    "case",
    "seq",
    "call_id",
    "step_id",
    "host",
    "destinations",
    "apps",
    "risk",
    "type",
    "recorded_by",
    "escalated_to",
    "experiment",
    "tools_hash",
    "source_run",
    "choice",
    "verdict_from",
    "where",
    "media_type",
    "op",
    "scope",
    "role",
    "decided",
    "hash",
];
/// Keys whose string values name a person, a workspace or one of their
/// records: shown as pseudonyms.
pub const PSEUDONYM_KEYS: &[&str] =
    &["person", "by", "owner", "task", "thread", "item", "workspace", "approved_by", "actor", "controller", "edited_by", "waiting_for", "owners", "layer"];
/// Keys whose labels are made by the developer view itself from codes.
const LABEL_KEYS: &[&str] = &["source", "key", "label"];
/// Keys whose whole value is someone's content, never shown anonymised.
const CONTENT_KEYS: &[&str] = &["input", "data", "details", "contract", "body", "plan", "blocks", "reasoning", "attachments", "cedar", "notes", "context_text"];

/// How much of one person a developer view may show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sight {
    Hidden,
    Anonymous,
    Full,
}

/// One developer's view of everyone: who they see in full, who
/// anonymised, and the key that makes pseudonyms.
pub struct Lens {
    viewer: String,
    hidden_runs: BTreeSet<String>,
    key: Option<Vec<u8>>,
    full: BTreeSet<String>,
    shared: BTreeSet<String>,
}

impl Lens {
    /// `full`: testers; `shared`: those who said yes to anonymous usage
    /// data. Without a key nobody is shown anonymised.
    pub fn new(viewer: &str, key: Option<Vec<u8>>, full: BTreeSet<String>, shared: BTreeSet<String>) -> Lens {
        Lens { viewer: viewer.into(), hidden_runs: BTreeSet::new(), key, full, shared }
    }

    pub fn run_sight(&self, run: &str, person: &str) -> Sight {
        if self.hidden_runs.contains(run) {
            Sight::Hidden
        } else {
            self.sight(person)
        }
    }

    pub fn sight(&self, person: &str) -> Sight {
        if person == self.viewer || self.full.contains(person) {
            Sight::Full
        } else if self.shared.contains(person) && self.key.is_some() {
            Sight::Anonymous
        } else {
            Sight::Hidden
        }
    }

    /// A workspace's things are shown only as far as every member allows.
    pub fn sight_of_all<'a>(&self, people: impl IntoIterator<Item = &'a String>) -> Sight {
        people.into_iter().map(|p| self.sight(p)).min().unwrap_or(Sight::Hidden)
    }

    /// The same id gives the same pseudonym on this server, and no developer
    /// can turn it back. A `person:` prefix stays, so the log still says who
    /// kind of actor it was.
    pub fn pseudonym(&self, id: &str) -> String {
        if let Some(rest) = id.strip_prefix("person:") {
            return format!("person:{}", self.pseudonym(rest));
        }
        let Some(key) = &self.key else { return "anon".into() };
        let Ok(mut mac) = Hmac::<sha2::Sha256>::new_from_slice(key) else { return "anon".into() };
        mac.update(id.as_bytes());
        let hex = hex::encode(mac.finalize().into_bytes());
        format!("anon-{}", &hex[..PSEUDONYM_HEX])
    }

    /// `v` as `sight` allows: as it is, through [`shape`], or nothing.
    pub fn show(&self, sight: Sight, v: Value) -> Option<Value> {
        match sight {
            Sight::Full => Some(v),
            Sight::Anonymous => Some(shape(self, &v)),
            Sight::Hidden => None,
        }
    }
}

/// Whether `s` is a bare token: an id, a kind, a model or tool name, a
/// hostname or a timestamp. Words, sentences, addresses and paths are not.
pub fn code(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_CODE_CHARS && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:+".contains(c)) && !s.contains("://")
}

fn label(s: &str) -> bool {
    s.chars().count() <= MAX_LABEL_CHARS && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:+/ ·,".contains(c)) && !s.contains("://")
}

/// `v` anonymised: content withheld, ids pseudonymised, codes and numbers kept.
pub fn shape(lens: &Lens, v: &Value) -> Value {
    walk(lens, None, v)
}

fn walk(lens: &Lens, key: Option<&str>, v: &Value) -> Value {
    let k = key.unwrap_or_default();
    // A count under such a key (`tokens.input`) is not content.
    if CONTENT_KEYS.contains(&k) && !v.is_number() {
        return Value::Null;
    }
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(name, x)| (name.clone(), walk(lens, Some(name), x))).collect::<Map<String, Value>>()),
        Value::Array(a) => Value::Array(a.iter().map(|x| walk(lens, key, x)).collect()),
        Value::String(s) if PSEUDONYM_KEYS.contains(&k) => Value::String(lens.pseudonym(s)),
        Value::String(s) if CODE_KEYS.contains(&k) && code(s) => v.clone(),
        Value::String(s) if LABEL_KEYS.contains(&k) && label(s) => v.clone(),
        Value::String(_) => Value::Null,
        other => other.clone(),
    }
}

/// Take every hidden run's id out of `v`: from lists, and as a value.
pub fn scrub(v: &mut Value, hidden: &BTreeSet<String>) {
    match v {
        Value::Array(a) => {
            a.retain(|x| x.as_str().is_none_or(|s| !hidden.contains(s)));
            a.iter_mut().for_each(|x| scrub(x, hidden));
        }
        Value::Object(m) => m.values_mut().for_each(|x| scrub(x, hidden)),
        Value::String(s) if hidden.contains(s.as_str()) => *v = Value::Null,
        _ => {}
    }
}

/// Making the key is one at a time, so two first views agree on it.
static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Kernel {
    /// The pseudonym key, made on first use. None while the vault is sealed.
    fn pseudonym_key(&self) -> Option<Vec<u8>> {
        let _g = MAKING.lock().ok()?;
        if let Ok(Some(k)) = self.vault.get(KEY_NAME) {
            return hex::decode(k.expose()).ok();
        }
        let mut raw = [0u8; KEY_BYTES];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut raw);
        self.vault.put(KEY_NAME, &hex::encode(raw)).ok()?;
        Some(raw.to_vec())
    }

    /// `viewer`'s lens on everyone, as the agreements stand now.
    pub fn lens(&self, viewer: &Person) -> Lens {
        let now = crate::legal::documents::Versions::of(&self.cfg.legal);
        let (full, shared) = self.read(|s| (s.legal.testers_now(&now), s.legal.sharing_now(&now)));
        let mut lens = Lens::new(&viewer.id, self.pseudonym_key(), full, shared);
        // A requester's tester agreement is not the team's consent. This also
        // covers whispers and descendants which inherited a team's context.
        lens.hidden_runs = self
            .read(|s| s.runs.values().filter(|r| s.lineage(&r.id).iter().any(|t| s.team.run_of(t).is_some() || s.team.whisper_of(t).is_some())).map(|r| r.id.clone()).collect());
        lens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_testers_consent_does_not_expose_a_team_run() {
        let mut l = Lens::new("developer", None, BTreeSet::from(["alice".into()]), BTreeSet::new());
        l.hidden_runs.insert("team-run".into());
        assert_eq!(l.run_sight("personal-run", "alice"), Sight::Full);
        assert_eq!(l.run_sight("team-run", "alice"), Sight::Hidden);
    }

    fn lens() -> Lens {
        Lens::new("dev", Some(vec![7; 32]), ["tess".to_string()].into(), ["ann".to_string()].into())
    }

    #[test]
    fn each_person_is_seen_as_they_chose() {
        let l = lens();
        assert_eq!(l.sight("dev"), Sight::Full, "a developer's own work is theirs");
        assert_eq!(l.sight("tess"), Sight::Full, "a tester is seen in full");
        assert_eq!(l.sight("ann"), Sight::Anonymous);
        assert_eq!(l.sight("nobody"), Sight::Hidden, "someone who has not said yes is not seen");
        let keyless = Lens::new("dev", None, BTreeSet::new(), ["ann".to_string()].into());
        assert_eq!(keyless.sight("ann"), Sight::Hidden, "without the key nobody can be shown anonymised");
        assert_eq!(l.sight_of_all(&["tess".to_string(), "ann".to_string()]), Sight::Anonymous);
        assert_eq!(l.sight_of_all(&["tess".to_string(), "bob".to_string()]), Sight::Hidden);
    }

    #[test]
    fn a_pseudonym_is_stable_keyed_and_not_the_id() {
        let l = lens();
        assert_eq!(l.pseudonym("ann"), l.pseudonym("ann"));
        assert_ne!(l.pseudonym("ann"), l.pseudonym("bob"));
        assert!(!l.pseudonym("ann").contains("ann"));
        assert_eq!(l.pseudonym("person:ann"), format!("person:{}", l.pseudonym("ann")));
        let other = Lens::new("dev", Some(vec![8; 32]), BTreeSet::new(), BTreeSet::new());
        assert_ne!(other.pseudonym("ann"), l.pseudonym("ann"), "another key gave the same pseudonym");
    }

    #[test]
    fn shaping_keeps_codes_and_numbers_and_withholds_words_ids_and_urls() {
        let l = lens();
        let v = json!({
            "run": "r1", "person": "ann", "title": "Email Bob about the merger", "status": "failed", "reason": "the model wrote ann@example.com",
            "usd": 0.25, "tokens": { "input": 120 }, "model": "claude-x", "tool": "gmail__send",
            "input": { "to": "bob@example.com", "status": "secret-status" }, "text": "hello",
            "url": "https://example.com/private/path?q=1", "host": "example.com", "source": "tool results: gmail__send",
            "harness": [{ "kind": "notice", "text": "Ann's file budget.xlsx" }], "name": "Ann",
        });
        let out = shape(&l, &v);
        let s = out.to_string();
        for leak in ["merger", "ann@example.com", "bob@example.com", "secret-status", "hello", "/private/path", "budget.xlsx", "\"Ann\"", "\"ann\""] {
            assert!(!s.contains(leak), "{leak} was shown: {s}");
        }
        assert_eq!(out["run"], "r1");
        assert_eq!(out["usd"], 0.25);
        assert_eq!(out["tokens"]["input"], 120);
        assert_eq!((out["model"].as_str(), out["tool"].as_str(), out["status"].as_str()), (Some("claude-x"), Some("gmail__send"), Some("failed")));
        assert_eq!(out["host"], "example.com");
        assert_eq!(out["harness"][0]["kind"], "notice");
        assert_eq!(out["person"], l.pseudonym("ann"));
        assert_eq!(out["source"], "tool results: gmail__send");
    }

    #[test]
    fn only_bare_tokens_count_as_codes() {
        for ok in ["budget", "claude-sonnet-4.5", "2026-11-11T10:00:00.123+00:00", "api.example.com", "gmail__send"] {
            assert!(code(ok), "{ok}");
        }
        for not in ["buy milk", "a@b.c", "/home/ann", "https://x.y", "", "Ann's"] {
            assert!(!code(not), "{not}");
        }
    }

    #[test]
    fn scrubbing_removes_hidden_runs_everywhere() {
        let mut v = json!({ "runs": ["r1", "r2"], "run": "r2", "breakdown": [{ "runs": ["r2"] }] });
        scrub(&mut v, &["r2".to_string()].into());
        assert_eq!(v, json!({ "runs": ["r1"], "run": null, "breakdown": [{ "runs": [] }] }));
    }
}
