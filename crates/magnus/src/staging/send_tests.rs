//! A staged send, through the module's interface: what Send does with it,
//! and what is never staged at all.

use super::{GrantScope, Resolution};
use crate::admin_app::tests::{server_with, Server};
use crate::apps::{AppTool, CallOutput};
use crate::boundary::{Action, Issuer, Verdict};
use crate::transport::fake::Fake;
use crate::transport::Kind;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;

/// The pipeline's futures are deep: the server runs them on worker stacks
/// of this size (`cli.rs`), and so do these tests.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn on_big_stacks(f: impl std::future::Future<Output = ()> + Send + 'static) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(WORKER_STACK_BYTES).build().unwrap();
    rt.block_on(async { tokio::spawn(f).await.unwrap() })
}

fn tool(name: &str) -> AppTool {
    AppTool { name: name.into(), description: format!("{name} mail."), input_schema: json!({ "type": "object" }), annotations: json!({}) }
}

/// A mail app whose drafts are read at the gate, as Gmail's are, and whose
/// sends are read back. What it reads is untrusted and private.
async fn mail() -> (Server, Arc<Fake>) {
    let fake = Fake { tools: ["read", "get_draft", "get_sent", "send_draft"].map(tool).to_vec(), ..Fake::new(Kind::Brokered) };
    let (url, fake) = fake.install();
    let app = format!(
        "[[connectors]]\nname = \"mail\"\nurl = \"{url}\"\naudience = \"private\"\nlabel = \"untrusted+private\"\nidempotency = \"header\"\n\
         [connectors.tools.read]\nrisk = \"read\"\n[connectors.tools.get_draft]\nrisk = \"read\"\n[connectors.tools.get_sent]\nrisk = \"read\"\n\
         [connectors.tools.send_draft]\nrisk = \"irreversible\"\n[connectors.read_back.send_draft]\nread = \"get_sent\"\n\
         [connectors.destinations.send_draft]\nread = \"get_draft\"\nid = \"draft_id\"\nrecipients = [\"/message/to\", \"/message/bcc\"]\ncontent = [\"/message/body\"]\n"
    );
    (server_with(&app, |_| Router::new()).await, fake)
}

/// mo's running run, its task created and left to the test.
async fn run(s: &Server) -> String {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    let created = json!({ "kind": "task_created", "task": "t_s", "run": "r_s", "person": "mo", "config": config });
    s.k.emit("person:mo", serde_json::from_value(created).unwrap()).await.unwrap();
    "r_s".into()
}

fn action(run: &str, n: u32, tool: &str, input: Value) -> Action {
    Action { run: run.into(), call_id: format!("c{n}"), step_id: format!("s{n}.0"), tool: tool.into(), input, issuer: Issuer::Model }
}

/// The draft d1 as the app answers a read of it.
fn draft(bcc: &str) -> CallOutput {
    let d = json!({ "draft_id": "d1", "message": { "to": "bob@corp.example", "bcc": bcc, "body": "Confidential: revenue 41.7M" } });
    CallOutput { text: d.to_string(), structured: Some(d), is_error: false }
}

/// Queue the app's next answers.
fn answers(fake: &Fake, next: impl IntoIterator<Item = CallOutput>) {
    fake.answers.lock().unwrap().extend(next.into_iter().map(Ok));
}

fn sends(fake: &Fake) -> usize {
    fake.sent().iter().filter(|c| c.tool == "send_draft").count()
}

/// The run has read untrusted, private mail; then the model sends draft d1,
/// which goes to no one the person named: it is staged, and the run ends
/// `how`. The run and the id staged.
async fn staged_send(s: &Server, fake: &Fake, how: &str) -> (String, String) {
    let r = run(s).await;
    s.k.execute(action(&r, 1, "mail__read", json!({ "id": "m1" }))).await.unwrap();
    answers(fake, [draft("")]);
    let v = s.k.execute(action(&r, 2, "mail__send_draft", json!({ "draft_id": "d1" }))).await.unwrap();
    let Verdict::Done { text, .. } = v else { panic!("the send waited instead of being staged") };
    assert!(!text.contains("saved as a draft") && text.contains("held for the person"), "the model was not told plainly what became of it: {text}");
    assert_eq!(sends(fake), 0, "the send went out before the person sent it");
    let open = s.k.read(|st| st.staged.open_for("mo").into_iter().map(|i| i.id.clone()).collect::<Vec<_>>());
    assert_eq!(open.len(), 1, "the send was not staged");
    // The run goes on without it and ends; the person sends it later.
    end(s, &r, how).await;
    (r, open[0].clone())
}

/// End run `r` as `how` (`completed`, `cancelled`).
async fn end(s: &Server, r: &str, how: &str) {
    let ended = json!({ "kind": how, "run": r, "summary": "Done." });
    s.k.emit("kernel", serde_json::from_value(ended).unwrap()).await.unwrap();
}

/// Send performs the staged action once, through the gate, after its run
/// ended: an intent under its key, a done result (its receipt), and a
/// read-back. Sending it again does nothing. Before, Send recorded the item
/// resolved and nothing ran.
#[test]
fn sending_a_staged_item_performs_it_once_with_a_receipt() {
    on_big_stacks(async {
        let (s, fake) = mail().await;
        let (r, id) = staged_send(&s, &fake, "completed").await;
        // Read again before it goes, then by the gate itself.
        answers(&fake, [draft(""), draft("")]);
        let sent = s.k.resolve_staged(&id, Resolution::Sent, "person:mo", Some(GrantScope::Once)).await.unwrap();
        assert_eq!(sent["sent"], true, "{sent}");
        assert_eq!(sends(&fake), 1, "the staged send did not run once");
        let receipt = s.k.read(|st| st.runs[&r].intents.values().find(|i| i.tool == "mail__send_draft" && i.issuer == "model").and_then(|i| i.result.clone()));
        assert!(receipt.is_some_and(|res| res.status == "done" && !res.is_error), "the send has no receipt");
        assert!(s.k.read(|st| !st.evidence.read_backs(&r).is_empty()), "the send was not read back");
        assert_eq!(s.k.read(|st| st.staged.get(&id).and_then(|i| i.resolution)), Some(Resolution::Sent));

        answers(&fake, [draft(""), draft("")]);
        let again = s.k.resolve_staged(&id, Resolution::Sent, "person:mo", Some(GrantScope::Once)).await.unwrap();
        assert_eq!(again["sent"], false, "{again}");
        assert_eq!(sends(&fake), 1, "a second Send sent it again");
        s.done().await;
    });
}

/// A draft changed after it was staged (one added Bcc) is not sent on the
/// Send of the one shown: it is staged again, showing where it goes now,
/// and that one sends.
#[test]
fn a_staged_draft_whose_recipients_changed_is_not_sent() {
    on_big_stacks(async {
        let (s, fake) = mail().await;
        let (_, id) = staged_send(&s, &fake, "completed").await;
        answers(&fake, [draft("eve@evil.example")]);
        let out = s.k.resolve_staged(&id, Resolution::Sent, "person:mo", Some(GrantScope::Once)).await.unwrap();
        assert_eq!(out["sent"], false, "{out}");
        assert_eq!(sends(&fake), 0, "the changed draft was sent on the Send of the one shown");
        assert_eq!(s.k.read(|st| st.staged.get(&id).and_then(|i| i.resolution)), Some(Resolution::Edited));
        let again = out["staged_again"].as_str().expect("staged again").to_string();
        let shown = s.k.read(|st| st.staged.get(&again).map(|i| json!(i.shown)));
        assert!(shown.is_some_and(|v| v.to_string().contains("eve@evil.example")), "the new item does not show the added Bcc");

        answers(&fake, [draft("eve@evil.example"), draft("eve@evil.example")]);
        let sent = s.k.resolve_staged(&again, Resolution::Sent, "person:mo", Some(GrantScope::Once)).await.unwrap();
        assert_eq!(sent["sent"], true, "{sent}");
        assert_eq!(sends(&fake), 1);
        s.done().await;
    });
}

/// A staged item of a task that was cancelled does not go out on Send.
#[test]
fn a_staged_send_of_a_cancelled_task_is_not_sent() {
    on_big_stacks(async {
        let (s, fake) = mail().await;
        let (_, id) = staged_send(&s, &fake, "cancelled").await;
        answers(&fake, [draft(""), draft("")]);
        let out = s.k.resolve_staged(&id, Resolution::Sent, "person:mo", Some(GrantScope::Once)).await.unwrap();
        assert_eq!(out["sent"], false, "{out}");
        assert_eq!(sends(&fake), 0, "a cancelled task's staged send went out");
        s.done().await;
    });
}

/// After untrusted input with private data in hand, `tool` waits on case
/// 3's ask, and nothing is staged.
async fn asked_not_staged(tool: &str, input: Value) {
    let (s, _fake) = mail().await;
    let r = run(&s).await;
    s.k.execute(action(&r, 1, "mail__read", json!({ "id": "m1" }))).await.unwrap();
    let v = s.k.execute(action(&r, 2, tool, input)).await.unwrap();
    if let Verdict::Done { text, .. } = &v {
        panic!("{tool} did not wait for the person: {text}");
    }
    assert!(s.k.read(|st| st.staged.open_for("mo").is_empty()), "{tool} was staged");
    let rule = s.k.read(|st| st.inbox.open_for_run(&r).find(|i| i.kind == "approval").map(|i| i.details["rule"].clone()));
    assert_eq!(rule, Some(json!(crate::asks::NEW_DESTINATION)), "{tool} was not asked about as case 3");
    s.done().await;
}

/// A sign-in is not a message: it gets case 3's ask, as before staging,
/// never a staged item no one can carry out, nor a refusal as public
/// browsing.
#[test]
fn a_sign_in_after_untrusted_input_is_asked_not_staged() {
    on_big_stacks(asked_not_staged("browser_sign_in", json!({ "identity": "idn_1", "password_field": "#password" })));
}

/// Filling a form in a signed-in browser is not a message either.
#[test]
fn a_signed_in_form_after_untrusted_input_is_asked_not_staged() {
    on_big_stacks(asked_not_staged("browser_as_type", json!({ "identity": "idn_1", "target": "#card", "text": "4242", "submit": true })));
}
