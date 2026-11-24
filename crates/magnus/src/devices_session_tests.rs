//! A session works only from its own device (2026-11-23): through the
//! tunnel, the paired computer a request presents must be the session
//! person's, and the session's own when it was bound to one. Against a
//! kernel booted on a fresh database (`admin_app::tests`), with device
//! certificates configured.

use super::CERT_HEADER;
use crate::admin_app::tests::{server_with, Server};
use axum::Router;

const CERTS: &str = "[client_certs]\nhostname = \"magnus.example\"\nzone_id = \"zone\"\n";

/// A request for `/v1/me` as cloudflared forwards it, with `fp` as the
/// certificate the edge saw; its status.
async fn me_through_the_tunnel(s: &Server, token: &str, fp: &str) -> u16 {
    let r = reqwest::Client::new().get(format!("{}/v1/me", s.base)).bearer_auth(token).header("cf-connecting-ip", "203.0.113.9").header(CERT_HEADER, fp);
    r.send().await.unwrap().status().as_u16()
}

/// The same request from the LAN, where no certificate is asked for.
async fn me_on_the_lan(s: &Server, token: &str, fp: &str) -> u16 {
    let r = reqwest::Client::new().get(format!("{}/v1/me", s.base)).bearer_auth(token).header(CERT_HEADER, fp);
    r.send().await.unwrap().status().as_u16()
}

/// A session token copied off Mo's computer used to work from any paired
/// computer, Sam's included, and from Mo's other one after it was bound.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_works_through_the_tunnel_only_from_its_own_device() {
    let s = server_with(CERTS, |_| Router::new()).await;
    let (mo_a, mo_b, sams) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
    let device_a = s.k.devices.paired_for_test("mo", &mo_a);
    s.k.devices.paired_for_test("mo", &mo_b);
    s.k.devices.paired_for_test("sam", &sams);
    let token = s.token("mo").await;

    assert_eq!(me_through_the_tunnel(&s, &token, &sams).await, 401, "Mo's session passed with Sam's computer");
    assert_eq!(me_through_the_tunnel(&s, &token, &mo_a).await, 200, "Mo's session was refused on Mo's computer");
    assert_eq!(me_through_the_tunnel(&s, &token, &"d".repeat(64)).await, 401, "an unpaired certificate passed");

    // Bound to computer A, the session works from A alone.
    s.k.accounts.bind_session_device(&token, &device_a).await.unwrap();
    assert_eq!(me_through_the_tunnel(&s, &token, &mo_b).await, 401, "a session bound to A passed with Mo's other computer");
    assert_eq!(me_through_the_tunnel(&s, &token, &mo_a).await, 200, "a session bound to A was refused on A");

    // The LAN keeps what `admit` says: no certificate is asked for there.
    assert_eq!(me_on_the_lan(&s, &token, &sams).await, 200, "the LAN was held to the tunnel's device rule");
    s.done().await;
}

#[test]
fn a_device_holds_for_its_own_persons_session_and_its_own_bound_one() {
    let d = super::Device {
        id: "dev_a".into(),
        person: "Mo".into(),
        name: "laptop".into(),
        cloudflare_id: String::new(),
        fingerprint: "aa".into(),
        cloudflare_fingerprint: String::new(),
        serial: String::new(),
        created: chrono::Utc::now(),
        revoked: None,
    };
    assert!(super::session_holds(&d, "mo", None).is_ok());
    assert!(super::session_holds(&d, "mo", Some("dev_a")).is_ok());
    assert!(super::session_holds(&d, "sam", None).is_err());
    assert!(super::session_holds(&d, "sam", Some("dev_a")).is_err(), "someone else's session bound to this computer");
    assert!(super::session_holds(&d, "mo", Some("dev_b")).is_err());
}
