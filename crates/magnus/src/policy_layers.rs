//! Who writes which policy layer, and what a layer may say (the owner,
//! 2026-11-11: "The rule change should be allowed by inherited policy").
//!
//! The layers stand in a hierarchy: the kernel's floor, then the
//! organisation's layer (`org`), then a group's (`group/<g>`), then a
//! person's own (`user/<id>`), then a task's (`task/<t>`). Only `org` widens:
//! what it permits is open to every layer below, which may forbid within it
//! and nothing more (`policy::decide`, `control_allowed` and `loop_allowed`
//! add a permit-all to each lower layer, so its forbids alone count). A
//! permit written below `org` therefore could only try to loosen what a
//! layer above holds, and it is refused when it is written or accepted,
//! naming the layers above it. The floor (prohibited tools never run, every
//! payment asks its person, Auto Review on every boundary action, the rule
//! of two, the hard denials) is compiled into the kernel and in no layer.

use crate::config::Person;
use cedar_policy::{Effect, PolicySet};
use std::str::FromStr;

/// Whether `p` may write, read as theirs, or accept a proposal for `layer`.
/// A person's own layer is theirs alone, with no role needed and nobody
/// else's say, the owner's included. The organisation's is the owner's; a
/// group's is the owner's and its policy role's. A task's layer is its
/// person's, which the caller checks with the task in hand.
pub fn may_write(p: &Person, layer: &str) -> bool {
    if let Some(u) = layer.strip_prefix("user/") {
        return !u.is_empty() && u == p.id;
    }
    if layer == "org" {
        return p.is_owner();
    }
    if let Some(g) = layer.strip_prefix("group/") {
        return !g.is_empty() && (p.is_owner() || p.has_role(&format!("policy:group:{g}")));
    }
    false
}

/// A layer the server's owner and policy roles keep: the organisation's or a
/// group's. A person's and a task's are not theirs to read.
pub fn kept_by_the_server(layer: &str) -> bool {
    layer == "org" || layer.strip_prefix("group/").is_some_and(|g| !g.is_empty() && !g.contains('/'))
}

/// The layers above `layer`, in words, for a refusal.
fn above(layer: &str) -> &'static str {
    if layer.starts_with("group/") {
        "the organisation's layer (org)"
    } else if layer.starts_with("user/") {
        "the organisation's layer (org) and your groups' layers"
    } else {
        "the organisation's layer (org), the groups' layers and the person's own layer"
    }
}

/// Whether `cedar`, written to `layer`, only tightens: below `org`, a
/// permit is refused with why. Text that does not parse is left to the
/// validator.
pub fn tightens_only(layer: &str, cedar: &str) -> Result<(), String> {
    if layer == "org" {
        return Ok(());
    }
    let Ok(set) = PolicySet::from_str(cedar) else { return Ok(()) };
    let permit = set.policies().find(|p| p.effect() == Effect::Permit).map(|p| p.id().to_string());
    match permit {
        None => Ok(()),
        Some(id) => Err(format!(
            "{layer} can only tighten what the layers above it allow: the permit {id} would loosen what {} holds, so it is refused. Write a forbid here, or ask whoever keeps {} to change it there.",
            above(layer),
            above(layer)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str, roles: &[&str]) -> Person {
        serde_json::from_value(serde_json::json!({ "id": id, "roles": roles })).unwrap()
    }

    #[test]
    fn a_persons_own_layer_is_theirs_alone_and_the_rest_the_servers() {
        let (sam, carol, erin) = (person("sam", &["owner"]), person("carol", &[]), person("erin", &["policy:group:finance"]));
        assert!(may_write(&carol, "user/carol"), "carol needed a role for her own rules");
        assert!(!may_write(&sam, "user/carol"), "the owner wrote carol's own rules");
        assert!(!may_write(&erin, "user/carol") && !may_write(&carol, "user/"));
        assert!(may_write(&sam, "org") && !may_write(&carol, "org") && !may_write(&erin, "org"));
        assert!(may_write(&erin, "group/finance") && !may_write(&erin, "group/staff") && may_write(&sam, "group/staff"));
        assert!(!may_write(&sam, "task/t1"), "the owner wrote a person's task layer");
        assert!(kept_by_the_server("org") && kept_by_the_server("group/staff"));
        assert!(!kept_by_the_server("user/carol") && !kept_by_the_server("task/t1") && !kept_by_the_server("group/"));
    }

    #[test]
    fn below_the_organisation_a_layer_only_tightens() {
        let permit = r#"permit(principal, action == Action::"execute", resource == Tool::"mail__send");"#;
        let forbid = r#"forbid(principal, action, resource == Tool::"mail__send");"#;
        assert!(tightens_only("org", permit).is_ok());
        for layer in ["group/staff", "user/carol", "task/t1"] {
            let why = tightens_only(layer, permit).expect_err(layer);
            assert!(why.contains("org") && why.contains(layer), "{why}");
            assert!(tightens_only(layer, forbid).is_ok(), "{layer} refused a tightening");
        }
        assert!(tightens_only("user/carol", &format!("{forbid}\n{permit}")).is_err(), "a permit beside a forbid went through");
    }
}

#[cfg(test)]
#[path = "policy_layers_tests.rs"]
mod route_tests;
