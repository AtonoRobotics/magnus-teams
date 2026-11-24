//! Agent cards (A2A 1.0): an outside agent's, fetched at connect and
//! trusted only once its signature verifies against its key set; and the
//! server's own, which lists only the skills policy exposes.

use super::jws;
use super::ledger::Scheme;
use crate::egress_client::Egress;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// The largest card or key set read, in bytes.
pub const MAX_CARD_BYTES: usize = 256 * 1024;
/// Where the server's card is published (A2A 1.0, Discovery).
pub const CARD_PATH: &str = "/.well-known/agent-card.json";
/// Where the server's card signing keys are published (its `jku`).
pub const KEYS_PATH: &str = "/.well-known/agent-card-keys.json";
/// The server's JSON-RPC endpoint for outside agents.
pub const ENDPOINT_PATH: &str = "/a2a";
/// The header an outside agent sends its key in, as the card declares.
pub const KEY_HEADER: &str = "x-magnus-agent-key";
/// The name of that scheme in the card (written out in `ours`).
#[cfg(test)]
const KEY_SCHEME: &str = "magnus_agent_key";
/// The skills the server can expose: asking one person, asking a shared project.
pub const ASK_PERSON: &str = "ask_person";
pub const ASK_PROJECT: &str = "ask_project";

/// What an outside agent's verified card says.
#[derive(Debug, Clone)]
pub struct Verified {
    pub name: String,
    pub description: String,
    pub endpoint: String,
    pub scheme: Scheme,
    pub skills: Vec<String>,
    pub key_id: String,
}

/// An address an outside agent named: https, or http where private
/// networks are allowed (tests and company setups); no user, query or fragment.
pub fn checked_url(raw: &str, allow_http: bool) -> Result<url::Url> {
    let u = url::Url::parse(raw.trim()).with_context(|| format!("{raw} is not an address"))?;
    let scheme_ok = u.scheme() == "https" || (allow_http && u.scheme() == "http");
    if !scheme_ok || u.host_str().is_none() || !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        bail!("{raw} is not an https address an agent card can be at");
    }
    Ok(u)
}

async fn get_json(e: &Egress, url: &str) -> Result<Value> {
    let resp = e.send(reqwest::Method::GET, url, |r| r.header(reqwest::header::ACCEPT, "application/json")).await?;
    if !resp.status().is_success() {
        bail!("{url} answered {}", resp.status());
    }
    let body = resp.bytes().await?;
    if body.len() > MAX_CARD_BYTES {
        bail!("{url} is larger than an agent card may be");
    }
    serde_json::from_slice(&body).with_context(|| format!("{url} is not JSON"))
}

/// Fetch the card at `card_url` and verify it: at least one signature
/// whose key, from a key set on the card's own host, matches its contents.
pub async fn fetch(e: &Egress, card_url: &str, allow_http: bool) -> Result<Verified> {
    let at = checked_url(card_url, allow_http)?;
    let card = get_json(e, at.as_str()).await?;
    let sigs = card["signatures"].as_array().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("the agent card is not signed, so who published it cannot be checked"))?;
    let mut why = String::new();
    let mut key_id = None;
    for sig in sigs {
        match verify_one(e, &card, sig, &at, allow_http).await {
            Ok(kid) => {
                key_id = Some(kid);
                break;
            }
            Err(err) => why = format!("{err:#}"),
        }
    }
    let key_id = key_id.ok_or_else(|| anyhow!("the agent card's signature does not verify: {why}"))?;
    read(&card, allow_http, key_id)
}

async fn verify_one(e: &Egress, card: &Value, sig: &Value, at: &url::Url, allow_http: bool) -> Result<String> {
    let h = jws::header(sig)?;
    let jku = h.jku.as_deref().ok_or_else(|| anyhow!("its signature names no key set (jku)"))?;
    let keys_at = checked_url(at.join(jku)?.as_ref(), allow_http)?;
    if keys_at.host_str() != at.host_str() {
        bail!("its key set is on {}, not on the card's own host", keys_at.host_str().unwrap_or_default());
    }
    let set = get_json(e, keys_at.as_str()).await?;
    let jwk =
        set["keys"].as_array().and_then(|ks| ks.iter().find(|k| k["kid"].as_str() == Some(h.kid.as_str()))).ok_or_else(|| anyhow!("its key {} is not in its key set", h.kid))?;
    jws::verify(card, sig, jwk)?;
    Ok(h.kid)
}

/// What a verified card says: its JSON-RPC endpoint for protocol 1.0, and
/// the first security scheme it requires that Magnus can use.
pub fn read(card: &Value, allow_http: bool, key_id: String) -> Result<Verified> {
    let name = card["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| anyhow!("the agent card has no name"))?;
    let endpoint = card["supportedInterfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["protocolBinding"].as_str().is_some_and(|b| b.eq_ignore_ascii_case("JSONRPC")) && i["protocolVersion"].as_str().is_none_or(|v| v.starts_with("1.")))
        .and_then(|i| i["url"].as_str())
        .ok_or_else(|| anyhow!("the agent card declares no JSON-RPC endpoint for A2A 1.0"))?;
    let endpoint = checked_url(endpoint, allow_http)?.to_string();
    let skills = card["skills"].as_array().into_iter().flatten().filter_map(|s| s["name"].as_str().or(s["id"].as_str()).map(String::from)).collect();
    Ok(Verified { name: name.to_string(), description: card["description"].as_str().unwrap_or_default().to_string(), endpoint, scheme: scheme(card)?, skills, key_id })
}

/// The scheme the card's security requirements choose: the first that
/// Magnus can use, among API key in a header, HTTP bearer, OAuth 2 client
/// credentials and mutual TLS. A card that requires none needs nothing.
fn scheme(card: &Value) -> Result<Scheme> {
    let schemes = card["securitySchemes"].as_object();
    let required: Vec<String> = card["securityRequirements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["schemes"].as_object())
        .flat_map(|m| m.keys().cloned())
        .chain(schemes.into_iter().flat_map(|m| m.keys().cloned()))
        .collect();
    if required.is_empty() {
        return Ok(Scheme::None);
    }
    for name in &required {
        let Some(s) = schemes.and_then(|m| m.get(name)) else { continue };
        if let Some(k) = s.get("apiKeySecurityScheme") {
            if k["location"] == "header" {
                if let Some(h) = k["name"].as_str().filter(|h| !h.is_empty()) {
                    return Ok(Scheme::ApiKey { header: h.to_string() });
                }
            }
        } else if let Some(h) = s.get("httpAuthSecurityScheme") {
            if h["scheme"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("bearer")) {
                return Ok(Scheme::Bearer);
            }
        } else if let Some(o) = s.get("oauth2SecurityScheme") {
            if let Some(cc) = o["flows"].get("clientCredentials") {
                if let Some(token_url) = cc["tokenUrl"].as_str() {
                    let scopes = cc["scopes"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                    return Ok(Scheme::OAuth2 { token_url: token_url.to_string(), scopes });
                }
            }
        } else if s.get("mtlsSecurityScheme").is_some() {
            return Ok(Scheme::MutualTls);
        }
    }
    bail!("the agent card requires a way of signing in Magnus cannot use (it takes an API key in a header, a bearer token, OAuth 2 client credentials or a client certificate)")
}

/// The server's own card, unsigned: one JSON-RPC endpoint, the key scheme
/// outside agents authenticate with, and only the skills policy exposes
/// (`person`: some agent is admitted for a person; `project`: for a shared project).
pub fn ours(endpoint: &str, person: bool, project: bool) -> Value {
    let mut skills = vec![];
    if person {
        skills.push(json!({
            "id": ASK_PERSON, "name": "Ask a person",
            "description": "Leave a request for one person. Name them in the request's metadata as {\"to\": {\"person\": \"<id>\"}}. It waits in their inbox until they take it up, and the reply is sent once they release it.",
            "tags": ["request"]
        }));
    }
    if project {
        skills.push(json!({
            "id": ASK_PROJECT, "name": "Ask a shared project",
            "description": "Post a request into a shared project you are a guest of. Name it in the request's metadata as {\"to\": {\"project\": \"<id>\"}}. Its members read it; GetTask returns what they post in reply.",
            "tags": ["request"]
        }));
    }
    json!({
        "name": "Magnus",
        "description": "The people and shared projects on this server, for agents they have admitted.",
        "supportedInterfaces": [{ "url": endpoint, "protocolBinding": "JSONRPC", "protocolVersion": super::PROTOCOL_VERSION }],
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": { "streaming": false, "pushNotifications": false },
        "securitySchemes": { "magnus_agent_key": { "apiKeySecurityScheme": { "location": "header", "name": KEY_HEADER, "description": "The key the person who admitted you gave you." } } },
        "securityRequirements": [{ "schemes": { "magnus_agent_key": { "list": [] } } }],
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": skills,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The server's card lists a skill only once some agent is admitted
    /// for it, and always says how to authenticate.
    #[test]
    fn the_servers_card_lists_only_the_skills_policy_exposes() {
        let none = ours("https://m.example/a2a", false, false);
        assert_eq!(none["skills"], json!([]));
        assert_eq!(none["securitySchemes"][KEY_SCHEME]["apiKeySecurityScheme"]["name"], KEY_HEADER);
        let person = ours("https://m.example/a2a", true, false);
        let ids: Vec<&str> = person["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec![ASK_PERSON]);
        let read_back = read(&ours("https://m.example/a2a", true, true), false, "k".into()).unwrap();
        assert_eq!(read_back.endpoint, "https://m.example/a2a");
        assert_eq!(read_back.scheme, Scheme::ApiKey { header: KEY_HEADER.into() });
    }

    /// A card's scheme follows its requirements: bearer, OAuth 2 client
    /// credentials, mutual TLS; one Magnus cannot use is refused.
    #[test]
    fn the_cards_security_schemes_choose_the_authentication() {
        let with = |s: Value| json!({ "securitySchemes": { "s": s }, "securityRequirements": [{ "schemes": { "s": { "list": [] } } }] });
        assert_eq!(scheme(&with(json!({ "httpAuthSecurityScheme": { "scheme": "Bearer" } }))).unwrap(), Scheme::Bearer);
        assert_eq!(scheme(&with(json!({ "mtlsSecurityScheme": {} }))).unwrap(), Scheme::MutualTls);
        let oauth = with(json!({ "oauth2SecurityScheme": { "flows": { "clientCredentials": { "tokenUrl": "https://a.example/token", "scopes": { "tasks": "" } } } } }));
        assert_eq!(scheme(&oauth).unwrap(), Scheme::OAuth2 { token_url: "https://a.example/token".into(), scopes: vec!["tasks".into()] });
        assert!(scheme(&with(json!({ "openIdConnectSecurityScheme": { "openIdConnectUrl": "https://a.example" } }))).is_err());
        assert_eq!(scheme(&json!({})).unwrap(), Scheme::None);
    }
}
