//! A relayed sign-in against a fake relay: an in-process server that is the
//! app's authorization server (its metadata) and the relay (refresh and
//! revocation), signing with a test key. The claim and the handoff are sent
//! as the relay would send them.

use super::sign_in::{age, is_waiting};
use super::*;
use crate::admin_app::tests::{server_with, Server};
use crate::connectors::{connection_name, PENDING_FOR};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const RELAY_SEED: [u8; 32] = [7; 32];
const OTHER_SEED: [u8; 32] = [9; 32];
const SERVER_ID: &str = "srv-test";
const PAST_EXPIRY: Duration = Duration::from_secs(1);

fn pair(seed: [u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&seed).unwrap()
}

fn signed(seed: [u8; 32], m: &Message) -> Signed {
    seal_with(m, |b| pair(seed).sign(b).as_ref().to_vec()).unwrap()
}

#[derive(Default)]
struct Fake {
    base: String,
    server_key: Mutex<Option<[u8; 32]>>,
    refuse_refresh: Mutex<bool>,
    refreshed: Mutex<Vec<String>>,
    revoked: Mutex<Vec<String>>,
}

type F = State<Arc<Fake>>;

async fn mcp(State(f): F) -> impl IntoResponse {
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, format!("Bearer resource_metadata=\"{}/prm\"", f.base))])
}

async fn prm(State(f): F) -> Json<Value> {
    Json(json!({ "resource": format!("{}/mcp", f.base), "authorization_servers": [f.base] }))
}

async fn asm(State(f): F) -> Json<Value> {
    let b = &f.base;
    Json(json!({
        "issuer": b, "authorization_endpoint": format!("{b}/authorize"), "token_endpoint": format!("{b}/token"),
        "revocation_endpoint": format!("{b}/revoke"), "code_challenge_methods_supported": ["S256"], "authorization_response_iss_parameter_supported": true,
    }))
}

fn from_server(f: &Fake, env: &Signed) -> Message {
    let key = f.server_key.lock().unwrap().expect("the server's key is known");
    open(env, &key).expect("signed by the server")
}

async fn relay_refresh(State(f): F, Json(env): Json<Signed>) -> Json<Signed> {
    let Message::Refresh { server_id, request, refresh_token, .. } = from_server(&f, &env) else { panic!("not a refresh") };
    f.refreshed.lock().unwrap().push(refresh_token);
    let outcome = if *f.refuse_refresh.lock().unwrap() {
        Refreshed::Refused { reason: "invalid_grant".into() }
    } else {
        Refreshed::Tokens { tokens: tokens("fresh-access", Some("rotated"), Some(3600)) }
    };
    Json(signed(RELAY_SEED, &Message::Refreshed { server_id, request, outcome, issued_at: now() }))
}

async fn relay_revoke(State(f): F, Json(env): Json<Signed>) -> Json<Signed> {
    let Message::Revoke { server_id, request, token, .. } = from_server(&f, &env) else { panic!("not a revocation") };
    f.revoked.lock().unwrap().push(token);
    Json(signed(RELAY_SEED, &Message::Revoked { server_id, request, done: true, detail: "revoked".into(), issued_at: now() }))
}

async fn fake() -> Arc<Fake> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Arc::new(Fake { base: format!("http://{}", l.local_addr().unwrap()), ..Default::default() });
    let app = Router::new()
        .route("/mcp", post(mcp))
        .route("/prm", get(prm))
        .route("/.well-known/oauth-authorization-server", get(asm))
        .route("/v1/refresh", post(relay_refresh))
        .route("/v1/revoke", post(relay_revoke))
        .with_state(f.clone());
    tokio::spawn(async move { axum::serve(l, app).await });
    f
}

fn tokens(access: &str, refresh: Option<&str>, expires_in: Option<i64>) -> RelayTokens {
    RelayTokens { access_token: access.into(), refresh_token: refresh.map(String::from), expires_in, scope: None, account: Some("sam@example.com".into()) }
}

fn connector(f: &Fake) -> String {
    format!(
        "[[connectors]]\nname = \"relayed\"\nurl = \"{}/mcp\"\ncredential = \"connector_relayed\"\nauth = \"oauth\"\naudience = \"private\"\nlabel = \"private+untrusted\"\nregistration = \"slack\"\n",
        f.base
    )
}

/// A server registered with the fake relay, for the app `relayed`.
async fn relayed() -> (Server, Arc<Fake>) {
    let f = fake().await;
    let public = B64.encode(pair(RELAY_SEED).public_key().as_ref());
    let more = format!("[relay]\nserver_id = \"{SERVER_ID}\"\npublic_key = \"{public}\"\nurl = \"{}\"\nvia_relay = [\"slack\"]\n{}", f.base, connector(&f));
    let s = server_with(&more, |_| Router::new()).await;
    *f.server_key.lock().unwrap() = Some(s.k.relay_keys.public(&s.k.vault).unwrap());
    (s, f)
}

/// Start `person`'s sign-in: its nonce and the provider address.
async fn begin(s: &Server, person: &str) -> (String, url::Url) {
    let c = s.k.connector("relayed").unwrap();
    let (address, state) = s.k.connector_sign_in(&c, person, "http://127.0.0.1:9/callback").await.unwrap();
    let nonce = state.strip_prefix(&format!("{SERVER_ID}.")).expect("the state names this server").to_string();
    (nonce, url::Url::parse(&address).unwrap())
}

async fn send(s: &Server, path: &str, env: &Signed) -> (u16, Value) {
    let r = reqwest::Client::new().post(format!("{}{path}", s.base)).json(env).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

fn claim_of(f: &Fake, nonce: &str) -> Message {
    Message::Claim { server_id: SERVER_ID.into(), nonce: nonce.into(), iss: Some(f.base.clone()), issued_at: now() }
}

fn handoff_of(nonce: &str, connector: &str, person: &str, t: RelayTokens) -> Message {
    Message::Handoff { server_id: SERVER_ID.into(), nonce: nonce.into(), connector: connector.into(), person: person.into(), tokens: t, issued_at: now() }
}

/// Begun and claimed, as the relay does once the provider sends it the code.
async fn claimed(s: &Server, f: &Fake, person: &str) -> String {
    let (nonce, _) = begin(s, person).await;
    assert_eq!(send(s, "/v1/relay/claim", &signed(RELAY_SEED, &claim_of(f, &nonce))).await.0, 200);
    nonce
}

fn stored(s: &Server, person: &str) -> Option<Value> {
    s.k.vault.get(&connection_name("relayed", person)).unwrap().map(|raw| serde_json::from_str(raw.expose()).unwrap())
}

#[tokio::test]
async fn a_relayed_sign_in_connects_and_its_nonce_is_used_once() {
    let (s, f) = relayed().await;
    let (nonce, address) = begin(&s, "sam").await;
    let q: std::collections::HashMap<String, String> = address.query_pairs().into_owned().collect();
    assert_eq!(q["redirect_uri"], CALLBACK, "the provider sends the browser to the relay");
    assert_eq!(q["state"], format!("{SERVER_ID}.{nonce}"));
    assert!(!q.contains_key("client_secret") && !address.as_str().contains("secret"));

    let (status, answer) = send(&s, "/v1/relay/claim", &signed(RELAY_SEED, &claim_of(&f, &nonce))).await;
    assert_eq!(status, 200);
    let answer: Signed = serde_json::from_value(answer).unwrap();
    let Message::Claimed { connector, person, provider, code_verifier, .. } = open(&answer, &f.server_key.lock().unwrap().unwrap()).unwrap() else { panic!("not claimed") };
    assert_eq!((connector.as_str(), person.as_str(), provider.as_str()), ("relayed", "sam", "slack"));
    assert_eq!(crate::connectors::challenge(&code_verifier.unwrap()), q["code_challenge"], "the verifier is the one the address was challenged with");
    assert_eq!(send(&s, "/v1/relay/claim", &signed(RELAY_SEED, &claim_of(&f, &nonce))).await.0, 403, "claimed once");

    let good = signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", tokens("access-1", Some("refresh-1"), Some(3600))));
    assert_eq!(send(&s, "/v1/relay/handoff", &good).await.0, 204);
    let t = stored(&s, "sam").expect("connected");
    assert_eq!(t["access_token"], "access-1");
    assert_eq!(t["client"]["relay"], "slack");
    assert!(t["client"]["client_secret"].is_null(), "no secret on the server");
    let c = s.k.connector("relayed").unwrap();
    assert_eq!(s.k.standing(&c, "sam").await.0, crate::connectors::Standing::Connected);

    // The same handoff again, and another for the used nonce: refused, nothing changed.
    assert_eq!(send(&s, "/v1/relay/handoff", &good).await.0, 403);
    let again = signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", tokens("replayed", None, None)));
    assert_eq!(send(&s, "/v1/relay/handoff", &again).await.0, 403);
    assert_eq!(stored(&s, "sam").unwrap()["access_token"], "access-1");

    // The owner reads what the relay registers.
    let (status, reg) = s.get(&s.token("sam").await, "/v1/relay").await;
    assert_eq!(status, 200);
    assert_eq!(reg["server_id"], SERVER_ID);
    assert_eq!(reg["public_key"], B64.encode(f.server_key.lock().unwrap().unwrap()));
    s.done().await;
}

#[tokio::test]
async fn a_handoff_that_is_not_right_is_refused_and_nothing_is_kept() {
    let (s, f) = relayed().await;
    let t = || tokens("bad", Some("bad"), Some(3600));

    // Signed with another key: refused, and the sign-in still waits for the real one.
    let nonce = claimed(&s, &f, "sam").await;
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(OTHER_SEED, &handoff_of(&nonce, "relayed", "sam", t()))).await.0, 403);
    assert!(is_waiting(&nonce), "a forged handoff does not use the nonce up");

    // Its body changed after signing.
    let mut tampered = signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", t()));
    let body = String::from_utf8(B64.decode(&tampered.payload).unwrap()).unwrap().replace("\"bad\"", "\"evil\"");
    tampered.payload = B64.encode(body);
    assert_eq!(send(&s, "/v1/relay/handoff", &tampered).await.0, 403);

    // For another server.
    let mut other = handoff_of(&nonce, "relayed", "sam", t());
    if let Message::Handoff { server_id, .. } = &mut other {
        *server_id = "srv-other".into();
    }
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &other)).await.0, 403);

    // Issued outside the skew window.
    let mut skewed = handoff_of(&nonce, "relayed", "sam", t());
    if let Message::Handoff { issued_at, .. } = &mut skewed {
        *issued_at -= ISSUED_SKEW_SECS + 60;
    }
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &skewed)).await.0, 403);

    // For another person, then for another app: each uses its nonce up.
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "mo", t()))).await.0, 403);
    assert!(!is_waiting(&nonce));
    let nonce = claimed(&s, &f, "sam").await;
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "slack", "sam", t()))).await.0, 403);

    // A nonce past its time.
    let nonce = claimed(&s, &f, "sam").await;
    age(&nonce, PENDING_FOR + PAST_EXPIRY);
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", t()))).await.0, 403);

    // Handed off without being claimed first.
    let (nonce, _) = begin(&s, "sam").await;
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", t()))).await.0, 403);

    // Claimed with the answer of another authorization server (RFC 9207), or with none.
    let (nonce, _) = begin(&s, "sam").await;
    let mixed = Message::Claim { server_id: SERVER_ID.into(), nonce: nonce.clone(), iss: Some("https://elsewhere.example".into()), issued_at: now() };
    assert_eq!(send(&s, "/v1/relay/claim", &signed(RELAY_SEED, &mixed)).await.0, 403);
    let (nonce, _) = begin(&s, "sam").await;
    let unnamed = Message::Claim { server_id: SERVER_ID.into(), nonce, iss: None, issued_at: now() };
    assert_eq!(send(&s, "/v1/relay/claim", &signed(RELAY_SEED, &unnamed)).await.0, 403);

    assert!(stored(&s, "sam").is_none(), "no refused handoff kept anything");
    assert!(stored(&s, "mo").is_none());
    s.done().await;
}

#[tokio::test]
async fn a_relayed_sign_in_refreshes_and_revokes_through_the_relay() {
    let (s, f) = relayed().await;
    let c = s.k.connector("relayed").unwrap();
    // Expired on arrival, so the first use refreshes.
    let nonce = claimed(&s, &f, "sam").await;
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "sam", tokens("access-1", Some("refresh-1"), Some(0))))).await.0, 204);
    let secret = s.k.connector_secret(&c, "sam").await.unwrap().expect("refreshed");
    assert_eq!(secret.expose(), "fresh-access");
    assert_eq!(*f.refreshed.lock().unwrap(), vec!["refresh-1".to_string()]);
    assert_eq!(stored(&s, "sam").unwrap()["refresh_token"], "rotated");

    let (at_provider, _) = s.k.disconnect("relayed", "sam", "person:sam").await.unwrap().expect("was signed in");
    assert!(at_provider);
    assert_eq!(*f.revoked.lock().unwrap(), vec!["rotated".to_string(), "fresh-access".to_string()]);
    assert!(stored(&s, "sam").is_none());

    // A refresh the provider refuses ends the sign-in until the person signs in again.
    *f.refuse_refresh.lock().unwrap() = true;
    let nonce = claimed(&s, &f, "mo").await;
    assert_eq!(send(&s, "/v1/relay/handoff", &signed(RELAY_SEED, &handoff_of(&nonce, "relayed", "mo", tokens("access-2", Some("refresh-2"), Some(0))))).await.0, 204);
    assert!(s.k.connector_secret(&c, "mo").await.unwrap().is_none());
    assert!(s.k.lapsed(&c, "mo"));
    s.done().await;
}

#[tokio::test]
async fn without_a_relay_sign_in_is_as_before() {
    let f = fake().await;
    let s = server_with(&connector(&f), |_| Router::new()).await;
    let owner = s.token("sam").await;
    assert_eq!(s.get(&owner, "/v1/relay").await.0, 404);
    let env = signed(RELAY_SEED, &claim_of(&f, "anything"));
    assert_eq!(send(&s, "/v1/relay/claim", &env).await.0, 404);
    assert_eq!(send(&s, "/v1/relay/handoff", &env).await.0, 404);
    let c = s.k.connector("relayed").unwrap();
    assert!(s.k.via_relay(&c).is_none());
    // Without the provisioned secret it cannot sign in; with it, the
    // address is the desktop's loopback one, with the secret kept here.
    assert!(s.k.connector_sign_in(&c, "sam", "http://127.0.0.1:9/callback").await.is_err());
    crate::registrations::provision(&s.k.vault, "slack", None, Some("provisioned-secret")).unwrap();
    let (address, state) = s.k.connector_sign_in(&c, "sam", "http://127.0.0.1:9/callback").await.unwrap();
    let q: std::collections::HashMap<String, String> = url::Url::parse(&address).unwrap().query_pairs().into_owned().collect();
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:9/callback");
    assert!(!state.contains('.'), "a plain nonce, naming no server");
    s.done().await;
}

#[test]
fn relay_settings_are_checked_at_startup() {
    let key = B64.encode(pair(RELAY_SEED).public_key().as_ref());
    let s = |id: &str, key: &str, url: &str, via: &[&str]| Settings {
        server_id: id.into(),
        public_key: key.into(),
        url: url.into(),
        callback: CALLBACK.into(),
        via_relay: via.iter().map(|v| v.to_string()).collect(),
    };
    assert!(s("srv-1", &key, "https://relay.example", &["slack", "google"]).check().is_ok());
    assert!(s("srv-1", &key, "http://127.0.0.1:8080", &[]).check().is_ok(), "plain http on this machine, for a relay under test");
    assert!(s("srv.1", &key, "https://relay.example", &[]).check().is_err(), "a '.' would split the state");
    assert!(s("", &key, "https://relay.example", &[]).check().is_err());
    assert!(s("srv-1", "not-a-key", "https://relay.example", &[]).check().is_err());
    assert!(s("srv-1", &B64.encode([1u8; 16]), "https://relay.example", &[]).check().is_err());
    assert!(s("srv-1", &key, "http://relay.example", &[]).check().is_err());
    assert!(s("srv-1", &key, "https://relay.example", &["microsoft"]).check().is_err(), "a public client has no secret to hold");
    assert!(s("srv-1", &key, "https://relay.example", &["nobody"]).check().is_err());
    assert!(s("srv-1", &key, "https://relay.example", &["slack", "slack"]).check().is_err());
}

#[test]
fn a_message_opens_only_with_its_signers_key() {
    let m = Message::Claim { server_id: SERVER_ID.into(), nonce: "n".into(), iss: None, issued_at: now() };
    let env = signed(RELAY_SEED, &m);
    let key: [u8; 32] = pair(RELAY_SEED).public_key().as_ref().try_into().unwrap();
    let other: [u8; 32] = pair(OTHER_SEED).public_key().as_ref().try_into().unwrap();
    assert_eq!(open(&env, &key).unwrap(), m);
    assert!(open(&env, &other).is_err());
    // A signature over the bare payload, without the context, does not verify.
    let bare = Signed { payload: env.payload.clone(), signature: B64.encode(pair(RELAY_SEED).sign(&B64.decode(&env.payload).unwrap()).as_ref()) };
    assert!(open(&bare, &key).is_err());
    assert!(!timely(now() - ISSUED_SKEW_SECS - 1) && timely(now() - ISSUED_SKEW_SECS + 5));
}
