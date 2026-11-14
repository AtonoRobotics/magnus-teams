//! One rule for what a run may use (decisions/2026-11-14-one-admission.md):
//! what the model is offered and what the boundary lets through come from
//! the same `Admission::admits`, so they cannot drift apart. Offering is
//! the run's catalog filtered by it; the boundary's policy stage calls it
//! for the same rules. The switches are still checked again where a call
//! goes out (`halts`), on purpose: that is a fence at the time of use, not
//! a second copy of the rule.
use crate::kernel::Kernel;
use crate::run::kind::RunKind;
use crate::state::{Run, State};
use crate::tools;
use magnus_adapter_proto::ToolSpec;
use std::collections::BTreeSet;

pub(crate) mod placement;

/// Why a run may not use a tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A kill switch holds the run, or this tool's connector: the run
    /// waits with `reason`, and `said` is what it is told.
    Halted { reason: String, said: String },
    /// Refused, in the words the run is told; `rule` names a hard denial.
    Denied { why: String, rule: Option<&'static str> },
}

/// The rule's name on a refusal nothing can permit.
pub(crate) const HARD_DENIAL: &str = "hard_denial";

/// What the rule needs from outside the log, read before the state is:
/// the organisation's apps a workspace may use (on, with no per-person
/// sign-in), and whether this server has computers.
#[derive(Clone, Debug, Default)]
pub(crate) struct Setting {
    pub shared_apps: BTreeSet<String>,
    pub computers: bool,
}

/// The rule for one run, over the state as it is now.
pub(crate) struct Admission<'a> {
    s: &'a State,
    run: &'a Run,
    kind: RunKind,
    setting: &'a Setting,
}

impl<'a> Admission<'a> {
    pub(crate) fn new(s: &'a State, run: &'a Run, setting: &'a Setting) -> Self {
        Admission { s, run, kind: s.run_kind(&run.id), setting }
    }

    /// Whether the run may use `tool`, and if not, why: the switches, where
    /// it is (chat, a team thread, a schedule), a sub-agent's limits, and a
    /// team run's scope, in that order. A run record is the run's own
    /// bookkeeping, not an action: only the switches and where it is judge it.
    pub(crate) fn admits(&self, tool: &str) -> Result<(), Refusal> {
        if let Some((reason, said)) = crate::halts::halt(self.s, self.run, Some(tool)) {
            return Err(Refusal::Halted { reason, said });
        }
        if let Some(why) = placement::placement(&self.kind, tool) {
            return Err(Refusal::Denied { why, rule: None });
        }
        if tools::RECORDS.contains(&tool) {
            return Ok(());
        }
        let hard = |why: String| Refusal::Denied { why, rule: Some(HARD_DENIAL) };
        if let Some(why) = self.s.sub_agents.refusal(&self.run.id, tool) {
            return Err(hard(why));
        }
        match crate::team::refusal(self.s, self.run, tool, &self.setting.shared_apps, self.setting.computers) {
            Some(why) => Err(hard(why)),
            None => Ok(()),
        }
    }

    /// The tools of `specs` the run may use, in their order.
    pub(crate) fn filter(&self, specs: Vec<ToolSpec>) -> Vec<ToolSpec> {
        specs.into_iter().filter(|t| self.admits(&t.name).is_ok()).collect()
    }
}

impl Kernel {
    /// What the admission rule reads from outside the log.
    pub(crate) fn admission_setting(&self) -> Setting {
        let shared_apps = self.connectors().into_iter().filter(|c| !crate::team::signs_in_per_person(c)).map(|c| c.name).collect();
        Setting { shared_apps, computers: self.cfg.computer.is_some() }
    }

    /// Whether `run` may use `tool` now (`Admission::admits`).
    pub(crate) fn admits(&self, run: &Run, tool: &str) -> Result<(), Refusal> {
        let setting = self.admission_setting();
        self.read(|s| Admission::new(s, run, &setting).admits(tool))
    }

    /// The tools of `specs` `run` may use now.
    pub(crate) fn admitted(&self, run: &Run, specs: Vec<ToolSpec>) -> Vec<ToolSpec> {
        let setting = self.admission_setting();
        self.read(|s| Admission::new(s, run, &setting).filter(specs))
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
