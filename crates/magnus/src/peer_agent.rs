//! Foundation hooks for Peer-Agent Delegation (Claude Code / Codex) inside
//! server-side Firecracker `Role::Computer` VMs (W3.4, M11, N8, P7, O3).
//!
//! - Ships server-side Firecracker VM execution first (deferring local-machine
//!   OS sandboxing on macOS/Windows to a separate decision record, P7).
//! - Routes provider API traffic strictly through Magnus's `computer/egress/`
//!   proxy using the surrogate pattern: strips any client-supplied auth headers
//!   (`Authorization`, `x-api-key`) and injects the user's own provider API key
//!   from the vault (`N8`), preventing a hijacked peer agent from exfiltrating
//!   data through an attacker's provider account.
//! - All peer-agent outputs enter Magnus labeled `BOTH { untrusted: true, private: true }`.

use crate::compartment::query_gate::scrub_check;
use crate::tools::Label;
use serde::{Deserialize, Serialize};

pub const PEER_AGENT_OUTPUT_LABEL: Label = Label { untrusted: true, private: true };
pub const MAX_PEER_BRIEF_BYTES: usize = 8 * 1024;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PeerAgentKind {
    ClaudeCode,
    Codex,
}

impl PeerAgentKind {
    /// Allowed upstream provider API host for surrogate-proxied execution (N8).
    pub fn provider_host(self) -> &'static str {
        match self {
            PeerAgentKind::ClaudeCode => "api.anthropic.com",
            PeerAgentKind::Codex => "api.openai.com",
        }
    }

    /// Header name used when the Magnus egress proxy injects the user's vault API key.
    pub fn auth_header(self) -> &'static str {
        match self {
            PeerAgentKind::ClaudeCode => "x-api-key",
            PeerAgentKind::Codex => "authorization",
        }
    }

    /// Vault secret key name storing the user's provider API key (P7: requires
    /// API key in vault, not a consumer subscription login).
    pub fn vault_key(self, person: &str) -> String {
        let slug = match self {
            PeerAgentKind::ClaudeCode => "claude_code",
            PeerAgentKind::Codex => "codex",
        };
        format!("peer_agent/{person}/{slug}/api_key")
    }
}

/// Strip any client-supplied provider auth headers before the Magnus egress
/// proxy injects the user's own vault credential (prevents attacker API key
/// exfiltration over an allowed provider host, N8).
pub fn strip_client_auth_headers(headers: &mut Vec<(String, String)>) {
    headers.retain(|(name, _)| {
        let lower = name.to_ascii_lowercase();
        lower != "authorization" && lower != "x-api-key" && lower != "openai-organization" && lower != "openai-project"
    });
}

/// Assemble a minimal, scrubbed, task-specific brief for a peer agent (O3).
/// Refuses to emit a brief containing credentials/PII detected by `scrub_check`.
pub fn build_scrubbed_brief(task_goal: &str, clean_model_snippet: &str, routing_pointers: &str) -> Result<String, String> {
    let mut brief = format!("Task:\n{}\n", task_goal.trim());
    if !clean_model_snippet.trim().is_empty() {
        brief.push_str("\nPreferences:\n");
        brief.push_str(clean_model_snippet.trim());
        brief.push('\n');
    }
    if !routing_pointers.trim().is_empty() {
        brief.push_str("\nWorkspace pointers:\n");
        brief.push_str(routing_pointers.trim());
        brief.push('\n');
    }
    if brief.len() > MAX_PEER_BRIEF_BYTES {
        let mut cut = MAX_PEER_BRIEF_BYTES;
        while !brief.is_char_boundary(cut) {
            cut -= 1;
        }
        brief.truncate(cut);
    }
    if let Some(reason) = scrub_check(&brief) {
        return Err(format!("refused peer-agent brief: {reason}"));
    }
    Ok(brief)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_attacker_supplied_auth_headers_before_surrogate_injection() {
        let mut headers = vec![
            ("Content-Type".into(), "application/json".into()),
            ("Authorization".into(), "Bearer sk-attacker-key".into()),
            ("x-api-key".into(), "sk-ant-attacker".into()),
            ("OpenAI-Organization".into(), "org-attacker".into()),
        ];
        strip_client_auth_headers(&mut headers);
        assert_eq!(headers, vec![("Content-Type".into(), "application/json".into())]);
        assert_eq!(PeerAgentKind::ClaudeCode.provider_host(), "api.anthropic.com");
        assert_eq!(PeerAgentKind::Codex.provider_host(), "api.openai.com");
        assert_eq!(PeerAgentKind::ClaudeCode.vault_key("alice"), "peer_agent/alice/claude_code/api_key");
    }

    #[test]
    fn brief_builder_rejects_leaked_secrets() {
        assert!(build_scrubbed_brief("Fix CI test in repo", "- Keep diffs minimal", "- repo owner/app").is_ok());
        assert!(build_scrubbed_brief("Use key sk-1234567890abcdef", "", "").is_err());
    }
}
