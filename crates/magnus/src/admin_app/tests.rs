//! The Admin section's guard, against a kernel booted on a fresh database and
//! the app's own API served on a loopback port.

use super::guard::{mark, ADMIN_STEP_UP_SECONDS, STEP_UP_ERROR};
use crate::accounts::Role;
use crate::api::Transport;
use crate::kernel::Kernel;
use axum::Router;
use chrono::Utc;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

pub(super) const PASSWORD: &str = "a-long-admin-password";

pub(super) struct Server {
    pub(super) k: Arc<Kernel>,
    base: String,
    http: reqwest::Client,
    db: String,
    _dir: tempfile::TempDir,
}

fn pg() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

/// A database with the log, the content store and the accounts tables.
async fn database() -> (String, String) {
    let server = pg();
    let name = format!("a_{}", uuid::Uuid::new_v4().simple());
    let admin = crate::log::connect(&server).await.unwrap();
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
    let mut u = url::Url::parse(&server).unwrap();
    u.set_path(&format!("/{name}"));
    let url = u.to_string();
    let c = crate::log::connect(&url).await.unwrap();
    c.batch_execute(
        "CREATE TABLE public.events (seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, at timestamptz NOT NULL DEFAULT now(), kind text NOT NULL, actor text NOT NULL, run_id text, body jsonb NOT NULL);
         CREATE TABLE public.content (seq bigint PRIMARY KEY REFERENCES public.events (seq), salt bytea NOT NULL, body jsonb NOT NULL, person text);",
    )
    .await
    .unwrap();
    c.batch_execute(&crate::accounts::migrate_sql("postgres")).await.unwrap();
    (url, name)
}

/// sam owns the server, ana is an admin by the config's role, lee by the
/// server's own directory, and mo is a member.
/// `extra` adds routes beside the app's, given the kernel.
pub(super) async fn server(extra: impl FnOnce(Arc<Kernel>) -> Router<Arc<Kernel>>) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let (url, db) = database().await;
    let p = dir.path().join("magnus.toml");
    let state = dir.path().join("state");
    std::fs::write(
        &p,
        format!(
            "[appliance]\nstate_dir = {state:?}\nlisten = \"127.0.0.1:0\"\n[database]\nurl = {url:?}\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n\
             [[people]]\nid = \"sam\"\nname = \"Sam\"\nroles = [\"owner\"]\n[[people]]\nid = \"ana\"\nname = \"Ana\"\nroles = [\"admin\"]\n[[people]]\nid = \"mo\"\nname = \"Mo\"\n"
        ),
    )
    .unwrap();
    let k = Kernel::boot(crate::config::Config::load(&p).unwrap()).await.unwrap();
    k.accounts.add_person("lee", "Lee", None).await.unwrap();
    k.accounts.set_role("lee", Role::Admin).await.unwrap();
    for id in ["sam", "ana", "lee", "mo"] {
        k.accounts.set_password(id, PASSWORD).await.unwrap();
    }
    let app = crate::api::router(k.clone(), Transport::Plain).merge(extra(k.clone()).layer(axum::Extension(Transport::Plain)).with_state(k.clone()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app.into_make_service_with_connect_info::<SocketAddr>()).await });
    Server { k, base, http: reqwest::Client::new(), db, _dir: dir }
}

impl Server {
    pub(super) async fn done(self) {
        let admin = crate::log::connect(&pg()).await.unwrap();
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.db)).await.unwrap();
    }

    pub(super) async fn token(&self, person: &str) -> String {
        self.k.accounts.open_session(person, "browser", None).await.unwrap().1
    }

    pub(super) async fn call(&self, token: &str, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method, format!("{}{path}", self.base)).bearer_auth(token);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    pub(super) async fn get(&self, token: &str, path: &str) -> (u16, Value) {
        self.call(token, reqwest::Method::GET, path, None).await
    }

    pub(super) async fn post(&self, token: &str, path: &str, body: Value) -> (u16, Value) {
        self.call(token, reqwest::Method::POST, path, Some(body)).await
    }

    pub(super) fn audited(&self, action: &str) -> Vec<Value> {
        self.k.read(|s| s.audit.iter().map(|(_, _, v)| v.clone()).filter(|v| v["action"] == action).collect())
    }
}

#[test]
fn the_window_is_five_minutes() {
    assert_eq!(ADMIN_STEP_UP_SECONDS, 300);
    let now = Utc::now();
    assert!(super::guard::fresh(now - chrono::Duration::seconds(ADMIN_STEP_UP_SECONDS - 1), now));
    assert!(!super::guard::fresh(now - chrono::Duration::seconds(ADMIN_STEP_UP_SECONDS), now));
    assert!(!super::guard::fresh(now + chrono::Duration::seconds(1), now), "a step-up from the future held");
}

#[test]
fn only_pressing_a_kill_switch_skips_the_step_up() {
    use super::guard::needs_step_up;
    use axum::http::Method;
    assert!(!needs_step_up(&Method::POST, "/v1/admin/kill-switch/press"));
    assert!(!needs_step_up(&Method::POST, "/v1/admin/kill-switch/person/ada/press"));
    assert!(needs_step_up(&Method::POST, "/v1/admin/kill-switch/release"));
    assert!(needs_step_up(&Method::POST, "/v1/admin/kill-switch/person/ada/release"));
    assert!(needs_step_up(&Method::POST, "/v1/admin/people/press"), "a press outside the kill switches skipped the step-up");
    assert!(needs_step_up(&Method::DELETE, "/v1/admin/people/ada"));
    assert!(!needs_step_up(&Method::GET, "/v1/admin/people"));
}

/// A member is refused every admin route; the owner, a config admin and a
/// directory admin all reach them; `standing` tells the app which is which.
#[tokio::test(flavor = "multi_thread")]
async fn only_owners_and_admins_get_in() {
    let s = server(|_| Router::new()).await;
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/admin/kill-switch").await.0, 403, "a member read the kill switch");
    assert_eq!(s.post(&mo, "/v1/admin/kill-switch/press", json!({})).await.0, 403, "a member pressed the kill switch");
    assert_eq!(s.post(&mo, "/v1/admin/step-up", json!({ "password": PASSWORD })).await.0, 403, "a member stepped up");
    assert_eq!(s.get(&mo, "/v1/admin/standing").await.1["admin"], false);
    for id in ["sam", "ana", "lee"] {
        let t = s.token(id).await;
        let (st, v) = s.get(&t, "/v1/admin/kill-switch").await;
        assert_eq!((st, &v["engaged"]), (200, &json!(false)), "{id} could not read the kill switch: {v}");
        assert_eq!(s.get(&t, "/v1/admin/standing").await.1["admin"], true, "{id} is not an admin to the app");
    }
    assert_eq!(s.get("nobody", "/v1/admin/kill-switch").await.0, 401);
    s.done().await;
}

/// A change waits for a step-up no older than the window; a wrong password
/// does not step up and counts toward the lockout sign-in uses.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_needs_a_step_up_within_the_window() {
    // Security's own route (`security::set_policy`), as an admin reaches it.
    let s = server(|_| Router::new()).await;
    let sam = s.token("sam").await;
    let put = |b: Value| s.call(&sam, reqwest::Method::PUT, "/v1/admin/sign-in-policy", Some(b));
    let (st, v) = put(json!({ "code_required": false })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some(STEP_UP_ERROR)), "a change went through without a step-up: {v}");
    assert!(s.audited("sign_in_policy_changed").is_empty(), "a refused change was audited");
    assert_eq!(s.get(&sam, "/v1/admin/sign-in-policy").await.0, 200, "a read asked for a step-up");

    let (st, v) = s.post(&sam, "/v1/admin/step-up", json!({ "password": "not-the-password" })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("wrong")));
    assert_eq!(put(json!({ "code_required": false })).await.0, 403, "a wrong password stepped up");

    let (st, v) = s.post(&sam, "/v1/admin/step-up", json!({ "password": PASSWORD })).await;
    assert_eq!(st, 200, "the right password did not step up: {v}");
    assert!(v["stepped_up_until"].is_string());
    let (st, v) = put(json!({ "code_required": false, "actor": "admin:someone-else" })).await;
    assert_eq!(st, 200, "a stepped-up change was refused: {v}");
    let entry = s.audited("sign_in_policy_changed");
    assert_eq!(entry.len(), 1, "the change was not audited once");
    assert_eq!((entry[0]["admin"].as_str(), entry[0]["target"].as_str()), (Some("sam"), Some("sign-in-policy")));
    assert!(entry[0]["after"].get("actor").is_none(), "the client's actor reached the audit");
    let (st, _) = put(json!({ "code_required": "yes" })).await;
    assert_eq!(st, 400);
    assert_eq!(s.audited("sign_in_policy_changed_failed").len(), 1, "a failed change was not audited as failed");

    // Just inside the window it holds; at its end it does not.
    mark(&sam, Utc::now() - chrono::Duration::seconds(ADMIN_STEP_UP_SECONDS - 5));
    assert_eq!(put(json!({ "code_required": false })).await.0, 200, "a step-up inside the window lapsed");
    mark(&sam, Utc::now() - chrono::Duration::seconds(ADMIN_STEP_UP_SECONDS));
    let (st, v) = put(json!({ "code_required": false })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some(STEP_UP_ERROR)), "a step-up outlived the window");

    // Another session of the same admin has not stepped up.
    let other = s.token("sam").await;
    assert_eq!(s.call(&other, reqwest::Method::PUT, "/v1/admin/sign-in-policy", Some(json!({}))).await.0, 403);

    // Wrong step-ups lock it as wrong sign-ins do.
    let ana = s.token("ana").await;
    for _ in 0..s.k.cfg.sign_in.max_failures {
        let _ = s.post(&ana, "/v1/admin/step-up", json!({ "password": "wrong-password" })).await;
    }
    let (st, v) = s.post(&ana, "/v1/admin/step-up", json!({ "password": PASSWORD })).await;
    assert_eq!((st, v["error"].as_str()), (429, Some("locked")), "wrong step-ups did not count toward the lockout: {v}");
    s.done().await;
}

/// Pressing needs no step-up and names who pulled it to everyone; releasing
/// needs one.
#[tokio::test(flavor = "multi_thread")]
async fn pressing_is_free_and_releasing_steps_up() {
    let s = server(|_| Router::new()).await;
    let lee = s.token("lee").await;
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/stops").await.1["stops"], json!([]));
    let (st, v) = s.post(&lee, "/v1/admin/kill-switch/press", json!({})).await;
    assert_eq!(st, 200, "pressing asked for more: {v}");
    assert!(s.k.read(|st| st.kill_switch), "the switch is not on");
    assert_eq!(s.k.read(|st| st.kill_switch_by.clone().map(|b| b.0)), Some("admin:lee".into()), "the switch names the wrong actor");
    assert_eq!(s.audited("kill_switch_engaged").len(), 1, "pressing was not audited");

    let stops = s.get(&mo, "/v1/stops").await.1;
    assert_eq!((stops["stops"][0]["level"].as_str(), stops["stops"][0]["by_name"].as_str()), (Some("server"), Some("Lee")), "the banner does not name who: {stops}");
    assert!(stops["stops"][0]["at"].is_string());
    assert_eq!(s.get(&lee, "/v1/admin/kill-switch").await.1["by_name"], "Lee");

    let (st, v) = s.post(&lee, "/v1/admin/kill-switch/release", json!({})).await;
    assert_eq!((st, v["error"].as_str()), (403, Some(STEP_UP_ERROR)), "releasing skipped the step-up: {v}");
    assert!(s.k.read(|st| st.kill_switch), "a refused release released");
    assert_eq!(s.post(&lee, "/v1/admin/step-up", json!({ "password": PASSWORD })).await.0, 200);
    let (st, v) = s.post(&lee, "/v1/admin/kill-switch/release", json!({})).await;
    assert_eq!(st, 200, "a stepped-up release was refused: {v}");
    assert!(!s.k.read(|st| st.kill_switch), "the switch stayed on");
    assert_eq!(s.audited("kill_switch_released").len(), 1);
    assert_eq!(s.get(&mo, "/v1/stops").await.1["stops"], json!([]));
    s.done().await;
}
