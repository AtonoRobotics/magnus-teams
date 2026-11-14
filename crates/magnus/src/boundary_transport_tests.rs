//! The boundary's connector calls over the transport seam, against a fake
//! adapter: what each caller sends, with which key, behind which fence,
//! with no app server or microVM.

use super::{Action, CallFailure, Facts, Issuer};
use crate::admin_app::tests::{server_with, Server};
use crate::apps::CallError;
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::NewRun;
use crate::tools::{Audience, Label, Risk};
use crate::transport::fake::{Fake, Sent};
use crate::transport::Kind;
use axum::Router;
use serde_json::json;
use std::sync::Arc;

fn connector(url: &str, idempotency: &str) -> Connector {
    serde_json::from_value(json!({ "name": "mail", "url": url, "audience": "private", "label": "private", "idempotency": idempotency })).unwrap()
}

fn facts(c: Connector, risk: Risk) -> Facts {
    Facts {
        name: "mail__send".into(),
        app: "mail".into(),
        risk,
        label: Label::PRINCIPAL,
        audience: Audience::Private,
        connector: Some(c),
        remote: "send".into(),
        reaches_self: false,
    }
}

/// mo's running run, and an action of it.
async fn action(s: &Server) -> Action {
    let (_, run) = s.k.create_run("person:mo", "mo", "send the note".into(), vec![], NewRun::default()).await.unwrap();
    Action { run, call_id: "c1".into(), step_id: "s1".into(), tool: "mail__send".into(), input: json!({ "to": "a@example.com" }), issuer: Issuer::Model }
}

fn sent(key: Option<&str>) -> Sent {
    Sent { person: "mo".into(), bearer: Some("token".into()), tool: "send".into(), args: json!({ "to": "a@example.com" }), key: key.map(String::from) }
}

/// An agent's call goes over the connector's transport with the person's
/// sign-in and the action's idempotency key when the app honours one, and
/// with none when it does not; the answer is the transport's.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_goes_over_its_transport_with_its_key_when_the_app_takes_one() {
    let s = server_with("", |_| Router::new()).await;
    let (url, fake) = Fake::new(Kind::McpHttp).install();
    let a = action(&s).await;
    for idempotency in ["header", "none"] {
        let f = facts(connector(&url, idempotency), Risk::ReversibleWrite);
        let stops = s.k.brokered.generation("mail", "mo");
        let out = s.k.call(&a, &f, Some("token"), "key-1", stops).await;
        assert!(matches!(&out, Ok((text, false, None)) if text == "done"), "{idempotency}");
    }
    assert_eq!(fake.sent(), vec![sent(Some("key-1")), sent(None)]);
    s.done().await;
}

/// A run cancelled while its call waits to send (a brokered call's boot)
/// sends nothing: the fence the call path passes reads the run live.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_stopped_while_its_call_waits_sends_nothing() {
    let s = server_with("", |_| Router::new()).await;
    let (waiting, go) = (Arc::new(tokio::sync::Notify::new()), Arc::new(tokio::sync::Notify::new()));
    let (url, fake) = Fake { hold: Some((waiting.clone(), go.clone())), ..Fake::new(Kind::Brokered) }.install();
    let a = action(&s).await;
    let f = facts(connector(&url, "none"), Risk::ReversibleWrite);
    let k = s.k.clone();
    let a2 = Action { run: a.run.clone(), call_id: a.call_id.clone(), step_id: a.step_id.clone(), tool: a.tool.clone(), input: a.input.clone(), issuer: a.issuer };
    let stops = k.brokered.generation("mail", "mo");
    let call = tokio::spawn(async move { k.call(&a2, &f, Some("token"), "key-1", stops).await.map(|_| ()) });
    waiting.notified().await;
    s.k.emit("person:mo", Ev::Cancelled { run: a.run.clone(), source: Some("button".into()), turn: None }).await.unwrap();
    go.notify_one();
    let out = call.await.unwrap();
    assert!(matches!(&out, Err(CallFailure::Stopped(m)) if m.contains("Nothing was sent to mail")), "the call was not stopped");
    assert!(fake.sent().is_empty(), "a cancelled run's call was sent");
    s.done().await;
}

/// The gate's read of a connector is fenced as any call is (here by the
/// kill switch) though it has no run of its own, and carries no key even
/// to an app that takes one: a read has nothing to deduplicate.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_is_fenced_without_a_run_and_carries_no_key() {
    let s = server_with("", |_| Router::new()).await;
    let (url, fake) = Fake::new(Kind::Brokered).install();
    let c = connector(&url, "header");
    let args = json!({ "to": "a@example.com" });
    s.k.emit("person:sam", Ev::KillSwitch { engaged: true }).await.unwrap();
    let out = s.k.connector_call(&c, "mo", "mail", Some("token"), "send", &args).await;
    assert!(matches!(&out, Err(CallError::NotSent(m)) if m.contains("kill switch")), "a read went past the kill switch");
    assert!(fake.sent().is_empty());
    s.k.emit("person:sam", Ev::KillSwitch { engaged: false }).await.unwrap();
    assert!(s.k.connector_call(&c, "mo", "mail", Some("token"), "send", &args).await.is_ok());
    assert_eq!(fake.sent(), vec![sent(None)]);
    s.done().await;
}

/// An answer that breaks its schema is only unusable for a read, and
/// unknown for a write, which may have happened.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_breaks_its_schema_is_unusable_for_a_read_and_unknown_for_a_write() {
    let s = server_with("", |_| Router::new()).await;
    let fake = Fake::new(Kind::McpHttp);
    fake.answers.lock().unwrap().extend([Err(CallError::BadAnswer("read: bad".into())), Err(CallError::BadAnswer("write: bad".into()))]);
    let (url, _fake) = fake.install();
    let a = action(&s).await;
    let stops = s.k.brokered.generation("mail", "mo");
    let read = s.k.call(&a, &facts(connector(&url, "none"), Risk::Read), Some("token"), "k", stops).await;
    assert!(matches!(&read, Ok((m, true, None)) if m == "read: bad"));
    let write = s.k.call(&a, &facts(connector(&url, "none"), Risk::ReversibleWrite), Some("token"), "k", stops).await;
    assert!(matches!(&write, Err(CallFailure::Unknown(m)) if m == "write: bad"));
    s.done().await;
}
