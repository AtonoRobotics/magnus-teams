//! A fake outside agent for the tests: an in-process axum server speaking
//! A2A 1.0's JSON-RPC binding, with a signed card and its key set, which
//! counts what it is sent.

use super::jws;
use crate::admin_app::tests::Server;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The key the fake agent's operator issued Magnus.
pub(super) const KEY: &str = "supplier-key-123";
pub(super) const NAME: &str = "Supplier";

/// What the fake agent was sent, and how it answers.
#[derive(Default)]
pub(super) struct Seen {
    pub sends: Vec<Value>,
    pub gets: Vec<String>,
    pub cancels: Vec<String>,
    pub tasks: BTreeMap<String, String>,
    /// What it says with each task.
    pub says: String,
    /// The state a new task starts in, and the state GetTask finds it in.
    pub starts: String,
    pub later: String,
}

pub(super) struct Fake {
    pub base: String,
    pub seen: Arc<Mutex<Seen>>,
}

impl Fake {
    pub fn card_url(&self) -> String {
        format!("{}{}", self.base, super::CARD_PATH)
    }

    pub fn sends(&self) -> usize {
        self.seen.lock().unwrap().sends.len()
    }
}

fn task(id: &str, state: &str, says: &str) -> Value {
    json!({ "id": id, "contextId": id, "status": { "state": state, "message": { "messageId": format!("{id}-m"), "role": "ROLE_AGENT", "parts": [{ "text": says }] } } })
}

async fn rpc(State(seen): State<Arc<Mutex<Seen>>>, headers: HeaderMap, Json(req): Json<Value>) -> (StatusCode, Json<Value>) {
    if headers.get("x-api-key").and_then(|v| v.to_str().ok()) != Some(KEY) || headers.get("a2a-version").and_then(|v| v.to_str().ok()) != Some("1.0") {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32050, "message": "who are you" } })));
    }
    let mut s = seen.lock().unwrap();
    let id = req["params"]["id"].as_str().unwrap_or_default().to_string();
    let result = match req["method"].as_str().unwrap_or_default() {
        "SendMessage" => {
            s.sends.push(req["params"].clone());
            let t = format!("t-{}", s.sends.len());
            let state = s.starts.clone();
            s.tasks.insert(t.clone(), state.clone());
            json!({ "task": task(&t, &state, &s.says) })
        }
        "GetTask" => {
            s.gets.push(id.clone());
            let later = s.later.clone();
            match s.tasks.get_mut(&id) {
                Some(state) => {
                    *state = later.clone();
                    task(&id, &later, &s.says)
                }
                None => return (StatusCode::OK, Json(json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32001, "message": "no such task" } }))),
            }
        }
        "CancelTask" => {
            s.cancels.push(id.clone());
            s.tasks.insert(id.clone(), super::CANCELED.into());
            task(&id, super::CANCELED, "cancelled")
        }
        _ => return (StatusCode::OK, Json(json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32601, "message": "no" } }))),
    };
    (StatusCode::OK, Json(json!({ "jsonrpc": "2.0", "id": req["id"], "result": result })))
}

/// A fake agent saying `says`, whose tasks start `starts` and are found `later`.
pub(super) async fn agent(says: &str, starts: &str, later: &str) -> Fake {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let rng = ring::rand::SystemRandom::new();
    let key = Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref()).unwrap();
    let mut card = json!({
        "name": NAME,
        "description": "A parts supplier's agent.",
        "supportedInterfaces": [{ "url": format!("{base}/rpc"), "protocolBinding": "JSONRPC", "protocolVersion": "1.0" }],
        "version": "1.0.0",
        "capabilities": {},
        "securitySchemes": { "key": { "apiKeySecurityScheme": { "location": "header", "name": "x-api-key" } } },
        "securityRequirements": [{ "schemes": { "key": { "list": [] } } }],
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": [{ "id": "quote", "name": "quote", "description": "Quotes and ships parts.", "tags": [] }]
    });
    card["signatures"] = json!([jws::sign(&card, &key, "k1", "/keys.json")]);
    let keys = json!({ "keys": [jws::ed25519_jwk(key.public_key().as_ref(), "k1")] });
    let seen = Arc::new(Mutex::new(Seen { says: says.into(), starts: starts.into(), later: later.into(), ..Default::default() }));
    let app = Router::new()
        .route(super::CARD_PATH, get(move || async move { Json(card) }))
        .route("/keys.json", get(move || async move { Json(keys) }))
        .route("/rpc", post(rpc))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(l, app).await });
    Fake { base, seen }
}

/// `person` connects the fake agent: its id and the key it presents here.
pub(super) async fn connect(s: &Server, person: &str, fake: &Fake) -> (String, String) {
    let t = s.token(person).await;
    let (st, v) = s.post(&t, "/v1/outside-agents", json!({ "card_url": fake.card_url(), "credential": KEY })).await;
    assert_eq!(st, 201, "{v}");
    (v["agent"]["id"].as_str().unwrap().to_string(), v["inbound_key"].as_str().unwrap().to_string())
}

/// One JSON-RPC call to the server's endpoint, as an outside agent makes it.
pub(super) async fn ask(s: &Server, key: &str, method: &str, params: Value) -> (u16, Value) {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let r =
        reqwest::Client::new().post(format!("{}{}", s.base, super::ENDPOINT_PATH)).header(super::KEY_HEADER, key).header("A2A-Version", "1.0").json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

/// A SendMessage asking `to` (`{"person": ..}` or `{"project": ..}`).
pub(super) fn request(message_id: &str, text: &str, to: Value) -> Value {
    json!({ "message": { "messageId": message_id, "role": "ROLE_USER", "parts": [{ "text": text }], "metadata": { "to": to } } })
}
