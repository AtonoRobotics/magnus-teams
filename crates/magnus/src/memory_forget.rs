//! `memory_forget`: Magnus takes back a remembered fact that is wrong or
//! that the person took back. The owner, 2026-11-27, asked whether the
//! model should be able to forget or correct a fact itself: "Yes, add it".
//!
//! It has the same bounds as `memory_write`:
//! - only in a run the person spoke in;
//! - only their own current facts;
//! - under the same per-run cap.
//!
//! Forgetting moves the fact to the person's trash, as Forget in the app
//! does, so they can restore it. A run that has read untrusted content may
//! not forget a fact from a clean run. Otherwise a page could erase what
//! the person taught.

use crate::kernel::Kernel;
use serde_json::Value;

impl Kernel {
    pub(crate) async fn memory_forget(&self, run_id: &str, id: &str) -> anyhow::Result<(String, bool, Option<Value>)> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(("no such run".into(), true, None)) };
        let cap = crate::memory_index::MAX_MEMORY_WRITES_PER_RUN;
        if crate::tools_core::memories_written(&run) >= cap {
            return Ok((format!("memory change limit ({cap}) reached for this run; skip further changes."), true, None));
        }
        let person_spoke = self.run_words(&run).await?.asked(&[crate::run_words::Source::Person]).iter().any(|t| !t.trim().is_empty());
        if !person_spoke {
            return Ok(("Not forgotten: memory changes only on what the person says, and the person said nothing in this run.".into(), true, None));
        }
        let id = id.trim();
        let Some(fact) = self.read(|s| s.current_memory(&run.person).into_iter().find(|m| m.id == id)) else {
            return Ok((format!("No remembered fact {id:?}: use the id shown in brackets with the fact."), true, None));
        };
        if run.untrusted && !fact.label.untrusted {
            return Ok((
                "Not forgotten: this run has read untrusted content, and a fact remembered from a clean conversation is forgotten only in one, or by the person under Memory in Settings.".into(),
                true,
                None,
            ));
        }
        Ok(match self.trash(&run.person, false, id).await? {
            Ok(()) => (format!("forgotten ({id}); the person can restore it from their trash."), false, None),
            // Its writing run's record goes with it, so that run must have
            // ended (invariant 3).
            Err(crate::state::NotTrashable::Running) => {
                (format!("Not forgotten yet: {id} was remembered in a run that is still going, this one or another; it can be forgotten once that run ends."), true, None)
            }
            Err(crate::state::NotTrashable::NotFound) => (format!("No remembered fact {id:?} could be forgotten."), true, None),
        })
    }
}

#[cfg(test)]
#[path = "memory_forget_tests.rs"]
mod tests;
