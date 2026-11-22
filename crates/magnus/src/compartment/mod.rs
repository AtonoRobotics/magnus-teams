//! Compartmentalized worker execution (v2 security architecture).
//!
//! Splits execution into three kernel-enforced compartments:
//! - `Private`: full user context, OAuth connectors, memory, files, and
//!   default-deny VM egress. Direct `web_search` and `research_web` pass
//!   through the shared `query_gate`.
//! - `OpenWeb`: public web browsing with an ephemeral, cookie-free browser
//!   profile (`browser_*`, never `browser_as_*`) and zero access to private
//!   connectors, memory, or files. When spawned from a tainted `Private` run,
//!   `link_following_only` is set: it may only fetch URLs from its own search
//!   results, loaded pages, or the user's original prompt, and cannot type
//!   into site forms.
//! - `Quarantined`: zero tools; pure schema-constrained extraction of
//!   untrusted text.
pub mod query_gate;

use crate::boundary::Action;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel, NewRun};
use crate::sub_agents::SubAgent;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Fraction of parent budget granted to an `OpenWeb` research worker.
const RESEARCH_BUDGET_SHARE: f64 = 0.25;

/// Tools always admitted in an `OpenWeb` compartment (read-only browsing & search).
const OPEN_WEB_READ_TOOLS: [&str; 6] = ["web_search", "web_fetch", "browser_navigate", "browser_snapshot", "browser_screenshot", "browser_back"];

/// Interactive public browser tools admitted only in an unrestricted `OpenWeb`
/// run (never when `link_following_only` is true).
const OPEN_WEB_INTERACT_TOOLS: [&str; 3] = ["browser_click", "browser_type", "browser_select"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Compartment {
    #[default]
    Private,
    OpenWeb {
        /// When true (spawned from a tainted `Private` run), only URLs from
        /// the worker's own search results, loaded pages, or the user's
        /// original prompt may be opened, and form typing/selecting is denied.
        #[serde(default)]
        link_following_only: bool,
    },
    Quarantined,
}

impl Compartment {
    pub fn is_open_web(self) -> bool {
        matches!(self, Compartment::OpenWeb { .. })
    }

    pub fn is_link_following_only(self) -> bool {
        matches!(self, Compartment::OpenWeb { link_following_only: true })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Started {
        run: String,
        #[serde(default)]
        parent: Option<String>,
        compartment: Compartment,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    runs: HashMap<String, Compartment>,
}

impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.runs.retain(|run, _| !who.runs.contains(run));
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Started { run, compartment, .. } => {
                self.runs.insert(run.clone(), *compartment);
            }
        }
    }

    pub fn of(&self, run: &str) -> Compartment {
        self.runs.get(run).copied().unwrap_or(Compartment::Private)
    }

    /// Why `run`'s compartment refuses `tool`, or `None` when allowed.
    /// Run records (`task_plan`, `task_complete`, etc.) are admitted before
    /// this check in `Admission::admits`.
    pub fn refusal(&self, run: &str, tool: &str) -> Option<String> {
        match self.of(run) {
            Compartment::Private => None,
            Compartment::OpenWeb { link_following_only } => {
                if OPEN_WEB_READ_TOOLS.contains(&tool) {
                    return None;
                }
                if !link_following_only && OPEN_WEB_INTERACT_TOOLS.contains(&tool) {
                    return None;
                }
                if link_following_only && OPEN_WEB_INTERACT_TOOLS.contains(&tool) {
                    return Some(format!("{tool} is disabled in a link-following OpenWeb worker (no form typing or clicking from a tainted run)"));
                }
                Some(format!("{tool} is not available in an OpenWeb worker (no access to private connectors, files, memory, or signed-in browsers)"))
            }
            Compartment::Quarantined => Some(format!("{tool} is not available in a Quarantined reader (zero tool access)")),
        }
    }
}

/// Tools offered to an `OpenWeb` worker, narrowed to what the server supports.
pub(crate) fn open_web_tools(link_following_only: bool, has_search: bool, has_browser: bool) -> Vec<String> {
    let mut tools = vec!["web_fetch".to_string()];
    if has_search {
        tools.push("web_search".to_string());
    }
    if has_browser {
        for t in ["browser_navigate", "browser_snapshot", "browser_screenshot", "browser_back"] {
            tools.push(t.to_string());
        }
        if !link_following_only {
            for t in OPEN_WEB_INTERACT_TOOLS {
                tools.push(t.to_string());
            }
        }
    }
    tools
}

impl Kernel {
    /// `research_web`: spawns an isolated `OpenWeb` worker with no parent
    /// history, no private taint, and an ephemeral cookie-free browser.
    ///
    /// The query passes `check_web_query` (shared byte budget + deterministic
    /// scrub + mandatory outgoing classifier when `private && untrusted` +
    /// denial retry cap) before the child is created.
    pub(crate) async fn research_web(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let parent = self.read(|s| s.runs.get(&a.run).cloned()).context("no such run")?;
        let query = a.input["query"].as_str().map(str::trim).unwrap_or_default();
        if query.is_empty() {
            return Ok(("research_web requires a non-empty query.".into(), true, None));
        }
        let goal_extra = a.input["goal"].as_str().map(str::trim).filter(|g| !g.is_empty());
        let combined = match goal_extra {
            Some(g) => format!("{query}\n{g}"),
            None => query.to_string(),
        };

        // Shared query gate (same budget, scrubber, classifier & retry cap as web_search).
        if let Some(why) = self.check_web_query(a, &parent, &combined).await? {
            return Ok((why, true, None));
        }

        let tainted = parent.private && parent.untrusted;
        let compartment = Compartment::OpenWeb { link_following_only: tainted };
        let tools = open_web_tools(tainted, self.web.can_search(), self.browser.available());
        let (task, child) = (new_id("task"), new_id("run"));

        let prompt = format!(
            "Research the following public web question using web search, public HTTPS sites, and link-following. Return a concise summary with exact source URLs.\n\nQuery: {combined}"
        );
        let budget = crate::sub_agents::child_budget_for(&parent.config.budget, parent.spent_usd(), Some(RESEARCH_BUDGET_SHARE));
        let o = NewRun {
            // Clean OpenWeb compartment: never inherits parent.private or parent history.
            label: None,
            budget: Some(budget),
            model: Some((parent.config.adapter.clone(), parent.config.model.clone())),
            ids: Some((task.clone(), child.clone())),
            team: self.team_of(&parent.task).map(|t| crate::team::TeamRun { sub: true, ..t }),
            ..Default::default()
        };

        // Record compartment & sub-agent lineage before starting the child run.
        self.emit("kernel", Ev::Compartment(Change::Started { run: child.clone(), parent: Some(a.run.clone()), compartment })).await?;
        self.emit("kernel", Ev::SubAgent(SubAgent { child: child.clone(), parent: a.run.clone(), tools })).await?;
        self.create_run(&format!("run:{}", a.run), &parent.person, prompt, vec![], o).await?;
        self.wake(&child);
        let (text, is_error, _child_meta) = self.wait_for_sub_agent(&task, &child).await?;
        // Output returns to Private parent labeled untrusted (never private).
        Ok((text, is_error, Some(json!({ "untrusted": true, "private": false, "child_run": child, "task": task }))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_web_compartment_blocks_all_private_and_signed_in_tools() {
        let mut ledger = Ledger::default();
        ledger.apply(&Change::Started { run: "r_tainted".into(), parent: Some("p".into()), compartment: Compartment::OpenWeb { link_following_only: true } });
        ledger.apply(&Change::Started { run: "r_pure".into(), parent: None, compartment: Compartment::OpenWeb { link_following_only: false } });
        ledger.apply(&Change::Started { run: "r_quar".into(), parent: Some("p".into()), compartment: Compartment::Quarantined });

        // Allowed in both OpenWeb modes.
        for t in OPEN_WEB_READ_TOOLS {
            assert!(ledger.refusal("r_tainted", t).is_none(), "{t} refused in tainted OpenWeb");
            assert!(ledger.refusal("r_pure", t).is_none(), "{t} refused in pure OpenWeb");
        }
        // Form typing/clicking allowed in pure OpenWeb, blocked in link-following OpenWeb.
        for t in OPEN_WEB_INTERACT_TOOLS {
            assert!(ledger.refusal("r_pure", t).is_none(), "{t} refused in pure OpenWeb");
            assert!(ledger.refusal("r_tainted", t).is_some(), "{t} allowed in tainted OpenWeb");
        }
        // Private connectors, memory, files, signed-in browser & computer blocked in all OpenWeb modes.
        for private_tool in [
            "gmail__list_messages",
            "memory_search",
            "playbook_search",
            "commitments_search",
            "project_read",
            "bundle_read",
            "browser_as_navigate",
            "browser_sign_in",
            "computer_exec",
            "sandbox_read_file",
            "start_sub_agent",
        ] {
            assert!(ledger.refusal("r_tainted", private_tool).is_some(), "{private_tool} leaked into tainted OpenWeb");
            assert!(ledger.refusal("r_pure", private_tool).is_some(), "{private_tool} leaked into pure OpenWeb");
        }
        // Quarantined has zero tools.
        assert!(ledger.refusal("r_quar", "web_search").is_some());
        assert!(ledger.refusal("r_quar", "web_fetch").is_some());
    }
}
