//! Payments: a one-time Link card per approved purchase, and a saved card
//! approved at every fill. A fake Link stands in for Stripe's service.

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use magnus_invariants::{model::*, *};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const PAGE: &str = r#"<html><form action="/paid" method="get"><input name="number" placeholder="Card"><input name="cvc" placeholder="Code"><button>Pay</button></form></html>"#;
const WHY: &str = "Buying one notebook from the shop because the person asked for it in this task, and this is the only seller with it in stock.";

#[derive(Default)]
struct LinkState {
    answer: String,
    number: String,
    created: Vec<Value>,
}

#[derive(Clone)]
struct FakeLink {
    base: String,
    state: Arc<Mutex<LinkState>>,
}

type L = State<Arc<Mutex<LinkState>>>;

async fn token() -> Json<Value> {
    Json(json!({ "access_token": "liwltoken_a", "refresh_token": "liwlrefresh_a", "token_type": "Bearer", "expires_in": 3600 }))
}

async fn create(State(s): L, Json(body): Json<Value>) -> Json<Value> {
    let mut s = s.lock().unwrap();
    s.created.push(body);
    Json(json!({ "id": format!("lsrq_{}", s.created.len()), "status": "pending_approval", "approval_url": "https://app.link.test/approve" }))
}

async fn retrieve(State(s): L, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let s = s.lock().unwrap();
    let mut v = json!({ "id": id, "status": s.answer });
    if q.get("include").is_some_and(|i| i == "card") {
        v["card"] = json!({ "number": s.number, "cvc": "731", "exp_month": 6, "exp_year": 2031, "billing_address": { "name": "Alice", "postal_code": "94015" } });
    }
    Json(v)
}

impl FakeLink {
    async fn start(answer: &str, number: &str) -> FakeLink {
        let state = Arc::new(Mutex::new(LinkState { answer: answer.into(), number: number.into(), created: vec![] }));
        let app = Router::new().route("/auth/token", post(token)).route("/spend_requests", post(create)).route("/spend_requests/{id}", get(retrieve)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeLink { base, state }
    }

    fn created(&self) -> Vec<Value> {
        self.state.lock().unwrap().created.clone()
    }

    fn section(&self) -> Value {
        json!({ "link": { "client_id": "cid", "publishable_key": "pk_test", "client_secret": "link_client", "redirect_uri": "http://magnus.test/cb", "api_url": self.base, "login_url": self.base, "test_mode": true } })
    }
}

fn bare(site: &FakeWeb) -> String {
    site.base.trim_start_matches("http://").to_string()
}

fn card_number() -> String {
    // A canary would not look like a card; this is long and unique per run.
    let digits: String = uuid::Uuid::new_v4().as_u128().to_string().chars().take(16).collect();
    format!("{digits:0>16}")
}

fn permit_all() -> String {
    org_policy(&["browser_as_navigate", "purchase_request", "purchase_fill"].iter().map(|t| permit_execute(t)).collect::<Vec<_>>().join("\n"))
}

fn buy(url: &str) -> Value {
    json!({ "merchant_name": "The Shop", "merchant_url": url, "amount_minor": 3505, "context": WHY })
}

fn fill(purchase: &str, identity: &str) -> Value {
    json!({ "purchase_id": purchase, "identity": identity, "fields": { "number": "e1", "cvc": "e2" }, "submit": true })
}

struct Shop {
    _computerd: Computerd,
    site: FakeWeb,
    model: FakeModel,
    app: Appliance,
    identity: String,
}

async fn shop(section: Value) -> Shop {
    let computerd = Computerd::start();
    let site = FakeWeb::start(&[("/pay", PAGE), ("/paid", "<html><p>thank you</p></html>")]).await;
    let model = FakeModel::start(steps(vec![])).await;
    let spec = Spec::default().section("payments", section).computer_options(json!({ "public_hosts": { "shop.test": [bare(&site)] } }));
    let app = Appliance::start(spec.computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    assert!(alice.put_secret("link_client", "client-secret").await.is_success());
    assert!(alice.put_policy("org", &permit_all()).await.is_success());
    let (code, made) = alice.post("/v1/identities", json!({ "site": "shop.test" })).await;
    assert!(code.is_success(), "{made}");
    let identity = made["id"].as_str().unwrap().to_string();
    Shop { _computerd: computerd, site, model, app, identity }
}

async fn connect_link(app: &Appliance) {
    let alice = app.api("alice");
    let (code, started) = alice.post("/v1/payments/link", json!({})).await;
    assert_eq!(code, 200, "{started}");
    let url = url::Url::parse(started["url"].as_str().unwrap()).unwrap();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(query["scope"], "payment_methods.agentic");
    assert_eq!(query["code_challenge_method"], "S256");
    let (code, said) = alice.get(&format!("/v1/payments/link/callback?code=abc&state={}", query["state"])).await;
    assert!(code.is_success(), "{said}");
    let (code, _) = alice.get("/v1/payments/link/callback?code=abc&state=not-ours").await;
    assert_eq!(code, 400, "a sign-in nobody started was accepted");
}

fn answers(model: &FakeModel) -> Vec<String> {
    model.requests().iter().flat_map(|r| r.last_results()).map(|r| r.text).collect()
}

/// The purchase id Magnus returned, which the next step of the script needs.
fn purchase_in(c: &Conversation) -> String {
    let said = c.body.to_string();
    let at = said.rfind("pur_").expect("no purchase id yet");
    said[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
}

/// Reading a shop's page taints the run, so the kernel's Rule of Two asks the
/// person about each of these actions as well. Approve the next new ask.
async fn approve_next(alice: &Api, task: &str, tool: &str, seen: &mut Vec<String>) -> Value {
    let asked = eventually(&format!("an approval for {tool}"), std::time::Duration::from_secs(90), || {
        let (alice, task, seen) = (alice.clone(), task.to_string(), seen.clone());
        async move { alice.open_items(&task, "approval").await.into_iter().find(|i| !seen.iter().any(|id| i["id"] == id.as_str())) }
    })
    .await;
    assert_eq!(asked["details"]["tool"], tool, "{asked}");
    let id = asked["id"].as_str().unwrap();
    seen.push(id.to_string());
    assert!(alice.respond(id, json!({ "decision": "approve" })).await.is_success());
    asked
}

async fn no_trace(s: &Shop, secret: &str, what: &str) {
    for (i, r) in s.model.requests().iter().enumerate() {
        assert!(!scan::contains(r.text().as_bytes(), secret), "{what} reached model request {i}");
    }
    assert!(!scan::contains(s.app.db.dump().await.as_bytes(), secret), "{what} reached the log");
    assert!(!scan::contains(s.app.logs().as_bytes(), secret), "{what} reached the server's output");
    let files = scan::files_containing(&s.app.state_dir, secret);
    assert!(files.is_empty(), "{what} reached a file: {files:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_purchase_is_a_one_time_card_the_person_approved_and_the_agent_never_sees() {
    let number = card_number();
    let link = FakeLink::start("approved", &number).await;
    let s = shop(link.section()).await;
    connect_link(&s.app).await;
    let identity = s.identity.clone();
    s.model.set_script(script(move |c| {
        let identity = identity.clone();
        async move {
            match c.turn() {
                0 => plan("buy it"),
                1 => tool("browser_as_navigate", json!({ "identity": identity, "url": "http://shop.test/pay" })),
                2 => tool("purchase_request", buy("http://shop.test/pay")),
                3 => tool("purchase_fill", fill(&purchase_in(&c), &identity)),
                4 => tool("purchase_fill", fill(&purchase_in(&c), &identity)),
                _ => complete("bought"),
            }
        }
    }));
    let alice = s.app.api("alice");
    let task = alice.submit("Buy the notebook.").await;
    let mut seen = vec![];
    for tool in ["purchase_request", "purchase_fill", "purchase_fill"] {
        approve_next(&alice, &task, tool, &mut seen).await;
    }
    alice.wait_status(&task, &["completed"]).await;

    let made = link.created();
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!((made[0]["amount"].as_i64(), made[0]["merchant_name"].as_str(), made[0]["test"].as_bool()), (Some(3505), Some("The Shop"), Some(true)));
    assert!(s.site.hits().iter().any(|p| p.starts_with("/paid?") && p.contains(&number) && p.contains("731")), "the card never reached the page: {:?}", s.site.hits());
    let results = answers(&s.model);
    assert!(results.iter().any(|t| t.contains("already used")), "a one-time card was filled twice: {results:?}");
    assert!(alice.open_items(&task, "approval").await.is_empty());
    no_trace(&s, &number, "the card number").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_purchase_declined_in_link_ends_there_and_no_saved_card_stands_in() {
    let (link_number, saved) = (card_number(), card_number());
    let link = FakeLink::start("denied", &link_number).await;
    let s = shop(link.section()).await;
    connect_link(&s.app).await;
    let alice = s.app.api("alice");
    assert!(alice.put("/v1/payments/card", json!({ "number": saved, "cvc": "555", "exp_month": 1, "exp_year": 2032 })).await.0.is_success());
    let identity = s.identity.clone();
    s.model.set_script(script(move |c| {
        let identity = identity.clone();
        async move {
            match c.turn() {
                0 => plan("buy it"),
                1 => tool("browser_as_navigate", json!({ "identity": identity, "url": "http://shop.test/pay" })),
                2 => tool("purchase_request", buy("http://shop.test/pay")),
                _ => complete("could not buy it"),
            }
        }
    }));
    let task = alice.submit("Buy the notebook.").await;
    approve_next(&alice, &task, "purchase_request", &mut vec![]).await;
    eventually("Link's answer to reach the agent", std::time::Duration::from_secs(90), || {
        let model = s.model.clone();
        async move { answers(&model).iter().any(|t| t.contains("did not approve")).then_some(()) }
    })
    .await;
    alice.wait_settled(&task).await;
    let results = answers(&s.model);
    assert!(!results.iter().any(|t| t.contains("pur_")), "a declined purchase left something to fill: {results:?}");
    assert!(!s.site.hits().iter().any(|p| p.starts_with("/paid")), "a declined purchase reached the page");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_card_is_approved_at_every_fill_and_works_only_at_its_seller() {
    let saved = card_number();
    let s = shop(json!({})).await;
    let alice = s.app.api("alice");
    assert!(alice.put("/v1/payments/card", json!({ "number": saved, "cvc": "555", "exp_month": 1, "exp_year": 2032 })).await.0.is_success());
    let identity = s.identity.clone();
    s.model.set_script(script(move |c| {
        let identity = identity.clone();
        async move {
            match c.turn() {
                0 => plan("buy it"),
                1 => tool("browser_as_navigate", json!({ "identity": identity, "url": "http://shop.test/pay" })),
                2 => tool("purchase_request", buy("http://other.test/buy")),
                3 => tool("purchase_fill", fill(&purchase_in(&c), &identity)),
                4 => tool("purchase_request", buy("http://shop.test/pay")),
                5 => tool("purchase_fill", fill(&purchase_in(&c), &identity)),
                _ => complete("bought"),
            }
        }
    }));
    let task = alice.submit("Buy the notebook.").await;
    let mut seen = vec![];
    for round in 0..2 {
        approve_next(&alice, &task, "purchase_request", &mut seen).await;
        let asked = eventually("the saved card's approval", std::time::Duration::from_secs(90), || {
            let (alice, task, seen) = (alice.clone(), task.clone(), seen.clone());
            async move { alice.open_items(&task, "approval").await.into_iter().find(|i| !seen.iter().any(|id| i["id"] == id.as_str())) }
        })
        .await;
        seen.push(asked["id"].as_str().unwrap().to_string());
        assert_eq!(asked["details"]["tool"], "purchase_fill", "{asked}");
        // The ask's reasoning stays off the person's surface (spec rev 140);
        // the card itself says it is the saved card, what and where.
        let said = asked["summary"].as_str().unwrap_or_default();
        assert!(said.contains("saved card"), "the fill was not asked as a saved card: {asked}");
        assert!(said.contains("USD 35.05"), "the person was not told what it costs: {asked}");
        assert_eq!(asked["ask"]["allow_scope"], "once", "a saved card's Allow covered more than this fill: {asked}");
        let id = asked["id"].as_str().unwrap();
        assert!(!alice.respond(id, json!({ "decision": "approve", "scope": "task" })).await.is_success(), "a saved card was approved for more than one use");
        assert!(alice.respond(id, json!({ "decision": "approve" })).await.is_success());
        if round == 0 {
            eventually("the first fill to be refused", std::time::Duration::from_secs(60), || {
                let model = s.model.clone();
                async move { answers(&model).iter().any(|t| t.contains("only for other.test")).then_some(()) }
            })
            .await;
            assert!(!s.site.hits().iter().any(|p| p.starts_with("/paid")), "a card went to a seller it was not for");
        }
    }
    alice.wait_status(&task, &["completed"]).await;
    assert!(s.site.hits().iter().any(|p| p.starts_with("/paid?") && p.contains(&saved)), "{:?}", s.site.hits());
    no_trace(&s, &saved, "the saved card").await;
}
