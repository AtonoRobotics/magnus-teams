//! Sources a task may use from the person's computer. A host the computer
//! cannot reach yet is a new source. It never asks on its own (spec rev 140,
//! Asking grants the work): Auto Review judges it as a boundary action, and
//! once through it stays reachable from the person's computer, with every
//! method, so the next task does not ask again and a form there can be sent
//! (2026-11-28-gates-beyond-the-invariants). Before, it was granted to the
//! one task (`Change::DestinationGranted`, still read from the log).
use super::service::checked_host;
use super::Change;
use crate::boundary::Action;
use crate::kernel::Kernel;
use anyhow::{anyhow, Result};
use serde_json::Value;

impl Kernel {
    /// A boundary action: the pipeline has already reviewed it by now.
    pub(crate) async fn add_destination_tool(&self, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let run = self.read(|s| s.runs.get(&a.run).cloned()).ok_or_else(|| anyhow!("unknown run {}", a.run))?;
        let person = self.person(&run.person).ok_or_else(|| anyhow!("the run has no person"))?;
        let id = match self.computer_for(&super::Owner::Person(person.id.clone())).await {
            Ok(Some(id)) => id,
            Ok(None) => return Ok(("This server has no computers.".to_string(), true, None)),
            Err(e) => return Ok((format!("Your computer could not be made ready: {e}"), true, None)),
        };
        let host = a.input["host"].as_str().unwrap_or_default();
        let host = match checked_host(host) {
            Ok(h) => h,
            Err(e) => return Ok((e.to_string(), true, None)),
        };
        let added = Change::DestinationAdded { id, host: host.clone() };
        Ok(match self.record_computer(&person, added).await {
            Ok(()) => (format!("{host} can be reached from your computer from now on."), false, None),
            Err(e) => (e.to_string(), true, None),
        })
    }
}

#[cfg(test)]
#[path = "sources_tests.rs"]
mod tests;
