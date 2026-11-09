//! For Google's brokered connectors' tests: a local HTTPS host standing in
//! for a Google API host (the real one needs a person's Google account;
//! each connector's contract test uses the owner's test account), with a
//! certificate for its name from a CA of the test's own, and the appliance
//! running a connector's image as its manifest has it, Alice signed in.

pub use crate::brokered::GmailRequest as Request;
use crate::brokered::{configure, image, read_request, Image, TestCa};
use crate::{reconfigure, FakeOAuthApp};
use magnus_invariants::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// What the API answers a request, given those before it: status, headers
/// and body.
pub type Answer = fn(&Request, &[Request]) -> (u16, String, String);

pub fn ok(v: Value) -> (u16, String, String) {
    (200, String::new(), v.to_string())
}

pub fn not_found() -> (u16, String, String) {
    (404, String::new(), json!({ "error": { "code": 404, "message": "Requested entity was not found.", "status": "NOT_FOUND" } }).to_string())
}

/// The stand-in: where it listens, its CA's certificate (DER, in a file:
/// `[brokered] roots`), each request it was sent, and one answer that may
/// be set ahead of its usual one.
pub struct StandIn {
    pub host: &'static str,
    pub addr: String,
    pub root: std::path::PathBuf,
    pub seen: Arc<Mutex<Vec<Request>>>,
    pub next: Arc<Mutex<Option<(u16, String, String)>>>,
    _ca: TestCa,
}

impl StandIn {
    pub async fn start(host: &'static str, answer: Answer) -> StandIn {
        let ca = TestCa::new("A CA standing in for Google's (test)");
        let (acceptor, root) = (ca.acceptor(host), ca.root.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let seen: Arc<Mutex<Vec<Request>>> = Arc::default();
        let next: Arc<Mutex<Option<(u16, String, String)>>> = Arc::default();
        let (s2, n2) = (seen.clone(), next.clone());
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                let (acceptor, seen, next) = (acceptor.clone(), s2.clone(), n2.clone());
                tokio::spawn(async move {
                    let Ok(mut s) = acceptor.accept(tcp).await else { return };
                    let sni = s.get_ref().1.server_name().unwrap_or_default().to_string();
                    let Some(r) = read_request(&mut s, sni).await else { return };
                    let (status, head, body) = next.lock().unwrap().take().unwrap_or_else(|| answer(&r, &seen.lock().unwrap()));
                    seen.lock().unwrap().push(r);
                    let out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{head}\r\n{body}", body.len());
                    use tokio::io::AsyncWriteExt;
                    let _ = s.write_all(out.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        StandIn { host, addr, root, seen, next, _ca: ca }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }

    /// Each request's method and path (without its query) under `base`.
    pub fn calls(&self, base: &str) -> Vec<(String, String)> {
        self.requests().iter().map(|r| (r.method.clone(), r.path.split('?').next().unwrap().trim_start_matches(base).to_string())).collect()
    }

    /// Answer the next request with this instead.
    pub fn then(&self, status: u16, head: &str, body: Value) {
        *self.next.lock().unwrap() = Some((status, head.to_string(), body.to_string()));
    }
}

/// The catalog's manifest `id`.
pub fn manifest(id: &str) -> toml::Table {
    std::fs::read_to_string(appliance::workspace_root().join(format!("catalog/{id}.toml"))).unwrap().parse().unwrap()
}

/// The appliance with the manifest `id` as the connector `name`, its image
/// built from this checkout as `package`, the API host's address given to
/// the proxy as the stand-in's and the stand-in's CA trusted in the
/// microVM; Alice signed in. The image is kept as long as the appliance
/// runs.
pub async fn signed_in(m: &FakeModel, api: &StandIn, id: &str, package: &str, name: &str) -> (Appliance, FakeOAuthApp, Image) {
    signed_in_at(m, &[api], id, package, name).await
}

/// The same setup for a connector that also needs a provider identity host.
pub async fn signed_in_at(m: &FakeModel, apis: &[&StandIn], id: &str, package: &str, name: &str) -> (Appliance, FakeOAuthApp, Image) {
    let mf = manifest(id);
    let image = image(package);
    assert_eq!(mf["endpoint"].as_str(), Some(image.digest.as_str()), "catalog/{id}.toml names another image than the one built from this commit");
    let hosts: Vec<String> = mf["hosts"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    assert_eq!(hosts, apis.iter().map(|api| api.host).collect::<Vec<_>>());
    let classes: Vec<(String, String)> = mf["tools"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    let tools: Vec<(&str, &str)> = classes.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let provider = FakeOAuthApp::provider_api(3600).await;
    let mut app = Appliance::start(Spec::default(), m).await;
    reconfigure(&mut app, |cfg| {
        configure(cfg, &image, 300, name, &hosts, &provider.base, &tools);
        let b = cfg["brokered"].as_table_mut().unwrap();
        let mut resolve = toml::Table::new();
        for api in apis {
            resolve.insert(format!("{}:443", api.host), api.addr.clone().into());
        }
        b.insert("resolve".into(), resolve.into());
        b.insert("roots".into(), toml::Value::Array(apis.iter().map(|api| api.root.display().to_string().into()).collect()));
        let c = cfg["connectors"].as_array_mut().unwrap().iter_mut().find(|c| c["name"].as_str() == Some(name)).unwrap().as_table_mut().unwrap();
        c.insert("audience".into(), mf["audience"].clone());
        c.insert("scopes".into(), mf["scopes"].clone());
    })
    .await;
    let alice = app.api("alice");
    let (st, v) = alice.post(&format!("/v1/connectors/{name}/sign-in"), json!({ "redirect_uri": "http://127.0.0.1:9/callback" })).await;
    assert!(st.is_success(), "{v}");
    let (code, state) = FakeOAuthApp::browse(v["authorize_url"].as_str().unwrap()).await;
    assert_eq!(alice.post(&format!("/v1/connectors/{name}/sign-in/complete"), json!({ "state": state, "code": code })).await.0, 204);
    (app, provider, image)
}

/// Every request reached the API's name, by TLS and by `Host`, under
/// `base`, as Alice, and the proxy refused nothing.
pub fn only_the_api(app: &Appliance, api: &StandIn, provider: &FakeOAuthApp, base: &str) {
    let seen = api.requests();
    assert!(!seen.is_empty());
    for r in &seen {
        assert_eq!((r.sni.as_str(), r.host.as_str()), (api.host, api.host), "{r:?}");
        assert!(r.path.starts_with(base), "{r:?}");
        assert!(r.auth.strip_prefix("Bearer ").is_some_and(|t| provider.issued(t)), "not Alice's access token: {r:?}");
    }
    assert!(!app.logs().contains("the egress proxy refused"), "the server asked for a host other than {}", api.host);
}

/// The open approval for `task`, once there is one, and which of `tools`
/// it asks about.
pub async fn asked(p: &Api, task: &str, tools: &[&str]) -> (Value, String) {
    let ask = p.wait_item(task, "approval").await;
    let t = ask.to_string();
    let tool = tools.iter().find(|x| t.contains(*x)).unwrap_or_else(|| panic!("{ask}"));
    (ask, tool.to_string())
}

pub async fn approve(p: &Api, ask: &Value) {
    assert!(p.respond(ask["id"].as_str().unwrap(), json!({ "decision": "approve", "scope": "once" })).await.is_success());
}

/// What Magnus was last told in the task that began with `asked`.
pub fn told(m: &FakeModel, asked: &str) -> String {
    m.requests().iter().filter(|c| c.text().contains(asked)).map(|c| c.text()).max_by_key(|t| t.len()).unwrap_or_default()
}
