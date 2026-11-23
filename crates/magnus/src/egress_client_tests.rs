//! The one client for addresses someone else chose: a private address is
//! refused unless private networks are allowed, a GET's every hop is
//! checked, and a request with a body follows no redirect.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub(crate) const WAIT: Duration = Duration::from_secs(5);
/// A loopback address a test takes as a public server's (`with_stand_in`).
pub(crate) const STAND_IN: &str = "127.0.0.2";

pub(crate) fn stand_in() -> Egress {
    Egress::with_stand_in(STAND_IN.parse().unwrap(), WAIT)
}

/// A server at `ip` that counts the requests reaching it and answers each
/// with `status`, redirecting to `to` when given.
pub(crate) async fn counting(ip: &str, status: u16, to: Option<String>) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let app = axum::Router::new().fallback(move || {
        let (h, to) = (h.clone(), to.clone());
        async move {
            h.fetch_add(1, Ordering::SeqCst);
            let mut r = axum::response::Response::builder().status(status).header("content-type", "application/json");
            if let Some(to) = to {
                r = r.header("location", to);
            }
            r.body(axum::body::Body::from("{}")).unwrap()
        }
    });
    let l = tokio::net::TcpListener::bind((ip, 0)).await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (base, hits)
}

/// `http://localhost:<port>` for a server at `base` on 127.0.0.1: a name
/// that resolves to a loopback address.
pub(crate) fn by_name(base: &str) -> String {
    format!("http://localhost:{}", base.rsplit(':').next().unwrap())
}

#[tokio::test]
async fn a_name_resolving_to_loopback_is_refused_unless_private_networks_are_allowed() {
    let (base, hits) = counting("127.0.0.1", 200, None).await;
    let refusing = Egress::new(false, WAIT, WAIT);
    let e = refusing.get(&format!("{}/", by_name(&base)), |r| r).await.expect_err("a GET reached a loopback address");
    assert!(format!("{e:#}").contains("private address"), "{e:#}");
    assert!(refusing.send(reqwest::Method::POST, &format!("{base}/"), |r| r.body("secret")).await.is_err(), "a POST reached a loopback address");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let ok = Egress::new(true, WAIT, WAIT).get(&format!("{base}/"), |r| r).await.expect("private networks are allowed");
    assert_eq!((ok.status().as_u16(), hits.load(Ordering::SeqCst)), (200, 1));
}

#[tokio::test]
async fn a_get_checks_every_hop_and_refuses_one_to_a_private_address() {
    let (private, private_hits) = counting("127.0.0.1", 200, None).await;
    let (hop, hop_hits) = counting(STAND_IN, 302, Some(format!("{private}/metadata"))).await;
    let e = stand_in().get(&format!("{hop}/start"), |r| r).await.expect_err("a redirect to a private address was followed");
    assert!(format!("{e:#}").contains("private address"), "{e:#}");
    assert_eq!((hop_hits.load(Ordering::SeqCst), private_hits.load(Ordering::SeqCst)), (1, 0));
    // A hop to another public address is followed.
    let (public, public_hits) = counting(STAND_IN, 200, None).await;
    let (hop, _) = counting(STAND_IN, 302, Some(format!("{public}/metadata"))).await;
    let ok = stand_in().get(&format!("{hop}/start"), |r| r).await.expect("a public hop");
    assert_eq!((ok.status().as_u16(), public_hits.load(Ordering::SeqCst)), (200, 1));
}

#[tokio::test]
async fn a_send_follows_no_redirect_even_where_private_networks_are_allowed() {
    let (private, private_hits) = counting("127.0.0.1", 200, None).await;
    let (hop, _) = counting(STAND_IN, 307, Some(format!("{private}/token"))).await;
    for e in [stand_in(), Egress::new(true, WAIT, WAIT)] {
        let r = e.send(reqwest::Method::POST, &format!("{hop}/token"), |r| r.body("refresh_token=rt")).await.unwrap();
        assert_eq!(r.status().as_u16(), 307);
    }
    assert_eq!(private_hits.load(Ordering::SeqCst), 0, "a POST was replayed to where its redirect pointed");
}

#[tokio::test]
async fn only_http_and_https_are_reached() {
    for e in [Egress::new(false, WAIT, WAIT), Egress::new(true, WAIT, WAIT)] {
        assert!(e.get("file:///etc/passwd", |r| r).await.is_err());
        assert!(e.send(reqwest::Method::POST, "ftp://example.com/", |r| r).await.is_err());
    }
}
