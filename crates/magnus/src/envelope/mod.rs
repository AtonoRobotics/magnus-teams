//! Typed, user-only write envelopes (v2 security architecture).
//!
//! Write authority is derived strictly from trusted user actions (the user's
//! own prompt text and explicit grants) and never widened by untrusted content.
//! Contacts and the organisation directory only resolve names the user
//! actually wrote; they never silently pre-authorize unmentioned recipients.
//!
//! Payments are deliberately excluded from envelopes: every payment still
//! requires explicit human approval.
pub mod resolve;

use crate::state::Run;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// A normalized, typed external write destination.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    /// Canonicalized email address (lowercased; `+tag` stripped from local part).
    Email { local: String, domain: String },
    /// Domain-wide grant (e.g. `@company.com`), created only via explicit user grant.
    EmailDomain { domain: String },
    /// Immutable Slack channel ID (e.g. `C01234567`), never a mutable display name.
    SlackChannel { workspace: String, channel_id: String },
    /// Canonical `owner/repo` (lowercased).
    GitHubRepo { owner: String, repo: String },
    /// Canonical public hostname (lowercased).
    Host { host: String },
}

impl Destination {
    /// Parse and normalize an email address, stripping `+tag` suffixes.
    pub fn parse_email(raw: &str) -> Option<Destination> {
        let trimmed = raw.trim().trim_matches(|c| matches!(c, '<' | '>' | '"' | '\''));
        if trimmed.is_empty() || trimmed.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) {
            return None;
        }
        let (local_raw, domain_raw) = trimmed.split_once('@')?;
        if local_raw.is_empty() || domain_raw.is_empty() || domain_raw.contains('@') || !domain_raw.contains('.') {
            return None;
        }
        let local_base = local_raw.split_once('+').map_or(local_raw, |(base, _)| base);
        if local_base.is_empty() {
            return None;
        }
        Some(Destination::Email { local: local_base.to_ascii_lowercase(), domain: domain_raw.to_ascii_lowercase() })
    }

    /// Parse a domain-wide email grant (e.g. `company.com` or `@company.com`).
    pub fn parse_email_domain(raw: &str) -> Option<Destination> {
        let d = raw.trim().trim_start_matches('@').to_ascii_lowercase();
        if d.is_empty() || !d.contains('.') || d.contains('@') || d.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) {
            return None;
        }
        Some(Destination::EmailDomain { domain: d })
    }

    /// Parse an immutable Slack channel ID (`C...`, `G...`, `D...`).
    pub fn parse_slack_channel(workspace: &str, channel_id: &str) -> Option<Destination> {
        let id = channel_id.trim();
        let valid_prefix = id.starts_with('C') || id.starts_with('G') || id.starts_with('D');
        if !valid_prefix || id.len() < 5 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        Some(Destination::SlackChannel { workspace: workspace.trim().to_ascii_lowercase(), channel_id: id.to_string() })
    }

    /// Parse a GitHub `owner/repo` string.
    pub fn parse_github_repo(raw: &str) -> Option<Destination> {
        let trimmed = raw.trim().trim_matches('/');
        let (owner, repo) = trimmed.split_once('/')?;
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        Some(Destination::GitHubRepo { owner: owner.to_ascii_lowercase(), repo: repo.to_ascii_lowercase() })
    }

    /// Parse a hostname or URL host.
    pub fn parse_host(raw: &str) -> Option<Destination> {
        let trimmed = raw.trim();
        let host = if trimmed.contains("://") { url::Url::parse(trimmed).ok()?.host_str()?.to_ascii_lowercase() } else { trimmed.trim_end_matches('/').to_ascii_lowercase() };
        if host.is_empty() || host.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) {
            return None;
        }
        Some(Destination::Host { host })
    }

    /// Parse a raw destination string in the context of `app`.
    pub fn parse_for_app(app: &str, raw: &str) -> Option<Destination> {
        if raw.contains('@') {
            return Self::parse_email(raw);
        }
        match app {
            "slack" => Self::parse_slack_channel("default", raw),
            "github" | "gitlab" => Self::parse_github_repo(raw),
            "web" | "browser" | "computer" => Self::parse_host(raw),
            _ => Self::parse_email(raw).or_else(|| Self::parse_github_repo(raw)).or_else(|| Self::parse_host(raw)),
        }
    }

    /// Whether this allowed destination covers `target`.
    pub fn covers(&self, target: &Destination) -> bool {
        match (self, target) {
            (Destination::EmailDomain { domain }, Destination::Email { domain: t_domain, .. }) => domain == t_domain,
            (a, b) => a == b,
        }
    }

    /// Human-readable display string for cards and provenance labels.
    pub fn display(&self) -> String {
        match self {
            Destination::Email { local, domain } => format!("{local}@{domain}"),
            Destination::EmailDomain { domain } => format!("@{domain}"),
            Destination::SlackChannel { channel_id, .. } => channel_id.clone(),
            Destination::GitHubRepo { owner, repo } => format!("{owner}/{repo}"),
            Destination::Host { host } => host.clone(),
        }
    }
}

/// Write authority for a single turn, task, or schedule.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    pub scope_id: String,
    pub allowed_destinations: BTreeSet<Destination>,
    pub allowed_write_apps: BTreeSet<String>,
    /// The names/targets extracted from the user's own words (never a directory dump).
    pub user_named: Vec<String>,
    pub grant_ids: Vec<String>,
}

impl Envelope {
    pub fn allows_destination(&self, target: &Destination) -> bool {
        self.allowed_destinations.iter().any(|allowed| allowed.covers(target))
    }

    pub fn allows_all(&self, targets: &[Destination]) -> bool {
        !targets.is_empty() && targets.iter().all(|t| self.allows_destination(t))
    }
}

/// Deterministic check for cross-thread replies to an untrusted external sender:
/// when a run has read untrusted content AND holds private data from outside
/// the current thread/app, replying to that sender must be staged as a draft
/// rather than sent automatically.
pub fn is_cross_thread_untrusted_reply(run: &Run, current_app: &str) -> bool {
    if !run.untrusted || !run.private {
        return false;
    }
    if run.private_from.is_empty() {
        // Private taint came from uploads, commitments, or inherited private context.
        return true;
    }
    run.private_from.iter().any(|src| src != current_app)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Compiled {
        run: String,
        scope_id: String,
        #[serde(default)]
        user_named: Vec<String>,
        #[serde(default)]
        grant_ids: Vec<String>,
        #[serde(default)]
        allowed_destinations: Vec<Destination>,
        #[serde(default)]
        allowed_write_apps: Vec<String>,
    },
    GrantAdded {
        scope_id: String,
        destination: Destination,
        grant_id: String,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    by_scope: HashMap<String, Envelope>,
    run_scope: HashMap<String, String>,
}

impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.run_scope.retain(|run, _| !who.runs.contains(run));
        self.by_scope.retain(|scope, _| !who.tasks.contains(scope) && !who.runs.contains(scope));
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Compiled { run, scope_id, user_named, grant_ids, allowed_destinations, allowed_write_apps } => {
                let env = Envelope {
                    scope_id: scope_id.clone(),
                    allowed_destinations: allowed_destinations.iter().cloned().collect(),
                    allowed_write_apps: allowed_write_apps.iter().cloned().collect(),
                    user_named: user_named.clone(),
                    grant_ids: grant_ids.clone(),
                };
                self.by_scope.insert(scope_id.clone(), env);
                self.run_scope.insert(run.clone(), scope_id.clone());
            }
            Change::GrantAdded { scope_id, destination, grant_id } => {
                let env = self.by_scope.entry(scope_id.clone()).or_insert_with(|| Envelope { scope_id: scope_id.clone(), ..Default::default() });
                env.allowed_destinations.insert(destination.clone());
                if !env.grant_ids.contains(grant_id) {
                    env.grant_ids.push(grant_id.clone());
                }
            }
        }
    }

    pub fn for_run(&self, run: &str) -> Option<&Envelope> {
        let scope = self.run_scope.get(run)?;
        self.by_scope.get(scope)
    }

    pub fn for_scope(&self, scope_id: &str) -> Option<&Envelope> {
        self.by_scope.get(scope_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_destinations_normalize_emails_and_match_domains() {
        let a = Destination::parse_email("Alice+tag@Example.COM").unwrap();
        let b = Destination::parse_email("alice@example.com").unwrap();
        assert_eq!(a, b);
        let dom = Destination::parse_email_domain("@example.com").unwrap();
        assert!(dom.covers(&a));
        assert!(!dom.covers(&Destination::parse_email("alice@evil-example.com").unwrap()));
    }

    #[test]
    fn slack_channels_require_immutable_ids_not_display_names() {
        assert!(Destination::parse_slack_channel("ws1", "C01234567").is_some());
        assert!(Destination::parse_slack_channel("ws1", "#general").is_none());
        assert!(Destination::parse_slack_channel("ws1", "general").is_none());
    }

    #[test]
    fn envelope_rejects_unmentioned_destinations() {
        let mut env = Envelope { scope_id: "t1".into(), ..Default::default() };
        env.allowed_destinations.insert(Destination::parse_email("jane@co.com").unwrap());
        assert!(env.allows_destination(&Destination::parse_email("JANE+news@co.com").unwrap()));
        assert!(!env.allows_destination(&Destination::parse_email("bob@vendor.com").unwrap()));
    }
}
