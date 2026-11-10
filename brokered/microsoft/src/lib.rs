//! What Microsoft's brokered connectors share (connectors spec, Brokered
//! API connectors): one request to Microsoft Graph (`graph.microsoft.com`)
//! as the person, over the kit's way out (TLS checked for Graph's name,
//! through the appliance's egress proxy), and Graph's refusals in words a
//! person can act on. A request that may have changed something and has no
//! clear answer has an unknown outcome.

use magnus_brokered_kit::{https, send, Body, Failure, Response};
use serde_json::Value;

/// Microsoft Graph, the one host every Microsoft connector reaches.
pub const HOST: &str = "graph.microsoft.com";
/// Graph's address, as its answers name the next page.
const ORIGIN: &str = "https://graph.microsoft.com";

/// One connector's use of Graph: its name as a person knows it, and the
/// scopes its tools work with.
pub struct Graph {
    pub name: &'static str,
    pub scopes: &'static [&'static str],
}

impl Graph {
    /// One request as the person: its JSON answer (`{}` for none), or
    /// what the refusal means. `path` starts with Graph's version.
    pub fn call(&self, method: &str, path: &str, token: &str, body: Option<&Value>) -> Result<Value, Failure> {
        let data = body.map(Value::to_string);
        let wrote = method != "GET";
        let r = self.request(method, path, token, "application/json", data.as_deref().map(|d| Body { kind: "application/json", bytes: d.as_bytes() }), wrote)?;
        self.json(&r.body, wrote)
    }

    /// One request whose body or answer is a file's bytes.
    pub fn bytes(&self, method: &str, path: &str, token: &str, body: Option<Body>) -> Result<Response, Failure> {
        self.request(method, path, token, "*/*", body, method != "GET")
    }

    fn request(&self, method: &str, path: &str, token: &str, accept: &str, body: Option<Body>, wrote: bool) -> Result<Response, Failure> {
        let name = self.name;
        let s = https(&format!("{HOST}:443")).map_err(|e| format!("{e}. Nothing was changed."))?;
        let r = send(s, method, HOST, path, token, accept, body).map_err(|e| {
            if wrote {
                Failure::Unknown(format!("{e}: whether {name} did this is unknown"))
            } else {
                Failure::NotDone(e)
            }
        })?;
        if r.status >= 400 {
            return Err(self.explain(&r, wrote));
        }
        Ok(r)
    }

    /// An answer of success as JSON (`{}` for none). One that is not JSON
    /// says nothing of what was done, so after a write it is an unknown
    /// outcome: tried again, it could do the write twice.
    pub fn json(&self, body: &[u8], wrote: bool) -> Result<Value, Failure> {
        let text = String::from_utf8_lossy(body).to_string();
        if text.trim().is_empty() {
            return Ok(Value::Object(Default::default()));
        }
        serde_json::from_str(&text).map_err(|_| {
            let what = format!("{}'s answer was not JSON: {}", self.name, clip(&text));
            if wrote {
                Failure::Unknown(format!("{what}; whether it did this is unknown"))
            } else {
                Failure::NotDone(what)
            }
        })
    }

    /// What a refusal means (Microsoft Graph, "Microsoft Graph error
    /// responses and resource types").
    pub fn explain(&self, r: &Response, wrote: bool) -> Failure {
        let name = self.name;
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        let code = v["error"]["code"].as_str().unwrap_or_default();
        let message = v["error"]["message"].as_str().unwrap_or_default().trim();
        let wait = r.header("retry-after").map(|w| format!(" (in {w} seconds)")).unwrap_or_default();
        if r.status == 401 {
            return Failure::NotDone(format!("{name} rejected the sign-in: it has expired or was revoked. Sign in again. Nothing was changed."));
        }
        if code == "MailboxNotEnabledForRESTAPI" || code == "MailboxNotSupportedForRESTAPI" {
            return Failure::NotDone(format!("This Microsoft account has no Exchange Online mailbox, so {name} has nothing to reach. Nothing was changed."));
        }
        if r.status == 403 && matches!(code, "ErrorAccessDenied" | "Authorization_RequestDenied" | "accessDenied" | "AccessDenied") {
            return Failure::NeedsScope(
                format!("This sign-in does not include the access {name} needs for this, or the organisation does not allow it. Sign in again and allow it; if it is still refused, an admin of the organisation allows it. Nothing was changed."),
                self.scopes.iter().map(|s| s.to_string()).collect(),
            );
        }
        if r.status == 429 || code == "TooManyRequests" || code == "activityLimitReached" {
            return Failure::NotDone(format!("{name} is limiting requests. Try again shortly{wait}. Nothing was changed."));
        }
        if r.status == 404 {
            return Failure::NotDone(format!("{name} could not find that. Nothing was changed."));
        }
        if r.status >= 500 && wrote {
            return Failure::Unknown(format!("{name} answered HTTP {} and whether it did this is unknown.", r.status));
        }
        let detail = if code.is_empty() { clip(message) } else { format!("{code}: {}", clip(message)) };
        Failure::NotDone(format!("{name} refused this (HTTP {}{}). Nothing was changed.", r.status, if detail.trim().is_empty() { String::new() } else { format!(", {detail}") }))
    }
}

/// The next page's path, from an answer's `@odata.nextLink`.
pub fn next_page(v: &Value) -> Option<String> {
    v["@odata.nextLink"].as_str().and_then(|l| l.strip_prefix(ORIGIN)).map(String::from)
}

/// The path a page token names, if it continues a request that began with
/// `prefix`: a token is never a way to another of Graph's addresses.
pub fn page(token: &str, prefix: &str) -> Result<String, String> {
    if token.starts_with(prefix) && !token.contains("..") && !token.contains('#') {
        Ok(token.to_string())
    } else {
        Err("page_token is not one this tool gave".into())
    }
}

pub fn clip(s: &str) -> String {
    s.chars().take(180).collect()
}

/// A string argument, or empty.
pub fn s(a: &Value, k: &str) -> String {
    a[k].as_str().unwrap_or_default().to_string()
}

/// A list of strings argument.
pub fn list(a: &Value, k: &str) -> Vec<String> {
    a[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
}

/// An id argument, required, encoded for a path.
pub fn id(a: &Value, k: &str) -> Result<String, String> {
    let v = s(a, k);
    if v.is_empty() {
        return Err(format!("{k} is missing"));
    }
    Ok(enc(&v))
}

/// A path segment or query value, percent-encoded.
pub fn enc(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A query string of the pairs that have a value; the keys are Graph's
/// own (`$top`), as they are.
pub fn query(pairs: &[(&str, Option<String>)]) -> String {
    let q: Vec<String> = pairs.iter().filter_map(|(k, v)| v.as_ref().filter(|v| !v.is_empty()).map(|v| format!("{k}={}", enc(v)))).collect();
    if q.is_empty() {
        String::new()
    } else {
        format!("?{}", q.join("&"))
    }
}

/// A whole-number argument, as it goes in a query.
pub fn opt_num(a: &Value, k: &str) -> Option<String> {
    a[k].as_u64().map(|n| n.to_string())
}

/// People as Graph takes them: `[{ "emailAddress": { "address" } }]`.
pub fn people(addresses: &[String]) -> Value {
    Value::Array(addresses.iter().map(|a| serde_json::json!({ "emailAddress": { "address": a } })).collect())
}

/// People as a person reads them: their addresses.
pub fn addresses(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|p| p["emailAddress"]["address"].as_str().map(String::from)).collect()
}

/// An object schema: its properties, which are required, and nothing else.
pub fn obj(props: Value, required: &[&str]) -> Value {
    serde_json::json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

#[cfg(test)]
#[path = "../../../tests/brokered-microsoft/microsoft.rs"]
mod tests;
