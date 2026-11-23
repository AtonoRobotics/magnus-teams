//! The store's own rules: a refresh writes back only onto the sign-in it
//! read, an ending sign-in is given to no one, and each sign-in refreshes
//! on its own turn.

use super::{SignInOwner, SignInStore, Superseded};
use crate::vault::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

#[test]
fn an_ended_sign_in_is_not_written_back_by_an_earlier_refresh() {
    let (_d, v) = vault();
    let s = SignInStore::default();
    s.replace(&v, "n", "old").unwrap();
    let (at, _) = s.read(&v, "n").unwrap();
    assert_eq!(s.end(&v, "n").unwrap().map(|x| x.expose().to_string()), Some("old".into()));
    assert_eq!(s.write_back(&v, "n", at, |_| Ok("refreshed".into())).unwrap(), Err(Superseded::Ended));
    assert!(v.get("n").unwrap().is_none(), "a refresh wrote an ended sign-in back");
}

#[test]
fn a_new_sign_in_is_not_overwritten_by_an_earlier_refresh() {
    let (_d, v) = vault();
    let s = SignInStore::default();
    s.replace(&v, "n", "old").unwrap();
    let (at, _) = s.read(&v, "n").unwrap();
    s.replace(&v, "n", "new, with more access").unwrap();
    assert_eq!(s.write_back(&v, "n", at, |_| Ok("refreshed".into())).unwrap(), Err(Superseded::Replaced));
    assert_eq!(v.get("n").unwrap().unwrap().expose(), "new, with more access");
    let (now, _) = s.read(&v, "n").unwrap();
    assert_eq!(s.write_back(&v, "n", now, |was| Ok(format!("{was}, refreshed"))).unwrap(), Ok(()));
    assert_eq!(v.get("n").unwrap().unwrap().expose(), "new, with more access, refreshed");
}

#[test]
fn an_ending_sign_in_is_given_to_no_one_and_kept_when_the_app_refuses() {
    let (_d, v) = vault();
    let s = SignInStore::default();
    s.replace(&v, "n", "t").unwrap();
    let (at, _) = s.ending(&v, "n").unwrap().unwrap();
    assert!(s.get(&v, "n").unwrap().is_none(), "an ending sign-in was given out");
    s.kept("n", at);
    assert_eq!(s.get(&v, "n").unwrap().unwrap().expose(), "t");
    let (at, _) = s.ending(&v, "n").unwrap().unwrap();
    // A new sign-in made while the old one was being revoked stays.
    s.replace(&v, "n", "fresh").unwrap();
    s.ended(&v, "n", at).unwrap();
    assert_eq!(s.get(&v, "n").unwrap().unwrap().expose(), "fresh");
}

#[tokio::test]
async fn one_sign_ins_refresh_does_not_hold_up_another() {
    let s = SignInStore::default();
    let held = s.refreshing("a").await;
    let other = tokio::time::timeout(std::time::Duration::from_secs(1), s.refreshing("b")).await;
    assert!(other.is_ok(), "one sign-in's refresh held up another's");
    let same = tokio::time::timeout(std::time::Duration::from_millis(50), s.refreshing("a")).await;
    assert!(same.is_err(), "two refreshes of one sign-in ran at once");
    drop(held);
}

#[test]
fn the_owner_of_a_sign_in_is_read_from_its_name() {
    let person = crate::connectors::connection_name("crm", "mo");
    let project = crate::connectors::connection_name("crm", &crate::team::sign_in::owner_key("w1"));
    assert_eq!(SignInOwner::of_name(&person, "crm"), Some(SignInOwner::Person("mo".into())));
    assert_eq!(SignInOwner::of_name(&project, "crm"), Some(SignInOwner::Project("w1".into())));
    assert_eq!(SignInOwner::of_name(&crate::connectors::credential_name("crm"), "crm"), None);
    assert_eq!(SignInOwner::of_name(&person, "crm_x"), None);
    assert_eq!(SignInOwner::parse(&project), Some(("crm".into(), SignInOwner::Project("w1".into()))));
    assert_eq!(SignInOwner::Project("w1".into()).key(), crate::team::sign_in::owner_key("w1"));
}
