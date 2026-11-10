//! Items left open under asks spec rev 140 retired, put in their current form
//! once at startup, so an update leaves the person's inbox holding only the
//! four asks (decisions/2026-11-10-permissions-asks.md):
//! - a criteria review completes its run with the criteria unconfirmed;
//! - a contract change is refused, and its run goes on against the frozen
//!   criteria;
//! - an approval outside the four is withdrawn, and the agent is told to do
//!   the action again if it still needs it, so the boundary judges it as now.
//!
//! The reviews of tool descriptions were retired the same way when they
//! are read (`consent::RETIRED_KINDS`).

use crate::events::Ev;
use crate::inbox::reply;
use crate::kernel::Kernel;
use crate::state::Item;
use crate::tools::Label;
use anyhow::Result;
use std::sync::Arc;

/// What the person's inbox says of a retired item.
pub const RETIRED: &str = "Retired by the update: Magnus no longer asks this.";
/// What the agent waiting on a withdrawn approval is told.
pub const WITHDRAWN: &str = "This action was not done: the approval it waited for is no longer asked. If it is still needed, do it again now; it is judged as any other action.";

/// What became of one retired item.
#[derive(Debug, PartialEq)]
pub enum Retired {
    /// A criteria review: the run completed, these criteria unconfirmed.
    Completed(Vec<String>),
    /// A contract change refused, the run going on.
    Refused,
    /// An approval outside the four withdrawn, the run going on.
    Withdrawn,
}

/// What an open item of a retired kind becomes, or None for a current one.
pub fn retiring(i: &Item) -> Option<Retired> {
    if i.resolution.is_some() {
        return None;
    }
    match i.kind.as_str() {
        "review" => Some(Retired::Completed(i.details["criteria"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_str().map(String::from)).collect())),
        "contract_change" => Some(Retired::Refused),
        "approval" if crate::asks::Reason::of(i.details["rule"].as_str()).is_none() => Some(Retired::Withdrawn),
        _ => None,
    }
}

impl Kernel {
    /// Retire every open item of a retired kind, each in one write with
    /// what its run is told. Run once at startup, before runs are driven.
    pub async fn retire_legacy_items(self: &Arc<Self>) -> Result<usize> {
        let open: Vec<(Item, Retired)> = self.read(|s| s.items.values().filter_map(|i| retiring(i).map(|r| (i.clone(), r))).collect());
        let n = open.len();
        for (item, what) in open {
            let run = item.run.clone().unwrap_or_default();
            let call_id = item.details["call_id"].as_str().unwrap_or_default().to_string();
            let mut evs: Vec<(&str, Ev)> = vec![("kernel", Ev::ItemResolved { id: item.id.clone(), decision: "superseded".into(), scope: None, text: Some(RETIRED.into()) })];
            let suspended_on_it = !run.is_empty() && self.read(|s| s.runs.get(&run).is_some_and(|r| r.status == crate::state::Status::Suspended));
            match &what {
                Retired::Completed(ids) => {
                    if suspended_on_it {
                        evs.extend(reply(&run, &call_id, format!("Completed. Not confirmed by any check: {}.", ids.join(", ")), false, Label::PRINCIPAL));
                        evs.push((
                            "kernel",
                            Ev::Completed {
                                run: run.clone(),
                                summary: item.details["summary"].as_str().unwrap_or_default().into(),
                                view: item.details.get("view").filter(|v| !v.is_null()).cloned(),
                                spoken: item.details["spoken"].as_str().map(String::from),
                            },
                        ));
                    }
                }
                Retired::Refused => {
                    evs.extend(reply(&run, &call_id, crate::run::FROZEN, true, Label::PRINCIPAL));
                    evs.extend(self.resume(&run));
                }
                Retired::Withdrawn => {
                    evs.extend(reply(&run, &call_id, WITHDRAWN, true, Label::PRINCIPAL));
                    evs.extend(self.resume(&run));
                }
            }
            self.emit_all(evs).await?;
            if suspended_on_it && matches!(what, Retired::Completed(_)) {
                self.on_stop(&run, true).await;
                self.after_end(&run);
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
#[path = "legacy_items_tests.rs"]
mod tests;
