//! Where an app is signed in to: its protected-resource metadata (RFC 9728)
//! and its authorization server's (RFC 8414, else OpenID).

use super::http;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// The well-known address for `suffix` of `base`, path-aware (RFC 8414 §3,
/// RFC 9728 §3): `{origin}/.well-known/{suffix}{path}`, then without the path.
pub(crate) fn well_known(base: &str, suffix: &str) -> Result<Vec<String>> {
    let u = url::Url::parse(base)?;
    let origin = u.origin().ascii_serialization();
    let path = u.path().trim_end_matches('/');
    let mut out = vec![];
    if !path.is_empty() {
        out.push(format!("{origin}/.well-known/{suffix}{path}"));
    }
    out.push(format!("{origin}/.well-known/{suffix}"));
    Ok(out)
}

pub(crate) async fn first_json(c: &reqwest::Client, urls: &[String]) -> Option<Value> {
    for u in urls {
        if let Ok(r) = c.get(u).header("accept", "application/json").send().await {
            if r.status().is_success() {
                if let Ok(v) = r.json::<Value>().await {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// What an app calls itself in its protected-resource metadata
/// (`resource_name`, RFC 9728), from the address its sign-in answer names
/// or the standard one. None when it does not say. (The name a server gives
/// at initialize is often its software's, "StatelessServer", so it is not
/// used.)
pub(crate) async fn own_name(url: &str) -> Option<String> {
    let r = http()
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "magnus", "version": env!("CARGO_PKG_VERSION") } } }))
        .send()
        .await
        .ok()?;
    let named = r.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).and_then(|h| {
        let i = h.find("resource_metadata=\"")? + "resource_metadata=\"".len();
        Some(h[i..].split('"').next()?.to_string())
    });
    // RFC 9728 §3.1: the metadata's standard address is the resource's
    // path under /.well-known/oauth-protected-resource.
    let standard = url::Url::parse(url).ok().map(|u| format!("{}/.well-known/oauth-protected-resource{}", u.origin().ascii_serialization(), u.path().trim_end_matches('/')));
    let urls: Vec<String> = named.into_iter().chain(standard).collect();
    first_json(&http(), &urls).await?["resource_name"].as_str().map(String::from)
}

/// Whether an app asks for a sign-in: an unauthenticated initialize answered
/// with 401. Its protected-resource metadata, when it says where.
pub async fn asks_sign_in(url: &str) -> Result<Option<Option<String>>> {
    let r = http()
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "magnus", "version": env!("CARGO_PKG_VERSION") } } }))
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;
    if r.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    let meta = r.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).and_then(|h| {
        let i = h.find("resource_metadata=\"")? + "resource_metadata=\"".len();
        Some(h[i..].split('"').next()?.to_string())
    });
    Ok(Some(meta))
}

pub(crate) fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The app's authorization server metadata: from its protected-resource
/// metadata (RFC 9728), then the server's own (RFC 8414, else OpenID). The
/// metadata must be for this app and that server (RFC 9728 §3.3, RFC 8414
/// §3.3): metadata for another resource or another issuer is refused, so
/// a sign-in never goes where the app's metadata was not about.
pub(crate) async fn discover(app_url: &str, resource_metadata: Option<String>) -> Result<(Value, Vec<String>, String)> {
    let c = http();
    let prm_urls = match resource_metadata {
        Some(u) => vec![u],
        None => well_known(app_url, "oauth-protected-resource")?,
    };
    let prm = first_json(&c, &prm_urls).await.ok_or_else(|| anyhow!("the app does not say where to sign in (no protected-resource metadata)"))?;
    if let Some(r) = prm["resource"].as_str() {
        if !same_url(r, app_url) {
            bail!("the app's sign-in metadata is for {r}, not {app_url}");
        }
    }
    let issuer = prm["authorization_servers"][0].as_str().ok_or_else(|| anyhow!("the app names no authorization server"))?.to_string();
    let scopes: Vec<String> = prm["scopes_supported"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
    Ok((server_metadata(&issuer).await?, scopes, issuer))
}

/// An authorization server's metadata (RFC 8414, else OpenID), which must
/// name that server as its issuer.
pub(crate) async fn server_metadata(issuer: &str) -> Result<Value> {
    let c = http();
    let mut urls = well_known(&issuer, "oauth-authorization-server")?;
    urls.extend(well_known(&issuer, "openid-configuration")?);
    urls.push(format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/')));
    let asm = first_json(&c, &urls).await.ok_or_else(|| anyhow!("{issuer} publishes no authorization server metadata"))?;
    match asm["issuer"].as_str() {
        Some(i) if crate::issuers::same(i, issuer) => {}
        other => bail!("the authorization server metadata found for {issuer} names issuer {}", other.unwrap_or("none")),
    }
    Ok(asm)
}
