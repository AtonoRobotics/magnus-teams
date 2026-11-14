//! Every question the server asks of a person's roles, over a table of
//! people, without Postgres: each row names the questions that answer yes,
//! and every other answers no.
use super::*;
use crate::accounts::Role as Directory;
use crate::tools::Risk;

type Question = fn(&Person, Directory) -> bool;

/// Each question by name. The directory reaches only those that read it.
const QUESTIONS: &[(&str, Question)] = &[
    ("administers", |p, d| Authority::of(p, d).administers()),
    ("server_owner", |p, _| rights(p).is_server_owner()),
    ("guest_limits", |p, _| rights(p).held_to_guest_limits()),
    ("policy_admin", |p, d| rights(p).reads_as_admin(Authority::of(p, d))),
    ("developer_view", |p, _| rights(p).sees_developer_view()),
    ("approve_irreversible", |p, _| rights(p).may_approve(Risk::Irreversible)),
    ("approve_reversible", |p, _| rights(p).may_approve(Risk::ReversibleWrite)),
    ("endorse", |p, _| rights(p).may_endorse_knowledge()),
    ("layer_org", |p, _| rights(p).may_write_policy_layer("org")),
    ("layer_group_ops", |p, _| rights(p).may_write_policy_layer("group/ops")),
    ("layer_own", |p, _| rights(p).may_write_policy_layer(&format!("user/{}", p.id))),
    ("layer_other", |p, _| rights(p).may_write_policy_layer("user/someone-else")),
    ("channel_dev", |p, _| rights(p).own_update_channel() == Some("development")),
    ("channel_test", |p, _| rights(p).own_update_channel() == Some("test")),
    ("deletions", |p, _| rights(p).decides_deletion_requests()),
    ("app_tools", |p, _| rights(p).decides_app_tools()),
    ("moderates", |p, _| rights(p).moderates_shared_knowledge()),
    ("devices", |p, _| rights(p).manages_every_device()),
    ("experiments", |p, _| rights(p).judges_experiments()),
    ("shared_apps", |p, _| rights(p).manages_shared_apps()),
    ("every_model", |p, _| rights(p).uses_every_model()),
    ("owners", |p, _| rights(p).may_change_owners()),
    ("idp", |p, _| rights(p).may_choose_identity_provider()),
    ("team_admin_role", |p, _| rights(p).holds_team_admin_role()),
];

/// What every person may: their own policy layer, and a reversible write.
const ANYONE: &[&str] = &["layer_own", "approve_reversible"];

/// Everything the owner's role gives, whatever else they hold.
const OWNER: &[&str] = &[
    "administers",
    "server_owner",
    "approve_irreversible",
    "endorse",
    "layer_org",
    "layer_group_ops",
    "channel_dev",
    "deletions",
    "app_tools",
    "moderates",
    "devices",
    "experiments",
    "shared_apps",
    "every_model",
    "owners",
    "idp",
];

fn owner_and(more: &[&'static str]) -> Vec<&'static str> {
    [OWNER, more].concat()
}

fn person(roles: &[&str]) -> Person {
    Person { id: "p".into(), name: "P".into(), token: None, groups: vec![], roles: roles.iter().map(|r| r.to_string()).collect() }
}

#[test]
fn each_question_answers_from_the_roles_it_names() {
    let both = owner_and(&["policy_admin"]);
    let cases: &[(&[&str], Directory, &[&str])] = &[
        (&[], Directory::Member, &[]),
        (&["owner"], Directory::Member, OWNER),
        (&["owner", "admin"], Directory::Member, both.as_slice()),
        (&["owner", "guest"], Directory::Member, OWNER),
        // An admin administers and reads as one in a policy, and nothing an
        // owner keeps comes with it.
        (&["admin"], Directory::Member, &["administers", "policy_admin"]),
        (&[], Directory::Admin, &["administers", "policy_admin"]),
        // A guest made an admin in the directory administers; their roles
        // here still hold them to a guest's limits, the safer reading.
        (&["guest"], Directory::Admin, &["administers", "policy_admin", "guest_limits"]),
        (&["guest"], Directory::Member, &["guest_limits"]),
        (&["guest", "admin"], Directory::Member, &["administers", "policy_admin"]),
        (&["developer"], Directory::Member, &["developer_view"]),
        (&["tester"], Directory::Member, &["channel_test"]),
        (&["owner", "tester"], Directory::Member, OWNER),
        (&["approver:irreversible"], Directory::Member, &["approve_irreversible"]),
        (&["approver:reversible_write"], Directory::Member, &[]),
        (&["knowledge:endorse"], Directory::Member, &["endorse"]),
        (&["policy:group:ops"], Directory::Member, &["layer_group_ops"]),
        (&["policy:group:sales"], Directory::Member, &[]),
        (&["team-admin"], Directory::Member, &["team_admin_role"]),
        // A role is a whole name: nothing near it counts.
        (&["Owner", "owners", "admin "], Directory::Member, &[]),
    ];
    for (roles, directory, yes) in cases {
        let p = person(roles);
        for (name, ask) in QUESTIONS {
            let want = yes.contains(name) || ANYONE.contains(name);
            assert_eq!(ask(&p, *directory), want, "{name} for {roles:?} with {directory:?}");
        }
    }
}

#[test]
fn a_policy_layer_is_never_an_empty_or_unknown_name() {
    let owner = person(&["owner", "policy:group:"]);
    for layer in ["user/", "group/", "", "task/t1", "orgs", "user/p/x"] {
        assert!(!rights(&owner).may_write_policy_layer(layer), "{layer:?}");
    }
}
