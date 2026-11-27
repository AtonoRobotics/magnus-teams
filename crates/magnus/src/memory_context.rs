//! The person's remembered facts in every run of theirs. The owner,
//! 2026-11-27, asked whether runs should start with the person's facts
//! loaded rather than only when the model searches: "Yes, load them".
//!
//! Only clean facts enter: a fact filed by a run that had read outside or
//! private content keeps that taint, and loading it would hand it to every
//! run untainted. Those stay with `memory_search`, which carries the taint
//! of what it returns. The facts are those kept when the run started, as
//! its relationship notes are, so its context is the same at every call.

use crate::kernel::Kernel;
use crate::recall::Remembered;
use crate::state::Run;

/// The most the loaded facts take of a run's context, in bytes: about a
/// thousand tokens, some fifteen facts at the longest a fact may be.
pub const MAX_LOADED_MEMORY_BYTES: usize = 4096;

impl Kernel {
    /// The memory section of `run`'s system context: empty for a team's
    /// run or a project's channel, where personal memory never appears.
    pub(crate) async fn memory_section(&self, run: &Run) -> anyhow::Result<String> {
        if self.run_kind(&run.id).in_team() || self.read(|s| s.in_project_channel(&run.id)) {
            return Ok(String::new());
        }
        let started = run.created_at;
        let facts = self.remembered(&run.person, |m| m.at <= started).await?;
        Ok(section(&facts))
    }
}

/// The person's clean facts, newest first while they fit
/// `MAX_LOADED_MEMORY_BYTES`, and a line saying how many more
/// `memory_search` can find. Empty when nothing is remembered.
pub(crate) fn section(facts: &[Remembered]) -> String {
    let mut lines = String::new();
    let mut more = 0;
    for f in facts.iter().rev() {
        let line = match &f.topic {
            Some(topic) => format!("- {} ({topic})\n", f.fact.trim()),
            None => format!("- {}\n", f.fact.trim()),
        };
        if f.entry.label.untrusted || f.entry.label.private || lines.len() + line.len() > MAX_LOADED_MEMORY_BYTES {
            more += 1;
            continue;
        }
        lines.push_str(&line);
    }
    if lines.is_empty() && more == 0 {
        return String::new();
    }
    let mut s = String::from("\n\n# What you remember about this person\n\nFacts they told you in earlier conversations, newest first. Use them where they bear on what they ask, without being asked and without reciting them. When they correct one, remember the correction.\n\n");
    s.push_str(&lines);
    if more > 0 {
        s.push_str(&format!("\n{more} more remembered facts are not shown here; memory_search finds them.\n"));
    }
    s
}

#[cfg(test)]
#[path = "memory_context_tests.rs"]
mod tests;
