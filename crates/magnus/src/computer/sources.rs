//! Sources a task may use from the person's computer. A host the computer
//! cannot reach yet is a new source. It never asks on its own (spec rev 140,
//! Asking grants the work): Auto Review judges it as a boundary action, and
//! once through it is granted to this task, reachable only while the task
//! runs.
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
        let granted = Change::DestinationGranted { id, host: host.clone(), task: run.task.clone() };
        Ok(match self.record_computer(&person, granted).await {
            Ok(()) => (format!("{host} can be reached from your computer for this task."), false, None),
            Err(e) => (e.to_string(), true, None),
        })
    }
}
