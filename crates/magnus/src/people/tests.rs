//! The merge rule and the authority rule, without Postgres.
use super::*;
use crate::accounts::Role as Directory;
use std::collections::BTreeMap;

/// The server's own directory as a table.
struct Table(BTreeMap<String, LocalPerson>);

impl Local for Table {
    fn local_ids(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
    fn local(&self, id: &str) -> Option<LocalPerson> {
        self.0.get(id).cloned()
    }
}

fn person(id: &str, name: &str, roles: &[&str]) -> Person {
    Person { id: id.into(), name: name.into(), token: Some("t".into()), groups: vec!["staff".into()], roles: roles.iter().map(|r| r.to_string()).collect() }
}

fn table(rows: &[(&str, &str)]) -> Table {
    Table(rows.iter().map(|(id, name)| (id.to_string(), LocalPerson { name: name.to_string(), email: None })).collect())
}

/// sam and zed in the config (zed first), ada and an intruding sam local.
fn world() -> (Vec<Person>, Table) {
    (vec![person("zed", "Zed", &[]), person("sam", "Sam", &["owner"])], table(&[("sam", "Intruder"), ("ada", "Ada")]))
}

#[test]
fn the_config_wins_a_namesake_and_a_local_person_arrives_a_plain_member() {
    let (config, local) = world();
    // (id, resolves, name, owner, has a token or groups)
    let cases = [("sam", true, "Sam", true, true), ("zed", true, "Zed", false, true), ("ada", true, "Ada", false, false), ("ghost", false, "", false, false)];
    for (id, resolves, name, owner, powers) in cases {
        let p = resolve(&config, &local, id);
        assert_eq!(p.is_some(), resolves, "{id} resolved wrong");
        assert_eq!(exists(&config, &local, id), resolves, "{id} existed wrong");
        let Some(p) = p else { continue };
        assert_eq!(p.name, name, "{id} has the wrong record");
        assert_eq!(p.is_owner(), owner, "{id}'s ownership is wrong");
        assert_eq!(p.token.is_some() || !p.groups.is_empty(), powers, "{id} arrived with the wrong powers");
    }
}

#[test]
fn everyone_is_listed_once_in_order_and_owners_read_the_config_first() {
    let (config, local) = world();
    assert_eq!(ids(&config, &local), ["ada", "sam", "zed"], "the list is not everyone once, in order");
    assert_eq!(config_first(&config, &local), ["zed", "sam", "ada"], "the config's people did not come first, in its order");
    assert_eq!(ids(&[], &local), ["ada", "sam"], "a server with only its own people listed nobody");
}

#[test]
fn overlays_replace_roles_and_the_providers_beat_roles_set_here() {
    let base = || person("ada", "Ada", &["member"]);
    let roles = |r: &[&str]| Some(r.iter().map(|x| x.to_string()).collect::<Vec<_>>());
    // (overlay, roles after, groups after)
    let cases = [
        (Overlay::default(), vec!["member"], vec!["staff"]),
        (Overlay { roles: roles(&["owner"]), provider: None }, vec!["owner"], vec!["staff"]),
        (Overlay { roles: roles(&["owner"]), provider: Some((None, roles(&["guest"]))) }, vec!["guest"], vec!["staff"]),
        (Overlay { roles: roles(&["admin"]), provider: Some((roles(&["ops"]), None)) }, vec!["admin"], vec!["ops"]),
    ];
    for (o, want_roles, want_groups) in cases {
        let p = overlaid(base(), o.clone());
        assert_eq!(p.roles, want_roles, "{o:?} left the wrong roles");
        assert_eq!(p.groups, want_groups, "{o:?} left the wrong groups");
    }
}

#[test]
fn a_name_falls_back_to_the_id() {
    assert_eq!(shown_name(Some(person("ada", "Ada", &[])), "ada"), "Ada");
    assert_eq!(shown_name(Some(person("ada", "", &[])), "ada"), "ada");
    assert_eq!(shown_name(None, "ghost"), "ghost");
}

#[test]
fn a_password_is_everyones_without_a_provider_and_the_owners_with_one() {
    let (owner, member) = (person("sam", "Sam", &["owner"]), person("zed", "Zed", &[]));
    assert!(may_use_password(Some(&owner), true) && may_use_password(Some(&owner), false));
    assert!(may_use_password(Some(&member), false) && !may_use_password(Some(&member), true));
    assert!(!may_use_password(None, false), "nobody signed in with a password");
}

/// Either place that keeps "admin" makes an admin; an owner is an owner
/// whatever the directory says; a guest is one only without a role above.
#[test]
fn authority_reads_both_places_admin_is_kept() {
    let cases = [
        (&["owner", "guest"][..], Directory::Member, Authority::Owner),
        (&["owner"][..], Directory::Admin, Authority::Owner),
        (&["admin"][..], Directory::Member, Authority::Admin),
        (&[][..], Directory::Admin, Authority::Admin),
        (&["guest"][..], Directory::Admin, Authority::Admin),
        (&["guest"][..], Directory::Member, Authority::Guest),
        (&["developer"][..], Directory::Member, Authority::Member),
    ];
    for (roles, directory, want) in cases {
        let a = Authority::of(&person("p", "P", roles), directory);
        assert_eq!(a, want, "{roles:?} with {directory:?}");
        assert_eq!(a.administers(), want >= Authority::Admin);
    }
    assert_eq!([Authority::Owner, Authority::Admin, Authority::Member, Authority::Guest].map(Authority::as_str), ["owner", "admin", "member", "guest"]);
}
