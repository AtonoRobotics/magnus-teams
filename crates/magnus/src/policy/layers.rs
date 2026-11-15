//! Who writes which policy layer, and what a layer may say (the owner,
//! 2026-11-11: "The rule change should be allowed by inherited policy").
//!
//! The layers stand in a hierarchy: the kernel's floor, then the
//! organisation's layer (`org`), then a group's (`group/<g>`), then a
//! person's own (`user/<id>`), then a task's (`task/<t>`). Only `org` widens:
//! what it permits is open to every layer below, which may forbid within it
//! and nothing more (`policy::stack` adds a permit-all to each lower layer,
//! so its forbids alone count). A permit written below `org` therefore
//! could only try to loosen what a layer above holds, and it is refused when
//! it is written or accepted, naming the layers above it. The floor
//! (prohibited tools never run, every payment asks its person, Auto Review
//! on every boundary action, the rule of two, the hard denials) is compiled
//! into the kernel and in no layer.

use cedar_policy::{Effect, PolicySet};
use std::str::FromStr;

/// The longest layer name the admin's editor takes, in bytes: `group/` and
/// a hundred more.
pub const MAX_LAYER_NAME_BYTES: usize = 106;

/// One policy layer, by its name: `org`, `group/<g>`, `user/<id>` or
/// `task/<t>`. The one reading of a layer's name; each name below `org` is
/// non-empty and holds no `/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer<'a> {
    Org,
    Group(&'a str),
    User(&'a str),
    Task(&'a str),
}

impl<'a> Layer<'a> {
    /// The layer `name` names, or None when it names none.
    pub fn parse(name: &'a str) -> Option<Layer<'a>> {
        if name == "org" {
            return Some(Layer::Org);
        }
        let (kind, of) = name.split_once('/')?;
        if of.is_empty() || of.contains('/') {
            return None;
        }
        match kind {
            "group" => Some(Layer::Group(of)),
            "user" => Some(Layer::User(of)),
            "task" => Some(Layer::Task(of)),
            _ => None,
        }
    }

    /// A layer the server's owner and policy roles keep: the organisation's
    /// or a group's. A person's and a task's are not theirs to read.
    pub fn kept_by_the_server(self) -> bool {
        matches!(self, Layer::Org | Layer::Group(_))
    }

    /// The layers above this one, in words, for a refusal.
    pub fn above(self) -> &'static str {
        match self {
            Layer::Group(_) => "the organisation's layer (org)",
            Layer::User(_) => "the organisation's layer (org) and your groups' layers",
            Layer::Org | Layer::Task(_) => "the organisation's layer (org), the groups' layers and the person's own layer",
        }
    }

    /// Whether a run of `person` may propose a change to this layer: the
    /// organisation's, a group's, or its own person's. Another person's
    /// layer is theirs, and a proposal to it would put this run's words in
    /// their inbox; a task's is set by its person, not proposed.
    pub fn proposable_by(self, person: &str) -> bool {
        match self {
            Layer::Org | Layer::Group(_) => true,
            Layer::User(u) => u == person,
            Layer::Task(_) => false,
        }
    }
}

/// Whether `cedar`, written to `layer`, only tightens: below `org`, a
/// permit is refused with why. Text that does not parse is left to the
/// validator.
pub fn tightens_only(layer: &str, cedar: &str) -> Result<(), String> {
    let parsed = Layer::parse(layer);
    if parsed == Some(Layer::Org) {
        return Ok(());
    }
    let Ok(set) = PolicySet::from_str(cedar) else { return Ok(()) };
    let permit = set.policies().find(|p| p.effect() == Effect::Permit).map(|p| p.id().to_string());
    // A name that is no layer has every layer above it.
    let above = parsed.unwrap_or(Layer::Task("")).above();
    match permit {
        None => Ok(()),
        Some(id) => Err(format!(
            "{layer} can only tighten what the layers above it allow: the permit {id} would loosen what {above} holds, so it is refused. Write a forbid here, or ask whoever keeps {above} to change it there."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Person;

    fn may_write_policy(p: &Person, layer: &str) -> bool {
        crate::people::rights(p).may_write_policy_layer(layer)
    }

    fn person(id: &str, roles: &[&str]) -> Person {
        serde_json::from_value(serde_json::json!({ "id": id, "roles": roles })).unwrap()
    }

    #[test]
    fn a_persons_own_layer_is_theirs_alone_and_the_rest_the_servers() {
        let (sam, carol, erin) = (person("sam", &["owner"]), person("carol", &[]), person("erin", &["policy:group:finance"]));
        assert!(may_write_policy(&carol, "user/carol"), "carol needed a role for her own rules");
        assert!(!may_write_policy(&sam, "user/carol"), "the owner wrote carol's own rules");
        assert!(!may_write_policy(&erin, "user/carol") && !may_write_policy(&carol, "user/"));
        assert!(may_write_policy(&sam, "org") && !may_write_policy(&carol, "org") && !may_write_policy(&erin, "org"));
        assert!(may_write_policy(&erin, "group/finance") && !may_write_policy(&erin, "group/staff") && may_write_policy(&sam, "group/staff"));
        assert!(!may_write_policy(&sam, "task/t1"), "the owner wrote a person's task layer");
        let kept = |l: &str| Layer::parse(l).is_some_and(Layer::kept_by_the_server);
        assert!(kept("org") && kept("group/staff"));
        assert!(!kept("user/carol") && !kept("task/t1") && !kept("group/"));
    }

    /// A layer's name is read one way: each kind, a non-empty name with no
    /// `/`, and nothing else.
    #[test]
    fn a_layers_name_is_read_one_way() {
        assert_eq!(Layer::parse("org"), Some(Layer::Org));
        assert_eq!(Layer::parse("group/staff"), Some(Layer::Group("staff")));
        assert_eq!(Layer::parse("user/carol"), Some(Layer::User("carol")));
        assert_eq!(Layer::parse("task/t1"), Some(Layer::Task("t1")));
        for no in ["", "org/", "orgs", "group/", "group/a/b", "user/", "user/a/b", "task/", "team/x", "/org", "Group/staff"] {
            assert_eq!(Layer::parse(no), None, "{no:?} read as a layer");
        }
        let mo = |l: &str| Layer::parse(l).is_some_and(|l| l.proposable_by("mo"));
        assert!(mo("user/mo") && mo("org") && mo("group/staff"));
        assert!(!mo("user/sam"), "mo's agent proposed to sam's own rules");
        assert!(!mo("user/") && !mo("task/t1") && !mo("group/") && !mo("group/a/b"));
        assert_eq!(Layer::Group("g").above(), "the organisation's layer (org)");
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
#[path = "layers_route_tests.rs"]
mod route_tests;
