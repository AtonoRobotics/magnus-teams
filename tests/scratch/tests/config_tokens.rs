//! The config's long-lived tokens (the owner's scripts, the evaluation
//! runner) work only on the appliance's plain listener, from this machine;
//! over HTTPS, where the LAN and a tunnel reach it, they are refused, and
//! people sign in instead (owner's ruling 13, 2026-11-01).

use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread")]
async fn config_tokens_work_only_on_this_machines_plain_listener() {
    let m = FakeModel::start(steps(vec![])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    // HTTPS on a port the appliance picks and logs, as `Flow::with_tls`.
    magnus_scratch::reconfigure(&mut app, |c| {
        c["appliance"].as_table_mut().unwrap().insert("tls_listen".into(), "127.0.0.1:0".into());
    })
    .await;
    let https = eventually("the HTTPS address in the log", PATIENCE, || {
        let logs = app.logs();
        async move {
            let line = logs.lines().rev().find(|l| l.contains("listening on https://"))?;
            Some(format!("https://{}", line.split("listening on https://").nth(1)?.split(',').next()?))
        }
    })
    .await;
    let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let me = |base: String, token: String| {
        let client = client.clone();
        async move {
            let r = client.get(format!("{base}/v1/me")).bearer_auth(token).send().await.unwrap();
            let st = r.status().as_u16();
            (st, r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let key = app.spec.token("alice");

    // On this machine's plain listener the key works.
    assert_eq!(me(app.base.clone(), key.clone()).await.0, 200);
    // Over HTTPS it does not, even from this machine.
    let (st, v) = me(https.clone(), key.clone()).await;
    assert_eq!(st, 401, "the config's token worked over HTTPS: {v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("only on the server itself"), "{v}");

    // A person signed in over HTTPS uses their session there as before.
    assert!(set_password(&app, "carol", PASSWORD));
    let r =
        client.post(format!("{https}/v1/sign-in")).header("x-magnus-app", "0.5.0").json(&json!({ "person": "carol", "password": PASSWORD, "client": "app" })).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let session = r.json::<Value>().await.unwrap()["token"].as_str().unwrap().to_string();
    assert_eq!(me(https, session).await.0, 200, "a session no longer works over HTTPS");
}
