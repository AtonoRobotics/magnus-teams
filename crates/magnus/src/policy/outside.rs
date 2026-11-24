//! Outside agents as Cedar principals (owner's design, Outside agents): an
//! outside agent is `OutsideAgent::"<id>"`, never a `Person`, and asking
//! Magnus is the action `ask` on one person or one shared project. Nothing
//! is allowed until a policy admits it: the set evaluated is the agent's
//! admissions alone, with no default permit, so an agent no policy admits
//! is refused, and so is any evaluation error.

use super::{eval, uid};
use crate::a2a::Target;
use anyhow::{anyhow, Result};
use cedar_policy::{Context, Entities, Entity, Request};
use std::collections::HashSet;

/// The schema of this side: who asks, and whom.
pub const OUTSIDE_SCHEMA: &str = r#"entity OutsideAgent;
entity Person;
entity Project;
action "ask" appliesTo { principal: [OutsideAgent], resource: [Person, Project] };"#;

fn resource(to: &Target) -> Result<cedar_policy::EntityUid> {
    match to {
        Target::Person(p) => uid("Person", p),
        Target::Project(w) => uid("Project", w),
    }
}

/// The policy that admits `agent` to ask `to`.
pub fn admission(agent: &str, to: &Target) -> Result<String> {
    Ok(format!("permit(principal == {}, action == Action::\"ask\", resource == {});", uid("OutsideAgent", agent)?, resource(to)?))
}

/// Whether `policies` (the agent's admissions) let `agent` ask `to`.
pub fn admits(policies: &[String], agent: &str, to: &Target) -> Result<bool> {
    if policies.is_empty() {
        return Ok(false);
    }
    let principal = uid("OutsideAgent", agent)?;
    let resource = resource(to)?;
    let ents = Entities::from_entities([Entity::new_no_attrs(principal.clone(), HashSet::new()), Entity::new_no_attrs(resource.clone(), HashSet::new())], None)
        .map_err(|e| anyhow!("{e}"))?;
    let req = Request::new(principal, uid("Action", "ask")?, resource, Context::empty(), None).map_err(|e| anyhow!("{e}"))?;
    Ok(eval(&policies.join("\n"), "outside_agent", &req, &ents)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Admitted for mo, the agent may ask mo and no one else, nor any
    /// project; another agent may ask no one; with no policy, nothing.
    #[test]
    fn an_outside_agent_asks_only_whom_a_policy_admits() {
        let mo = Target::Person("mo".into());
        let p = vec![admission("oa_1", &mo).unwrap()];
        assert!(admits(&p, "oa_1", &mo).unwrap());
        assert!(!admits(&p, "oa_1", &Target::Person("ana".into())).unwrap());
        assert!(!admits(&p, "oa_1", &Target::Project("ws_1".into())).unwrap());
        assert!(!admits(&p, "oa_2", &mo).unwrap());
        assert!(!admits(&[], "oa_1", &mo).unwrap());
        let ws = vec![admission("oa_1", &Target::Project("ws_1".into())).unwrap()];
        assert!(admits(&ws, "oa_1", &Target::Project("ws_1".into())).unwrap());
        assert!(!admits(&ws, "oa_1", &mo).unwrap());
    }
}
