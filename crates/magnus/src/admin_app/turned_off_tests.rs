//! One "person is turned off" (decision
//! `2026-11-23-one-person-is-turned-off.md`), against a kernel booted on a
//! fresh database (the harness in `tests.rs`): sam owns the server, ana is
//! an admin by the config, lee by the directory, and mo is a member.

use super::spec_tests::stepped;
use super::tests::{server, Server, PASSWORD};
use crate::accounts::Status;
use axum::Router;
use serde_json::json;

/// A member of the server's own directory, with a password, a schedule due
/// and a paired device. The device's fingerprint.
async fn member(s: &Server, id: &str) -> String {
    if s.k.person(id).is_none() {
        s.k.accounts.add_person(id, id, None).await.unwrap();
        s.k.accounts.set_password(id, PASSWORD).await.unwrap();
    }
    let t = s.token(id).await;
    let (st, v) = s.post(&t, "/v1/schedules", json!({ "text": "Send the weekly summary", "daily_at": "09:00" })).await;
    assert!(st < 300, "{v}");
    let fp = format!("{:064x}", id.len() as u64 * 7919 + id.as_bytes()[0] as u64);
    s.k.devices.paired_for_test(id, &fp);
    assert!(s.k.devices.is_live(&fp));
    fp
}

/// Whether `id` is off every way: no schedule of theirs fires, no new work
/// starts for them, and their device is refused.
fn off(s: &Server, id: &str, fp: &str) {
    let due = s.k.read(|st| st.arrangements.schedules().values().filter(|x| x.person == id && x.next_due().is_some()).count());
    assert_eq!(due, 0, "{id}'s schedule still fires");
    assert!(s.k.refuse_new(id, false).is_some(), "new work starts for {id}");
    assert!(!s.k.devices.is_live(fp), "{id}'s device still passes the device gate");
}

/// Turning someone off through the PATCH wrote only their status: their
/// schedules went on running as them and their paired laptop still passed
/// the device gate. The PATCH, POST /disable and the command line (which
/// writes the store alone, and the kernel's watch catches) now turn them
/// off the same one way.
#[tokio::test(flavor = "multi_thread")]
async fn after_either_way_of_turning_off_no_schedule_fires_and_the_device_is_refused() {
    let s = server(|_| Router::new()).await;
    let ana = stepped(&s, "ana").await;
    let (mo, zoe, kit) = (member(&s, "mo").await, member(&s, "zoe").await, member(&s, "kit").await);
    assert_eq!(s.call(&ana, reqwest::Method::PATCH, "/v1/admin/people/mo", Some(json!({ "status": "disabled" }))).await.0, 204);
    off(&s, "mo", &mo);
    let (st, v) = s.post(&ana, "/v1/admin/people/zoe/disable", json!({})).await;
    assert_eq!(st, 200, "{v}");
    off(&s, "zoe", &zoe);
    s.k.accounts.set_status("kit", Status::Disabled).await.unwrap();
    assert_eq!(s.k.hold_disabled_accounts().await, 1, "the account turned off on the command line was not held");
    off(&s, "kit", &kit);
    assert_eq!(s.k.hold_disabled_accounts().await, 0, "someone already held was turned off again");
    assert!(s.k.refuse_new("lee", false).is_none(), "someone still on was held");
    s.done().await;
}
