//! The one paid call, against a booted kernel and a model adapter served
//! here: every side call is charged where it belongs and refused at its
//! limit, a failed call the provider charged for is counted, two calls at
//! once cannot pass a limit together, and a turned-off provider is never
//! asked.

use super::*;
use crate::admin_app::tests::{server_with, Server};
use crate::spend::{month, Scope};
use axum::Router;
use magnus_adapter_proto::{self as proto, Block, ErrorClass, Event, Request, Role, Usage};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A model adapter served in this process. Every call's worst case is
/// `worst_usd`; each costs `cost_usd`, takes `delay`, and fails after the
/// provider charged when `fail`. It answers the first tool offered with a
/// clean verdict, or text.
#[derive(Clone)]
struct Fake {
    worst_usd: f64,
    /// What the prompt is quoted at.
    input_usd: f64,
    cost_usd: f64,
    delay: Duration,
    fail: bool,
    calls: Arc<AtomicUsize>,
}

impl Fake {
    fn new(worst_usd: f64, cost_usd: f64) -> Fake {
        Fake { worst_usd, input_usd: 0.0, cost_usd, delay: Duration::ZERO, fail: false, calls: Arc::default() }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

/// A verdict that lets the work go on, by the tool that asks for it.
fn clean(tool: &str) -> Value {
    match tool {
        "monitor" => json!({ "decision": "allow", "reason": "fine" }),
        "screen" => json!({ "injection": false, "finding": "" }),
        _ => json!({}),
    }
}

async fn answer(f: Fake, req: Request, mut sink: proto::Sink) -> anyhow::Result<()> {
    match req {
        Request::Quote { .. } => sink.send(&Event::Quote { input_usd: f.input_usd, output_usd: f.worst_usd }).await,
        Request::EstimatePrompt(_) => sink.send(&Event::PromptSize { input_tokens: 10 }).await,
        Request::Complete(p) => {
            f.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(f.delay).await;
            sink.send(&Event::Usage(Usage { input_tokens: 10, output_tokens: 10, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: f.cost_usd })).await?;
            if f.fail {
                return sink.error(ErrorClass::Unavailable, "the provider fell over after charging").await;
            }
            match p.tools.first() {
                Some(t) => sink.send(&Event::ToolCall { id: "c1".into(), name: t.name.clone(), input: clean(&t.name) }).await?,
                None => sink.send(&Event::Text { text: "ok".into() }).await?,
            }
            sink.send(&Event::ServedModel { id: p.model }).await?;
            sink.send(&Event::Done { stop: "end".into() }).await
        }
        Request::Image(_) => {
            f.calls.fetch_add(1, Ordering::SeqCst);
            sink.send(&Event::Image(proto::Picture { media_type: "image/png".into(), data_b64: "iVBORw0KGgo=".into() })).await
        }
        _ => sink.error(ErrorClass::InvalidRequest, "not served here").await,
    }
}

/// Serve `f` as adapter `id` on the server's kernel.
async fn serve(s: &Server, id: &str, f: &Fake) {
    let socket = std::env::temp_dir().join(format!("mf-{}.sock", crate::kernel::new_id(id)));
    let f = f.clone();
    let path = socket.clone();
    tokio::spawn(async move { proto::serve(&path, move |req, sink| answer(f.clone(), req, sink)).await });
    while !socket.exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    s.k.adapters.install_served(id, socket).await;
}

/// The safety models, set for the server's own checks.
const SAFETY: &str = "[safety]\nmonitor_model = \"fake/mon\"\ninbound_model = \"fake/scr\"\nreviewer_model = \"fake/rev\"\n";

async fn emit(s: &Server, actor: &str, e: Value) {
    s.k.emit(actor, serde_json::from_value(e).unwrap()).await.unwrap();
}

/// A run of `person`'s on the fake model, not driven: the calls below are
/// made on it by hand.
async fn run(s: &Server, id: &str, person: &str) -> Run {
    let config = json!({ "adapter": "fake", "model": "m", "max_retries": 0, "max_tokens": 100, "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 50.0 }, "verifier": false, "instructions": "", "harness": "" });
    emit(s, &format!("person:{person}"), json!({ "kind": "task_created", "task": format!("t-{id}"), "run": id, "person": person, "text": "x", "config": config })).await;
    s.k.read(|st| st.runs[id].clone())
}

async fn limit(s: &Server, scope: Scope, usd: f64) {
    s.k.emit("person:sam", crate::events::Ev::Spend(Change::LimitSet { scope, usd: Some(usd) })).await.unwrap();
}

fn spent(s: &Server) -> (f64, f64) {
    let m = month::key(Utc::now());
    s.k.read(|st| month::spent_by(st, &m, "mo"))
}

fn user(text: &str) -> Vec<Message> {
    vec![Message { role: Role::User, content: vec![Block::Text { text: text.into() }], state: None }]
}

async fn review(s: &Server, r: &Run) -> Option<Vec<Block>> {
    s.k.reviewer_call(r, "send", "Review.", user("a send"), vec![]).await.unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The reviewer, the monitor and an inbound screen on a run are each
/// charged to it and to the month; at the person's limit each is refused
/// before the provider is asked, and the checks fail safe.
#[tokio::test(flavor = "multi_thread")]
async fn each_side_call_on_a_run_is_charged_and_refused_at_the_limit() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake::new(0.1, 0.05);
    serve(&s, "fake", &f).await;
    let r = run(&s, "r1", "mo").await;
    assert!(review(&s, &r).await.is_some(), "the reviewer gave nothing");
    s.k.watch_answer(&r, "here is the answer").await;
    let page = format!("a page {}", crate::kernel::new_id("p"));
    let (shown, _) = s.k.screen_inbound("r1", "web_fetch", page.clone(), None, true).await;
    assert_eq!(shown, page, "a clean page was withheld");
    assert_eq!(f.calls(), 3);
    let charged = s.k.read(|st| st.runs["r1"].usd);
    assert!(near(charged, 0.15), "the run was charged {charged}, not its three calls");
    assert!(near(spent(&s).1, 0.15), "the month did not count the side calls: {:?}", spent(&s));

    limit(&s, Scope::Person("mo".into()), 0.15).await;
    assert!(review(&s, &r).await.is_none(), "the reviewer was asked past mo's month limit");
    let page = format!("another page {}", crate::kernel::new_id("p"));
    let (shown, _) = s.k.screen_inbound("r1", "web_fetch", page.clone(), None, true).await;
    assert!(shown.starts_with("[Withheld on arrival"), "an unscreened page reached the agent: {shown}");
    s.k.watch_answer(&r, "another answer").await;
    assert_eq!(f.calls(), 3, "a side call went to the provider past the limit");
    let told = s.k.read(|st| st.inbox.items().values().any(|i| i.run.as_deref() == Some("r1") && i.summary.contains("could not check")));
    assert!(told, "the monitor that could not run said nothing");
    s.done().await;
}

/// A call for no run is charged to its owner: a tool definition's screen to
/// the server, a credential review to the computer's workspace. Each is
/// refused at its owner's limit.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_for_no_run_is_charged_to_its_owner_and_refused_at_its_limit() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake::new(0.1, 0.05);
    serve(&s, "fake", &f).await;
    serve(&s, "a", &f).await;
    emit(&s, "person:sam", json!({ "kind": "team", "change": "created", "workspace": "w", "name": "W", "owner": "sam" })).await;
    let words = format!("A tool that lists things {}", crate::kernel::new_id("d"));
    assert!(matches!(s.k.screen_definition(None, &words).await, crate::safety::Screen::Passed(_)));
    let _ = s.k.review_credential_use("workspace:w", "pc-1", "GET", "api.example.com", "/").await;
    assert_eq!(f.calls(), 2);
    let m = month::key(Utc::now());
    let sp = s.k.read(|st| month::spent(st, &m));
    assert_eq!((sp.by_agent.get("server").copied(), sp.by_agent.get("team:w").copied()), (Some(0.05), Some(0.05)), "{sp:?}");
    let team = s.k.read(|st| crate::team::spend::spent_this_month(st, "w", Utc::now()));
    assert!(near(team.total_usd, 0.05), "the workspace's month did not count its review");

    emit(&s, "person:sam", json!({ "kind": "team", "change": "chat", "workspace": "w", "chat": { "what": "spend_limit", "usd": 0.05 } })).await;
    let v = s.k.review_credential_use("workspace:w", "pc-1", "POST", "api.example.com", "/x").await;
    assert!(matches!(v, crate::auto_review::Verdict::Ask(_)), "a credential left past the workspace's limit");
    limit(&s, Scope::Server, 0.1).await;
    let words = format!("Another tool {}", crate::kernel::new_id("d"));
    assert!(matches!(s.k.screen_definition(None, &words).await, crate::safety::Screen::Unanswered), "a screen ran past the server's limit");
    assert_eq!(f.calls(), 2, "a call went to the provider past its owner's limit");
    s.done().await;
}

/// What the provider charged before it failed is counted: on the run, on
/// the month, and for a call with no run, on the server's month.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_call_the_provider_charged_for_is_counted() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake { fail: true, ..Fake::new(0.5, 0.25) };
    serve(&s, "fake", &f).await;
    let r = run(&s, "r1", "mo").await;
    assert!(review(&s, &r).await.is_none());
    let words = format!("A tool {}", crate::kernel::new_id("d"));
    assert!(matches!(s.k.screen_definition(None, &words).await, crate::safety::Screen::Unanswered));
    assert!(near(s.k.read(|st| st.runs["r1"].usd), 0.25), "the run was not charged for the failed call");
    let (all, mo) = spent(&s);
    assert!(near(all, 0.5) && near(mo, 0.25), "the month did not count both failed calls: {all} {mo}");
    s.done().await;
}

/// A call its own timeout cuts off is booked at the quoted price of its
/// prompt, which the provider was sent and bills: on the run and on the
/// month. Before, it was booked at nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_cut_off_by_its_timeout_is_booked() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake { delay: Duration::from_millis(800), input_usd: 0.02, ..Fake::new(0.1, 0.05) };
    serve(&s, "fake", &f).await;
    let r = run(&s, "r1", "mo").await;
    let out = s.k.reviewer_call_within(&r, "send", "Review.", user("a send"), vec![], Some(Duration::from_millis(50))).await.unwrap();
    assert!(out.is_none(), "a call past its timeout gave an answer");
    assert_eq!(f.calls(), 1);
    let charged = s.k.read(|st| st.runs["r1"].usd);
    assert!(near(charged, 0.02), "the run was charged {charged}, not its prompt's quoted price");
    assert!(near(spent(&s).1, 0.02), "the month did not count the call cut off: {:?}", spent(&s));
    s.done().await;
}

/// Pictures made by the fake adapter, at a price.
const IMAGES: &str =
    "[[adapters]]\nid = \"fake\"\ncommand = [\"unused\"]\n[images]\nadapter = \"fake\"\nfast_model = \"img\"\npremium_model = \"img-hq\"\nfast_usd = 0.04\npremium_usd = 0.2\n";

/// How long a test waits for a picture made in the background.
const PICTURE_WAIT: Duration = Duration::from_secs(10);

/// A picture goes through the one paid call: priced at the configured
/// price and recorded on its run and the month, and refused at the
/// person's limit before the provider is asked. Before, a picture had no
/// price, no record and no model check.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_is_priced_and_recorded() {
    let s = server_with(IMAGES, |_| Router::new()).await;
    let f = Fake::new(0.1, 0.05);
    serve(&s, "fake", &f).await;
    run(&s, "r1", "mo").await;
    let a = |call: &str| crate::boundary::Action {
        run: "r1".into(),
        call_id: call.into(),
        step_id: format!("s1.{call}"),
        tool: crate::ui::images::GENERATE_TOOL.into(),
        input: json!({ "prompt": "a red barn" }),
        issuer: crate::boundary::Issuer::Model,
    };
    let (text, is_error, _) = s.k.image_tool(&a("c1")).await.unwrap();
    assert!(!is_error, "{text}");
    let deadline = tokio::time::Instant::now() + PICTURE_WAIT;
    while s.k.read(|st| st.runs["r1"].usd) == 0.0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(f.calls(), 1);
    let charged = s.k.read(|st| st.runs["r1"].usd);
    assert!(near(charged, 0.04), "the run was charged {charged} for a picture priced at $0.04");
    assert!(near(spent(&s).1, 0.04), "the month did not count the picture: {:?}", spent(&s));

    limit(&s, Scope::Person("mo".into()), 0.05).await;
    let (text, is_error, _) = s.k.image_tool(&a("c2")).await.unwrap();
    assert!(is_error && text.contains("not made"), "a picture past mo's month limit was started: {text}");
    assert_eq!(f.calls(), 1, "the provider was asked for a picture past the limit");
    s.done().await;
}

/// An embedding goes through the one paid call: charged at the server's
/// price per million tokens it counted, to the person who searched or, for
/// indexing, to the server. Before, it was posted with no record.
#[tokio::test(flavor = "multi_thread")]
async fn an_embedding_is_priced_and_recorded() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let reply = || async { axum::Json(json!({ "data": [{ "embedding": [0.5, 0.25] }], "usage": { "prompt_tokens": 5 } })) };
    let embeddings = Router::new().route("/v1/embeddings", axum::routing::post(reply));
    tokio::spawn(async move { axum::serve(l, embeddings).await });
    let s = server_with(&format!("[knowledge.embeddings]\nurl = {url:?}\nmodel = \"e\"\nusd_per_mtok = 1000.0\n"), |_| Router::new()).await;
    let got = s.k.embed(Owner::Person("mo".into()), "the books").await.unwrap();
    assert_eq!(got, Some(("e".to_string(), "[0.5,0.25]".to_string())));
    assert!(near(spent(&s).1, 0.005), "mo's month did not count the embedding: {:?}", spent(&s));
    s.k.embed(Owner::Server, "an entry").await.unwrap();
    let sp = s.k.read(|st| month::spent(st, &month::key(Utc::now())));
    assert_eq!(sp.by_agent.get("server").copied(), Some(0.005), "{sp:?}");
    s.done().await;
}

/// Two runs of one person, each asking for a call whose worst case is
/// within the month's limit alone but not together: the second waits for
/// the first and, with too little left, is refused. Together they never
/// pass the limit.
#[tokio::test(flavor = "multi_thread")]
async fn two_calls_at_once_cannot_together_pass_the_month() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake { delay: Duration::from_millis(300), ..Fake::new(0.6, 0.6) };
    serve(&s, "fake", &f).await;
    limit(&s, Scope::Person("mo".into()), 1.0).await;
    let (r1, r2) = (run(&s, "r1", "mo").await, run(&s, "r2", "mo").await);
    let (a, b) = tokio::join!(review(&s, &r1), review(&s, &r2));
    assert_eq!([a.is_some(), b.is_some()].iter().filter(|x| **x).count(), 1, "both calls, or neither, went ahead");
    assert_eq!(f.calls(), 1);
    assert!(spent(&s).1 <= 1.0, "two calls at once passed mo's month limit: {:?}", spent(&s));
    s.done().await;
}

/// A provider an admin turned off is refused when named directly (the
/// model check, a run's start and a side call), and as an escalation; a
/// fallback on it is skipped.
#[tokio::test(flavor = "multi_thread")]
async fn a_turned_off_provider_is_refused_directly_by_escalation_and_by_fallback() {
    let s = server_with(SAFETY, |_| Router::new()).await;
    let f = Fake::new(0.1, 0.05);
    serve(&s, "fake", &f).await;
    let r = run(&s, "r1", "mo").await;
    assert_eq!(s.k.escalation_for("mo", Some("fake/big".into())).await.as_deref(), Some("fake/big"));
    emit(&s, "person:sam", json!({ "kind": "provider_set", "id": "fake", "enabled": false })).await;
    let refused = s.k.check_model("fake", "m").await.unwrap_err();
    assert!(refused.contains("turned off"), "{refused}");
    let start = s.k.admit_run("mo", Some(("fake".into(), "m".into())), s.k.cfg.budget.clone()).await;
    assert!(start.is_err_and(|e| e.to_string().contains("turned off")), "a run started on a turned-off provider");
    assert!(review(&s, &r).await.is_none(), "the reviewer asked a turned-off provider");
    assert_eq!(f.calls(), 0, "a turned-off provider was asked");
    assert_eq!(s.k.escalation_for("mo", Some("fake/big".into())).await, None, "a run escalates to a turned-off provider");
    let fallbacks = vec!["fake/m2".to_string(), "other/m".to_string()];
    let off = s.k.read(|st| st.org.providers_off().clone());
    let mut idx = 0;
    assert_eq!(fallback_after(&fallbacks, &None, &off, ("a", "m"), &mut idx), Some(("other".into(), "m".into())), "a fallback on a turned-off provider was taken");
    s.done().await;
}
