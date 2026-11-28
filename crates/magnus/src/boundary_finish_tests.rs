//! A tool's answer is finished the same whichever try produced it: the
//! first, or the resend after a first try whose outcome was lost.

use super::{Action, Issuer, Verdict};
use crate::admin_app::tests::{server_with, Server};
use crate::apps::{AppTool, CallError, CallOutput};
use crate::events::Ev;
use crate::tools::Label;
use crate::transport::fake::Fake;
use crate::transport::Kind;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;

const CODE: &str = "482913";
const BYTES: &str = "SEVMTE8gV09STEQ=";

fn tool(name: &str) -> AppTool {
    AppTool { name: name.into(), description: format!("{name} mail."), input_schema: json!({ "type": "object" }), annotations: json!({}) }
}

/// A mail app over a fake transport whose calls answer `answers` in turn.
/// It asks for its login codes to be taken out, which mail no longer does
/// by default (2026-11-28-gates-beyond-the-invariants).
async fn mail(answers: Vec<Result<CallOutput, CallError>>) -> (Server, Arc<Fake>) {
    let fake = Fake { tools: vec![tool("read"), tool("send"), tool("get_sent")], ..Fake::new(Kind::Brokered) };
    fake.answers.lock().unwrap().extend(answers);
    let (url, fake) = fake.install();
    let app = format!(
        "[[connectors]]\nname = \"mail\"\nurl = \"{url}\"\naudience = \"private\"\nlabel = \"untrusted+private\"\nidempotency = \"header\"\nfilter_login_codes = true\n\
         [connectors.tools.read]\nrisk = \"read\"\n[connectors.tools.get_sent]\nrisk = \"read\"\n[connectors.tools.send]\nrisk = \"irreversible\"\n\
         [connectors.read_back.send]\nread = \"get_sent\"\n"
    );
    (server_with(&app, |_| Router::new()).await, fake)
}

/// A message with a sign-in code in its body and a file attached.
fn message() -> CallOutput {
    let m = json!({ "messages": [{ "from": "bank@example.com", "body": format!("Your verification code is {CODE}"), "attachments": [{ "filename": "statement.pdf", "media_type": "application/pdf", "data_b64": BYTES }] }] });
    CallOutput { text: m.to_string(), structured: None, is_error: false }
}

/// mo's running run, created as its task is and left to the test: no
/// model loop wakes for it, so it stays running while the test acts for it.
async fn run(s: &Server) -> String {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 }, "verifier": false, "instructions": "", "harness": "" });
    let created = json!({ "kind": "task_created", "task": "t_f", "run": "r_f", "person": "mo", "config": config });
    s.k.emit("person:mo", serde_json::from_value(created).unwrap()).await.unwrap();
    "r_f".into()
}

fn action(run: &str, n: u32, tool: &str, input: Value) -> Action {
    Action { run: run.into(), call_id: format!("c{n}"), step_id: format!("s{n}.0"), tool: tool.into(), input, issuer: Issuer::Model }
}

fn done_text(v: &Verdict) -> Option<&str> {
    match v {
        Verdict::Done { text, .. } => Some(text),
        Verdict::Suspended => None,
    }
}

/// The label and sources the result of `call` was recorded with.
async fn recorded(s: &Server, call: &str) -> (Label, Vec<String>) {
    let events = s.k.log.load().await.unwrap();
    events
        .into_iter()
        .find_map(|e| match e.ev {
            Ev::ToolResult { call_id, label, sources, .. } if call_id == call => Some((label, sources)),
            _ => None,
        })
        .unwrap()
}

/// The pipeline's futures are deep: the server runs them on worker stacks
/// of this size (`cli.rs`), and so do these tests, the body included, which
/// the test thread's 2 MiB could not hold.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn on_big_stacks(f: impl std::future::Future<Output = ()> + Send + 'static) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(WORKER_STACK_BYTES).build().unwrap();
    rt.block_on(async { tokio::spawn(f).await.unwrap() })
}

/// A mail read whose first try's outcome is lost is sent again, and the
/// second answer reaches the model as a first try's does: no sign-in code,
/// the attachment held back by its reference, the same label and sources.
/// Before, the resend's answer was only masked: the model got the code and
/// the file's bytes, under the app's fixed label.
#[test]
fn a_resent_mail_read_is_finished_as_a_first_try_is() {
    on_big_stacks(async {
        let (s, fake) = mail(vec![Ok(message()), Err(CallError::Unknown("the connector's answer was lost".into())), Ok(message())]).await;
        let r = run(&s).await;
        let mut seen = vec![];
        for n in [1, 2] {
            let v = s.k.execute(action(&r, n, "mail__read", json!({ "id": "m1" }))).await.unwrap();
            let Verdict::Done { text, is_error: false, data } = v else { panic!("read {n} did not succeed: {:?}", done_text(&v)) };
            assert!(!text.contains(CODE), "read {n}: the model was given the sign-in code: {text}");
            assert!(!text.contains(BYTES), "read {n}: the model was given the attachment's bytes: {text}");
            assert!(text.contains("statement.pdf") && text.contains("open_attachment"), "read {n}: the attachment was not held by its reference: {text}");
            let kept = &data.unwrap()["outside_attachments"][0];
            assert!(kept.get("data_b64").is_none() && kept["sha256"].is_string(), "read {n}: {kept}");
            seen.push(recorded(&s, &format!("c{n}")).await);
        }
        assert_eq!(fake.sent().len(), 3, "the lost read was not sent again");
        assert_eq!(seen[0], seen[1], "the resend was labelled other than a first try");
        assert_eq!(seen[1], (Label { untrusted: true, private: true }, vec!["mail".to_string()]));
        s.done().await;
    });
}

/// An irreversible send whose first try's outcome is lost, resent under the
/// same idempotency key, is read back as one that went through first time.
/// Before, the resend was recorded as done with no read-back.
#[test]
fn a_resent_irreversible_send_is_read_back() {
    on_big_stacks(async {
        let (s, fake) = mail(vec![Err(CallError::Unknown("mail did not answer within 120s".into()))]).await;
        let r = run(&s).await;
        let task = s.k.read(|st| st.runs[&r].task.clone());
        let approval = json!({ "kind": "approval", "id": "ap1", "item": "i1", "person": "mo", "task": task, "tool": "mail__send", "scope": "standing", "risk": "irreversible" });
        s.k.emit("person:mo", serde_json::from_value(approval).unwrap()).await.unwrap();
        let send = action(&r, 1, "mail__send", json!({ "to": "a@example.com", "body": "hello" }));
        let v = s.k.execute(send).await.unwrap();
        let Verdict::Done { text, is_error: false, .. } = v else { panic!("the send did not go through: {:?}", done_text(&v)) };
        assert!(text.contains("Read back with get_sent"), "the resent send was not read back: {text}");
        let tools: Vec<String> = fake.sent().into_iter().map(|c| c.tool).collect();
        assert_eq!(tools, ["send", "send", "get_sent"]);
        assert!(s.k.read(|st| !st.evidence.read_backs(&r).is_empty()), "no read-back was recorded");
        s.done().await;
    });
}
