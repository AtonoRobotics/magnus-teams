//! A certificate browsers already trust for the admin console (admin
//! console spec, Access: "ACME DNS-01 under a domain the organisation
//! controls, or the organisation's own CA"; Operations › Network:
//! `POST /network/certificate/renew`). With `trusted_via = "tailscale"` the
//! console has the tailnet issue it, here through a stand-in for
//! `tailscale cert` whose CA stands for Let's Encrypt: a browser that
//! trusts only that CA opens the console by its hostname as soon as it is
//! issued, with no CA trusted by hand; a restart serves it at once without
//! asking again; when the tailnet will not issue it, the console still
//! starts on the organisation's own certificate and says why. An admin's
//! Renew has it issued again, served from the next connection; a failed
//! renewal keeps what is served, is in the audit log, and shows as an alert
//! naming tailscale's reason.

use magnus_invariants::*;
use magnus_scratch::admin::*;
use magnus_scratch::*;
use reqwest::StatusCode;
use serde_json::Value;

const OFF: &str = "500 Internal Server Error: your Tailscale account does not support getting TLS certs";

#[tokio::test(flavor = "multi_thread")]
async fn the_console_serves_the_tailnets_certificate_for_its_hostname_and_its_own_until_then() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let mine = format!("{}/32", lan_ip());
    let ts = FakeTailscale::new();
    let issued = ts.ready(HOST);
    let keep = tempfile::tempdir().unwrap();

    // Issued as the console starts: a browser trusting only the public CA
    // opens it, and is shown the certificate the tailnet issued.
    let c = launch(prepare(&app, &[&mine], &ts.config(keep.path()))).await;
    let seen = eventually("the trusted certificate served", PATIENCE, || async { presented(&c, ts.public_ca()).await.ok() }).await;
    assert_eq!(seen, issued, "the console presents another certificate than the one issued");
    assert_eq!(ts.asked(), 1);
    drop(c);

    // Started again, it serves what was issued, without asking for another.
    let c = launch(prepare(&app, &[&mine], &ts.config(keep.path()))).await;
    assert_eq!(presented(&c, ts.public_ca()).await, Ok(issued.clone()), "a restart does not serve the issued certificate at once");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(ts.asked(), 1, "a restart asked the tailnet again for a certificate far from its end");
    drop(c);

    // The tailnet will not issue one: the console starts all the same, on
    // the organisation's own certificate.
    let ts = FakeTailscale::new();
    ts.fail(OFF);
    let empty = tempfile::tempdir().unwrap();
    let c = launch(prepare(&app, &[&mine], &ts.config(empty.path()))).await;
    eventually("the tailnet asked", PATIENCE, || async { (ts.asked() > 0).then_some(()) }).await;
    assert!(presented(&c, ts.public_ca()).await.is_err(), "a certificate the tailnet never issued is trusted");
    assert!(presented(&c, c.ca.clone()).await.is_ok(), "the console does not serve the organisation's own certificate when the tailnet will not issue one");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_renews_the_consoles_certificate_and_sees_why_it_could_not_be_issued() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    local_user(&app, "Alice", PASSWORD, true);
    let mine = format!("{}/32", lan_ip());
    let ts = FakeTailscale::new();
    let first = ts.ready(HOST);
    let keep = tempfile::tempdir().unwrap();
    let more = ts.config(keep.path());
    let p = prepare(&app, &[&mine], &more);
    let url = p.url();
    let c = launch(p).await;
    eventually("the trusted certificate served", PATIENCE, || async { presented(&c, ts.public_ca()).await.ok() }).await;
    // An admin's browser trusts the public CA, as any does, and the
    // organisation's own: the hostname gets the tailnet's certificate now.
    let b = reqwest::Client::builder()
        .add_root_certificate(c.ca.clone())
        .add_root_certificate(ts.public_ca())
        .resolve(HOST, c.addr)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let sid = sign_in(&url, c.addr, "Alice", PASSWORD, "admin", false).await.unwrap_or_else(|e| panic!("{e}"));
    let me: Value = with_cookie(b.get(format!("{url}/api/v1/session")), &sid).send().await.unwrap().json().await.unwrap();
    let csrf = me["csrf"].as_str().unwrap().to_string();
    let get = |path: &str| {
        let rq = with_cookie(b.get(format!("{url}/api/v1{path}")), &sid);
        async move { rq.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let renew = || {
        let rq = with_cookie(b.post(format!("{url}/api/v1/network/certificate/renew")), &sid).header("x-csrf-token", &csrf);
        async move {
            let r = rq.send().await.unwrap();
            (r.status(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };

    let n = get("/network").await;
    assert_eq!((n["trusted"]["via"].as_str(), n["trusted"]["serving"].as_bool()), (Some("tailscale"), Some(true)), "{n}");
    let t = n["certificates"].as_array().unwrap().iter().find(|x| x["what"] == "The admin console's trusted certificate").cloned().unwrap();
    assert!(t["days_left"].as_i64().is_some(), "{t}");

    // The tailnet has HTTPS turned off: Renew fails with its reason, and
    // what was served still is.
    ts.fail(OFF);
    let (st, v) = renew().await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "{v}");
    assert!(v["message"].as_str().unwrap().contains("does not support getting TLS certs"), "tailscale's reason is not given: {v}");
    assert_eq!(presented(&c, ts.public_ca()).await, Ok(first.clone()), "a failed renewal changed what is served");
    let n = get("/network").await;
    assert!(n["trusted"]["error"].as_str().is_some_and(|e| e.contains("does not support getting TLS certs")), "{n}");
    let o = get("/overview").await;
    let alert = o["alerts"].as_array().unwrap().iter().find(|a| a["what"].as_str().is_some_and(|w| w.contains("trusted certificate could not be issued"))).cloned();
    assert_eq!(alert.as_ref().map(|a| a["fix"].clone()), Some("#/operations/network".into()), "{o}");

    // Turned on again: Renew has a new one issued, served from the next
    // connection, and the alert is gone.
    let second = ts.ready(HOST);
    let (st, v) = renew().await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!((v["trusted"]["serving"].as_bool(), v["trusted"]["error"].is_null()), (Some(true), true), "{v}");
    assert_eq!(presented(&c, ts.public_ca()).await, Ok(second), "the renewed certificate is not served");
    let o = get("/overview").await;
    assert!(o["alerts"].as_array().unwrap().iter().all(|a| !a["what"].as_str().unwrap_or_default().contains("trusted certificate")), "{o}");

    // Both are in the audit log.
    let audit = get("/audit").await;
    let actions: Vec<&str> = audit["entries"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    for a in ["certificate_renewed", "certificate_renewed_failed"] {
        assert!(actions.contains(&a), "{a} is not in the audit: {actions:?}");
    }

    // The screen, in every look and at phone width.
    sign_in_and_visit(&url, c.addr, "Alice", PASSWORD, "admin", false, Some("#/operations/network")).await.unwrap_or_else(|e| panic!("{e}"));
}
