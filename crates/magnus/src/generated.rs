//! Connectors Magnus builds for a person: a declared set of HTTP calls to one
//! address, run here and never as code, so nothing the agent wrote comes near
//! a token. Each is one person's, listed with their other connectors, and
//! goes through the same checks. A copy is kept as a file on their volume;
//! the agent can edit that copy, so what runs is the one approved in the log.
use crate::api::{err, who};
use crate::apps::CallError;
use crate::boundary::Action;
use crate::computer::{Owner, Role};
use crate::config::{Connector, ToolClass};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use base64::Engine;
use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

const SCHEME: &str = "generated:";
const FOLDER: &str = "/workspace/connectors";
const MAX_DEFINITION_BYTES: usize = 64 * 1024;
const MAX_TOOLS: usize = 32;
const MAX_TITLE_CHARS: usize = 60;
const MAX_ANSWER_BYTES: usize = 1 << 20;
const MAX_TOKEN_BYTES: usize = 8 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
/// What it returns may be anything, and what it is sent can reach anyone.
const LABEL: &str = "private+untrusted";
const AUDIENCE: &str = "public";

pub const BUILTINS: &[Builtin] = &[Builtin { name: "connector_build", app: "connector", risk: Risk::Irreversible, label: Label::PRINCIPAL, audience: Audience::Private }];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub title: String,
    pub description: String,
    pub base_url: String,
    /// Whether calls carry a token the person supplies.
    #[serde(default)]
    pub token: bool,
    pub tools: Vec<DefinedTool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DefinedTool {
    pub name: String,
    pub description: String,
    pub method: String,
    /// Starts with `/`; `{name}` takes that argument.
    pub path: String,
    #[serde(default = "any_object")]
    pub input_schema: Value,
}

fn any_object() -> Value {
    json!({ "type": "object" })
}

pub fn specs() -> Vec<ToolSpec> {
    let definition = json!({ "type": "object", "description": "title, description, base_url (https), token (true when calls need a token the person supplies; never put one here), and tools: each with name, description, method, path (with {argument} placeholders) and input_schema" });
    vec![ToolSpec {
        name: "connector_build".into(),
        description:
            "Build a connector for this person from HTTP calls to one address. They approve it, it is listed with their connectors, and each call goes through the usual checks."
                .into(),
        input_schema: obj(json!({ "definition": definition }), &["definition"]),
    }]
}

/// Whether this person may use this connector at all.
pub fn may_use(person: &crate::config::Person, c: &Connector) -> bool {
    let theirs = c.owner.as_ref().is_none_or(|o| *o == person.id);
    let allowed = c.access_groups.is_empty() || c.access_groups.iter().any(|g| person.groups.contains(g));
    theirs && allowed && !c.blocked_groups.iter().any(|g| person.groups.contains(g))
}

fn checked_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("__") && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl Definition {
    fn check(&self, allow_private: bool) -> Result<(), String> {
        let chars = self.title.trim().chars().count();
        if chars == 0 || chars > MAX_TITLE_CHARS {
            return Err(format!("the title must be 1 to {MAX_TITLE_CHARS} characters"));
        }
        let url = url::Url::parse(&self.base_url).map_err(|_| "base_url is not a URL".to_string())?;
        if url.scheme() != "https" && !(url.scheme() == "http" && allow_private) {
            return Err("base_url must be https".into());
        }
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
            return Err("base_url takes a host and path only: no user, password, query or fragment".into());
        }
        if self.tools.is_empty() || self.tools.len() > MAX_TOOLS {
            return Err(format!("name between 1 and {MAX_TOOLS} tools"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &self.tools {
            if !checked_name(&t.name) || !seen.insert(&t.name) {
                return Err(format!("{} is not a usable tool name: lower-case letters, digits and single underscores, each name once", t.name));
            }
            if !METHODS.contains(&t.method.as_str()) {
                return Err(format!("{}: method must be one of {}", t.name, METHODS.join(", ")));
            }
            if !t.path.starts_with('/') || t.path.contains("..") || t.path.contains(['?', '#', '\\']) {
                return Err(format!("{}: path starts with / and has no .., query or fragment", t.name));
            }
        }
        match serde_json::to_vec(self) {
            Ok(b) if b.len() <= MAX_DEFINITION_BYTES => Ok(()),
            _ => Err(format!("the definition is over {MAX_DEFINITION_BYTES} bytes")),
        }
    }

    fn encoded(&self) -> Result<String> {
        Ok(format!("{SCHEME}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?)))
    }

    fn decoded(url: &str) -> Option<Definition> {
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(url.strip_prefix(SCHEME)?).ok()?;
        serde_json::from_slice(&body).ok()
    }
}

pub fn is_generated(url: &str) -> bool {
    url.starts_with(SCHEME)
}

/// Unique across people, since connector names are; readable for its person.
fn connector_name(person: &str, title: &str) -> String {
    let slug: String = title.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    format!("{slug}-{}", &hex::encode(Sha256::digest(person.as_bytes()))[..8])
}

fn connector_of(person: &str, d: &Definition) -> Result<Connector> {
    let name = connector_name(person, &d.title);
    let tools = d.tools.iter().map(|t| {
        let risk = if t.method == "GET" { "read" } else { "irreversible" };
        (t.name.clone(), ToolClass { risk: risk.into(), label: None, reach: None })
    });
    Ok(Connector {
        owner: Some(person.to_string()),
        filter_login_codes: false,
        credential: d.token.then(|| crate::connectors::credential_name(&name)),
        name,
        transport: crate::transport::MCP_HTTP.into(),
        url: d.encoded()?,
        audience: AUDIENCE.into(),
        label: LABEL.into(),
        access_groups: vec![],
        blocked_groups: vec![],
        idempotency: "none".into(),
        tools: tools.collect(),
        auth: None,
        client_id: None,
        system_of_record: false,
        scopes: vec![],
        registration: None,
        title: Some(d.title.trim().to_string()),
        summary: Some(d.description.clone()),
        catalog: false,
        hosts: vec![],
        issuer: None,
        pkce: true,
        redirect_uri: None,
        handoff: None,
        destinations: Default::default(),
        sent: Default::default(),
        read_back: Default::default(),
        watch: vec![],
    })
}

fn encoded_arg(v: &Value) -> Option<String> {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    Some(url::form_urlencoded::byte_serialize(text.as_bytes()).collect())
}

/// The address a call goes to, and the arguments its path did not take.
fn address(d: &Definition, t: &DefinedTool, args: &Value) -> Result<(url::Url, serde_json::Map<String, Value>), String> {
    let mut rest = args.as_object().cloned().unwrap_or_default();
    let mut path = t.path.clone();
    while let Some(open) = path.find('{') {
        let close = path[open..].find('}').map(|n| n + open).ok_or("an unclosed placeholder")?;
        let name = path[open + 1..close].to_string();
        let value = rest.remove(&name).as_ref().and_then(encoded_arg).ok_or(format!("{name} is needed, as text or a number"))?;
        path.replace_range(open..=close, &value);
    }
    let url = url::Url::parse(&format!("{}{path}", d.base_url.trim_end_matches('/'))).map_err(|_| "not a usable address".to_string())?;
    let base = url::Url::parse(&d.base_url).map_err(|_| "not a usable address".to_string())?;
    if url.origin() != base.origin() || !url.path().starts_with(base.path().trim_end_matches('/')) {
        return Err("that leaves the connector's address".into());
    }
    Ok((url, rest))
}

async fn pinned_client(url: &url::Url, allow_private: bool) -> Result<reqwest::Client, CallError> {
    let host = url.host_str().ok_or_else(|| CallError::NotSent("no host".into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let found: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port)).await.map_err(|e| CallError::NotSent(format!("could not find {host}: {e}")))?.collect();
    if found.is_empty() || (!allow_private && found.iter().any(|a| crate::web::private_ip(a.ip()))) {
        return Err(CallError::NotSent(format!("{host} is not an address a connector may reach")));
    }
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(CALL_TIMEOUT)
        .resolve_to_addrs(host, &found)
        .build()
        .map_err(|e| CallError::NotSent(e.to_string()))
}

fn failure(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": true })
}

async fn limited_text(mut resp: reqwest::Response) -> Result<String, CallError> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| CallError::Unknown(format!("response lost: {e}")))? {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_ANSWER_BYTES {
            body.truncate(MAX_ANSWER_BYTES);
            break;
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn call(d: &Definition, bearer: Option<&str>, params: &Value, allow_private: bool) -> Result<Value, CallError> {
    let Some(t) = d.tools.iter().find(|t| Some(t.name.as_str()) == params["name"].as_str()) else {
        return Err(CallError::NotSent("no such tool".into()));
    };
    let (mut url, rest) = match address(d, t, &params["arguments"]) {
        Ok(a) => a,
        Err(why) => return Ok(failure(why)),
    };
    let reads = matches!(t.method.as_str(), "GET" | "DELETE");
    if reads {
        for (k, v) in &rest {
            if let Some(value) = encoded_arg(v) {
                url.query_pairs_mut().append_key_only(k).finish();
                let full = url.query().unwrap_or_default().to_string();
                url.set_query(Some(&format!("{full}={value}")));
            }
        }
    }
    let client = pinned_client(&url, allow_private).await?;
    let method = reqwest::Method::from_bytes(t.method.as_bytes()).map_err(|e| CallError::NotSent(e.to_string()))?;
    let mut req = client.request(method, url);
    if let Some(b) = bearer {
        req = req.bearer_auth(b);
    }
    if let Some(k) = params["_meta"]["io.magnus/idempotency-key"].as_str() {
        req = req.header("idempotency-key", k);
    }
    if !reads {
        req = req.json(&Value::Object(rest));
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() {
            CallError::NotSent(format!("could not connect: {e}"))
        } else {
            CallError::Unknown(format!("request failed after sending: {e}"))
        }
    })?;
    let status = resp.status();
    let text = limited_text(resp).await?;
    Ok(if status.is_success() { json!({ "content": [{ "type": "text", "text": text }] }) } else { failure(format!("HTTP {}: {text}", status.as_u16())) })
}

/// A generated connector, answering as an app would.
pub(crate) async fn answer(url: &str, bearer: Option<&str>, method: &str, params: Value, allow_private: bool) -> Result<Option<Value>, CallError> {
    let d = Definition::decoded(url).ok_or_else(|| CallError::NotSent("this connector's definition cannot be read".into()))?;
    match method {
        "tools/list" => {
            let tools: Vec<Value> = d
                .tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.input_schema, "annotations": { "readOnlyHint": t.method == "GET" } }))
                .collect();
            Ok(Some(json!({ "tools": tools })))
        }
        "tools/call" => call(&d, bearer, &params, allow_private).await.map(Some),
        _ => Ok(None),
    }
}

impl Kernel {
    pub(crate) async fn connector_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let person = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone())).context("no such run")?;
        let d: Definition = match serde_json::from_value(a.input["definition"].clone()) {
            Ok(d) => d,
            Err(e) => return Ok((format!("That is not a connector definition: {e}"), true, None)),
        };
        if let Err(why) = d.check(self.cfg.web.allow_private_networks) {
            return Ok((format!("That connector cannot be built: {why}."), true, None));
        }
        let c = connector_of(&person, &d)?;
        if self.all_connectors().iter().any(|(known, _)| known.name == c.name) {
            return Ok(("This person already has a connector with that title. They can delete it first.".into(), true, None));
        }
        let (name, token) = (c.name.clone(), d.token);
        self.emit(&format!("kernel:connector:{person}"), Ev::ConnectorAdded { connector: c }).await?;
        self.sync_definitions(&person).await;
        let next = if token { "It needs a token, which the person adds under Connectors. Do not ask them for it in chat." } else { "It is ready." };
        Ok((format!("Built the connector {name}. {next}"), false, Some(json!({ "connector": name }))))
    }

    /// Their volume holds exactly their connectors' definitions, as approved.
    async fn sync_definitions(&self, person: &str) {
        let computer = self.read(|s| s.computers.all().find(|c| c.role == Role::Computer && matches!(&c.owner, Owner::Person(p) if p == person)).cloned());
        let Some(computer) = computer else { return };
        let mine: Vec<(String, String)> = self
            .all_connectors()
            .into_iter()
            .filter(|(c, _)| c.owner.as_deref() == Some(person))
            .filter_map(|(c, _)| Some((format!("{}.json", c.name), serde_json::to_string_pretty(&Definition::decoded(&c.url)?).ok()?)))
            .collect();
        if let Ok(listed) = self.computer_file_for(&computer, json!({ "op": "list", "path": FOLDER })).await {
            for e in listed["entries"].as_array().into_iter().flatten() {
                let name = e["name"].as_str().unwrap_or_default();
                if !mine.iter().any(|(file, _)| file == name) {
                    let _ = self.computer_file_for(&computer, json!({ "op": "erase", "path": format!("{FOLDER}/{name}") })).await;
                }
            }
        }
        for (file, body) in mine {
            let data_b64 = base64::engine::general_purpose::STANDARD.encode(body);
            if let Err(e) = self.computer_file_for(&computer, json!({ "op": "write", "path": format!("{FOLDER}/{file}"), "data_b64": data_b64 })).await {
                tracing::warn!("keeping {file} on the volume: {e:?}");
            }
        }
    }

    fn generated_of(&self, person: &str, name: &str) -> Option<Connector> {
        self.read(|s| s.connectors.get(name).filter(|c| c.owner.as_deref() == Some(person)).cloned())
    }

    /// Its token and grants revoked, its rules removed, its file gone.
    /// Grants and rules go even when the token cannot be revoked, since
    /// taking a permission away is always safe; the connector itself stays
    /// then, with the reason, so the token is not lost from view and the
    /// person can try again.
    async fn delete_generated(&self, person: &str, c: &Connector) -> Result<usize> {
        let revoked = token_revoked(self.revoke_all(c).await);
        let actor = format!("person:{person}");
        let (grants, rules) = self.read(|s| {
            let grants: Vec<String> = s.grants.values().filter(|g| g.person == person && g.revoked.is_none() && g.apps.contains(&c.name)).map(|g| g.id.clone()).collect();
            let rules: Vec<String> = s.rules.of(person).into_iter().filter(|r| r.compiled.as_ref().is_some_and(|x| x.connector == c.name)).map(|r| r.id).collect();
            (grants, rules)
        });
        for id in grants {
            if let Err(e) = self.emit(&actor, Ev::GrantRevoked { id: id.clone(), person: person.to_string() }).await {
                tracing::error!("deleting {}: grant {id} was not revoked: {e:#}", c.name);
            }
        }
        for id in rules {
            if let Err(e) = self.emit(&actor, Ev::Rule(crate::rules::Change::Removed { id: id.clone() })).await {
                tracing::error!("deleting {}: rule {id} was not removed: {e:#}", c.name);
            }
        }
        let revoked = revoked?;
        self.emit(&actor, Ev::ConnectorRemoved { name: c.name.clone() }).await?;
        self.sync_definitions(person).await;
        Ok(revoked)
    }
}

/// The tokens revoked, or why they could not be, with the vault's cause.
fn token_revoked(r: Result<usize, crate::connectors::Refusal>) -> Result<usize> {
    r.map_err(|(_, why)| anyhow::anyhow!("its token could not be revoked: {why}"))
}

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/connectors/{name}/definition", get(definition))
        .route("/v1/connectors/{name}/generated", delete(remove))
        .route("/v1/connectors/{name}/token", put(set_token))
}

async fn definition(State(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.generated_of(&p.id, &name).and_then(|c| Definition::decoded(&c.url)) {
        Some(d) => Json(d).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such connector of yours"),
    }
}

async fn remove(State(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(c) = k.generated_of(&p.id, &name) else { return err(StatusCode::NOT_FOUND, "no such connector of yours") };
    match k.delete_generated(&p.id, &c).await {
        Ok(n) => Json(json!({ "tokens_revoked": n })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

async fn set_token(State(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(c) = k.generated_of(&p.id, &name) else { return err(StatusCode::NOT_FOUND, "no such connector of yours") };
    let Some(credential) = &c.credential else { return err(StatusCode::CONFLICT, "this connector takes no token") };
    let token = b["token"].as_str().map(str::trim).unwrap_or_default();
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
        return err(StatusCode::BAD_REQUEST, "give the token");
    }
    match k.vault.put(credential, token) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weather() -> Definition {
        Definition {
            title: "Weather".into(),
            description: "Forecasts".into(),
            base_url: "https://api.example.com/v2".into(),
            token: true,
            tools: vec![DefinedTool {
                name: "forecast".into(),
                description: "A forecast".into(),
                method: "GET".into(),
                path: "/places/{city}/forecast".into(),
                input_schema: any_object(),
            }],
        }
    }

    #[test]
    fn a_token_that_could_not_be_revoked_says_why() {
        let e = token_revoked(Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, "the vault is locked".into()))).unwrap_err();
        assert!(format!("{e:#}").contains("the vault is locked"), "the cause was dropped: {e:#}");
        assert_eq!(token_revoked(Ok(2)).unwrap(), 2);
    }

    #[test]
    fn a_definition_round_trips_through_its_address() {
        let d = weather();
        let Ok(url) = d.encoded() else { panic!("not encoded") };
        assert!(is_generated(&url));
        assert_eq!(Definition::decoded(&url), Some(d));
    }

    #[test]
    fn it_must_be_https_with_no_key_in_the_address() {
        let mut d = weather();
        assert_eq!(d.check(false), Ok(()));
        d.base_url = "http://api.example.com".into();
        assert!(d.check(false).is_err());
        assert!(d.check(true).is_ok(), "a test server may use http");
        d.base_url = "https://user:secret@api.example.com".into();
        assert!(d.check(false).is_err());
        d.base_url = "https://api.example.com?key=abc".into();
        assert!(d.check(false).is_err());
    }

    #[test]
    fn a_tool_path_stays_under_the_address() {
        let mut d = weather();
        d.tools[0].path = "/../admin".into();
        assert!(d.check(false).is_err());
        d = weather();
        let Ok((url, _)) = address(&d, &d.tools[0], &json!({ "city": "a/../../evil?x=1" })) else { panic!("no address") };
        assert_eq!(url.host_str(), Some("api.example.com"));
        assert!(url.path().starts_with("/v2/places/"), "an argument left the address: {url}");
        assert!(!url.as_str().contains("/evil?"), "an argument became a path: {url}");
    }

    #[test]
    fn a_get_only_reads_and_anything_else_is_irreversible() {
        let mut d = weather();
        d.tools.push(DefinedTool { name: "post_note".into(), description: "x".into(), method: "POST".into(), path: "/notes".into(), input_schema: any_object() });
        let Ok(c) = connector_of("alice", &d) else { panic!("no connector") };
        assert_eq!(c.tools["forecast"].risk, "read");
        assert_eq!(c.tools["post_note"].risk, "irreversible");
        assert_eq!(c.owner.as_deref(), Some("alice"));
        assert!(c.credential.is_some(), "a token connector keeps its token in the vault");
    }

    #[tokio::test]
    async fn a_private_address_is_refused_unless_the_appliance_allows_it() {
        let Ok(url) = url::Url::parse("https://127.0.0.1:9/") else { panic!("no url") };
        assert!(pinned_client(&url, false).await.is_err());
        assert!(pinned_client(&url, true).await.is_ok());
    }

    #[test]
    fn two_people_can_use_the_same_title() {
        assert_ne!(connector_name("alice", "Weather"), connector_name("bob", "Weather"));
    }
}
