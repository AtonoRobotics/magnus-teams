//! Sources a task may use from the person's computer. A host the computer
//! cannot reach yet is a new source: the person approves it for this task
//! alone, or always. A source granted to a task is reachable only while
//! that task runs; one allowed always joins the computer's destinations.
use super::service::checked_host;
use super::Change;
use crate::boundary::Action;
use crate::kernel::Kernel;
use crate::rule_effects::{NEW_SOURCE, SOURCE_TOOL};
use crate::state::Run;
use anyhow::{anyhow, Result};
use serde_json::Value;

impl Kernel {
    /// The host this action asks for, if the run's computer cannot reach it
    /// yet for this task.
    pub(crate) fn new_source(&self, a: &Action, run: &Run) -> Option<String> {
        if a.tool != SOURCE_TOOL {
            return None;
        }
        let host = checked_host(a.input["host"].as_str().unwrap_or_default()).ok()?;
        let id = self.run_computer(&run.person)?;
        let reachable =
            self.read(|s| s.computers.get(&id).is_some_and(|c| c.destinations.contains(&host) || c.task_destinations.iter().any(|(h, t)| *h == host && *t == run.task)));
        (!reachable).then_some(host)
    }

    /// Whether the person allowed this source always, not only for its task.
    fn source_allowed_always(&self, a: &Action, person: &str) -> bool {
        let hash = crate::boundary::input_hash(&a.tool, &a.input);
        self.read(|s| {
            s.approvals.iter().any(|ap| {
                ap.person == person && ap.tool == a.tool && ap.scope == "standing" && ap.reason.as_deref() == Some(NEW_SOURCE) && ap.input_hash.as_deref() == Some(hash.as_str())
            })
        })
    }

    /// A boundary action: the pipeline has already reviewed it by now.
    pub(crate) async fn add_destination_tool(&self, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let run = self.read(|s| s.runs.get(&a.run).cloned()).ok_or_else(|| anyhow!("unknown run {}", a.run))?;
        let person = self.person(&run.person).ok_or_else(|| anyhow!("the run has no person"))?;
        let Some(id) = self.run_computer(&person.id) else { return Ok(("You have no running computer.".to_string(), true, None)) };
        let host = a.input["host"].as_str().unwrap_or_default();
        if self.source_allowed_always(a, &person.id) {
            return Ok(match self.add_computer_destination(&person, &id, host).await {
                Ok(_) => (format!("{host} can now be reached from your computer."), false, None),
                Err(e) => (e.to_string(), true, None),
            });
        }
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
