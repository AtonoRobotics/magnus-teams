//! Tool families: each built-in table, the specs it can offer and what
//! carries its tools out, in one list. `tools::builtin`, the reserved app
//! names, the per-person specs and the boundary's dispatch all read
//! `FAMILIES`, so a family added here is added everywhere at once.
use crate::boundary::Action;
use crate::kernel::Kernel;
use crate::tools::Builtin;
use anyhow::{anyhow, Result};
use magnus_adapter_proto::ToolSpec;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// The sandbox, the web, the browser, the run's computer, memory, the
    /// knowledge base and bundle, apps and chat (`tools.rs`).
    Core,
    /// The person's own computer (`local.rs`).
    Local,
    /// Programs on the person's desktop, through the bridge (`desktop.rs`).
    Desktop,
    /// A team workspace (`team/agent.rs`).
    Team,
    /// Pictures from the image provider (`tools_ui.rs`).
    Images,
    /// Connectors Magnus builds (`generated.rs`).
    Connector,
    /// Purchases (`payments`).
    Payments,
}

/// Every family, in the order their specs are offered.
pub const FAMILIES: [Family; 7] = [Family::Core, Family::Local, Family::Desktop, Family::Team, Family::Images, Family::Connector, Family::Payments];

impl Family {
    /// The family's built-in tools, with their risk, label and audience.
    pub fn builtins(self) -> &'static [Builtin] {
        match self {
            Family::Core => crate::tools::BUILTINS,
            Family::Local => crate::local::BUILTINS,
            Family::Desktop => crate::desktop::BUILTINS,
            Family::Team => crate::team::BUILTINS,
            Family::Images => crate::tools_ui::BUILTINS,
            Family::Connector => crate::generated::BUILTINS,
            Family::Payments => crate::payments::BUILTINS,
        }
    }

    /// Every spec the family can offer, whatever a run is offered of them.
    /// A desktop program's own tools are the bridge's, not built in.
    pub fn specs(self) -> Vec<ToolSpec> {
        match self {
            Family::Core => {
                let mut v = crate::tools::builtin_specs(true, true, true, true);
                v.extend(crate::tools::chat_specs());
                v.push(crate::tools::find_tools_spec());
                v
            }
            Family::Local => crate::local::specs(),
            Family::Desktop => vec![crate::desktop::programs_spec()],
            Family::Team => crate::team::specs(),
            Family::Images => crate::tools_ui::specs(),
            Family::Connector => crate::generated::specs(),
            Family::Payments => crate::payments::specs(),
        }
    }

    /// The family that carries a tool out: the one listing it, or the
    /// desktop for a program's tool (`desktop_<program>__<tool>`).
    pub fn of(tool: &str) -> Option<Family> {
        FAMILIES.into_iter().find(|f| f.builtins().iter().any(|b| b.name == tool)).or_else(|| tool.starts_with(crate::desktop::APP_PREFIX).then_some(Family::Desktop))
    }
}

/// Every built-in tool, across the families.
pub fn builtins() -> impl Iterator<Item = &'static Builtin> {
    FAMILIES.into_iter().flat_map(Family::builtins)
}

impl Kernel {
    /// The tools offered beside the core list, for this person: each
    /// family's that applies to them.
    pub(crate) fn family_specs(&self, person: &str) -> Vec<ToolSpec> {
        FAMILIES.into_iter().flat_map(|f| self.offered_of(f, person)).collect()
    }

    fn offered_of(&self, f: Family, person: &str) -> Vec<ToolSpec> {
        match f {
            // Assembled for each run, with chat's and a project's rules (`run/mod.rs`).
            Family::Core => vec![],
            // A team run's own, given in `team/agent.rs`.
            Family::Team => vec![],
            Family::Local => self.local_specs(person),
            Family::Desktop => self.desktop_specs(person),
            Family::Images => crate::tools_ui::specs(),
            Family::Connector => crate::generated::specs(),
            Family::Payments => self.cfg.payments.iter().flat_map(|_| crate::payments::specs()).collect(),
        }
    }

    /// Carry out a built-in tool, or a desktop program's, by its family.
    pub(crate) async fn carry_out(self: &Arc<Self>, a: &Action, key: &str) -> Result<(String, bool, Option<Value>)> {
        let no_executor = || anyhow!("no executor for {}", a.tool);
        match Family::of(&a.tool).ok_or_else(no_executor)? {
            Family::Core => self.core_tool(crate::tools_core::op(&a.tool).ok_or_else(no_executor)?, a, key).await,
            Family::Local => self.local_tool(a).await,
            Family::Desktop => self.desktop_tool(a).await,
            Family::Team => self.team_tool(a).await,
            Family::Images => self.image_tool(a).await,
            Family::Connector => self.connector_tool(a).await,
            Family::Payments => self.purchase_tool(a).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{builtins, Family, FAMILIES};
    use std::collections::BTreeSet;

    #[test]
    fn every_builtin_has_a_spec_in_its_family() {
        for f in FAMILIES {
            let specs: BTreeSet<String> = f.specs().into_iter().map(|s| s.name).collect();
            for b in f.builtins() {
                assert!(specs.contains(b.name), "{} ({f:?}) has no spec", b.name);
            }
        }
    }

    #[test]
    fn every_builtin_is_carried_out_by_its_own_family() {
        for f in FAMILIES {
            for b in f.builtins() {
                assert_eq!(Family::of(b.name), Some(f), "{} is carried out by another family", b.name);
                if f == Family::Core {
                    assert!(crate::tools_core::op(b.name).is_some(), "{} has no executor", b.name);
                }
            }
        }
        assert_eq!(Family::of("desktop_word__open"), Some(Family::Desktop));
        assert_eq!(Family::of("gmail__send"), None);
    }

    #[test]
    fn builtin_names_are_unique_across_families() {
        let mut seen = BTreeSet::new();
        for b in builtins() {
            assert!(seen.insert(b.name), "{} is built in twice", b.name);
        }
    }

    #[test]
    fn every_builtin_app_is_reserved() {
        let apps = crate::tools::builtin_apps();
        for b in builtins() {
            assert!(apps.contains(&b.app), "{} ({}) is not reserved", b.app, b.name);
            assert!(crate::tools::builtin(b.name).is_some_and(|x| std::ptr::eq(x, b)));
        }
    }
}
