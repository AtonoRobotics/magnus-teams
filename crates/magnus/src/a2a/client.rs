//! Calls to an outside agent: JSON-RPC 2.0 over HTTPS to the endpoint its
//! card declared at connect and nowhere else, with `A2A-Version: 1.0`, and
//! the credential its card's scheme chose taken from the vault at the last
//! moment and attached here, as an app's sign-in is at the boundary's
//! credential stage. Nothing returned carries the credential back.

use super::ledger::{OutsideAgent, Scheme};
use crate::egress_client::Egress;
use crate::kernel::Kernel;
use serde_json::{json, Value};
use std::time::Duration;

/// Why a call came back without an answer.
#[derive(Debug)]
pub(crate) enum RpcError {
    /// Nothing reached the agent: it can be tried again.
    NotSent(String),
    /// The request may have reached the agent: what it did is unknown.
    Unknown(String),
    /// The agent answered with a refusal or an error, in its own words.
    Refused { code: i64, message: String },
}

impl RpcError {
    pub(crate) fn said(&self) -> String {
        match self {
            RpcError::NotSent(m) => format!("nothing was sent: {m}"),
            RpcError::Unknown(m) => format!("the outcome is unknown: {m}"),
            RpcError::Refused { code, message } => format!("it refused ({code}): {message}"),
        }
    }
}

/// The vault name of the credential Magnus presents to an outside agent.
pub(crate) fn credential_name(agent: &str) -> String {
    format!("outside-agent/{agent}/credential")
}

/// Read `resp`'s body up to `cap` bytes, piece by piece: a longer one is
/// refused as soon as it passes the cap (or says it will), so a hostile
/// endpoint streaming without end costs at most `cap`.
pub(crate) async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(format!("its answer is over {cap} bytes"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("its answer was cut off: {e}"))? {
        if body.len() + chunk.len() > cap {
            return Err(format!("its answer is over {cap} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn scrub(text: &str, secret: Option<&str>) -> String {
    match secret.filter(|s| s.len() >= 4) {
        Some(s) => text.replace(s, "[credential]"),
        None => text.to_string(),
    }
}

impl Kernel {
    fn outside_egress(&self, wait: Duration) -> Egress {
        Egress::new(self.cfg.web.allow_private_networks, super::CONNECT_WAIT_SECS, wait)
    }

    /// An access token from the agent's token URL, by OAuth 2's client
    /// credentials flow with the client id and secret kept in the vault.
    async fn client_credentials(&self, token_url: &str, scopes: &[String], stored: &str) -> Result<String, RpcError> {
        let c: Value = serde_json::from_str(stored).map_err(|_| RpcError::NotSent("the stored client credentials are not readable".into()))?;
        let mut form = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", c["client_id"].as_str().unwrap_or_default().to_string()),
            ("client_secret", c["client_secret"].as_str().unwrap_or_default().to_string()),
        ];
        if !scopes.is_empty() {
            form.push(("scope", scopes.join(" ")));
        }
        let url = super::card::checked_url(token_url, self.cfg.web.allow_private_networks).map_err(|e| RpcError::NotSent(format!("{e:#}")))?;
        let resp = crate::token_endpoint::post(&self.outside_egress(super::CONNECT_WAIT_SECS), url.as_str(), &form)
            .await
            .map_err(|e| RpcError::NotSent(format!("its token URL: {e:#}")))?;
        let body = read_capped(resp, super::MAX_TOKEN_ANSWER_BYTES).await.map_err(|e| RpcError::NotSent(format!("its token URL: {e}")))?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| RpcError::NotSent(format!("its token URL: {e}")))?;
        v["access_token"].as_str().map(String::from).ok_or_else(|| RpcError::NotSent("its token URL gave no access token".into()))
    }

    /// One JSON-RPC call to `agent`'s declared endpoint: its `result`, or why not.
    pub(crate) async fn outside_rpc(&self, agent: &OutsideAgent, method: &str, params: Value, wait: Duration) -> Result<Value, RpcError> {
        let secret = match agent.scheme {
            Scheme::None => None,
            _ => Some(
                self.vault
                    .get(&credential_name(&agent.id))
                    .map_err(|e| RpcError::NotSent(format!("the vault: {e:#}")))?
                    .ok_or_else(|| RpcError::NotSent(format!("no credential is kept for {}: connect it again", agent.name)))?,
            ),
        };
        let raw = secret.as_ref().map(|s| s.expose().to_string());
        let bearer = match &agent.scheme {
            Scheme::Bearer => raw.clone(),
            Scheme::OAuth2 { token_url, scopes } => Some(self.client_credentials(token_url, scopes, raw.as_deref().unwrap_or_default()).await?),
            _ => None,
        };
        let identity = match &agent.scheme {
            Scheme::MutualTls => {
                Some(reqwest::Identity::from_pem(raw.as_deref().unwrap_or_default().as_bytes()).map_err(|e| RpcError::NotSent(format!("the stored client certificate: {e}")))?)
            }
            _ => None,
        };
        let url = url::Url::parse(&agent.endpoint).map_err(|e| RpcError::NotSent(format!("its endpoint: {e}")))?;
        let client = self.outside_egress(wait).client_as(&url, identity).await.map_err(|e| RpcError::NotSent(format!("{e:#}")))?;
        let body = json!({ "jsonrpc": "2.0", "id": crate::kernel::new_id("rpc"), "method": method, "params": params });
        let mut req = client.post(url).header("A2A-Version", super::PROTOCOL_VERSION).header(reqwest::header::ACCEPT, "application/json").json(&body);
        if let (Scheme::ApiKey { header }, Some(key)) = (&agent.scheme, raw.as_deref()) {
            req = req.header(header.as_str(), key);
        }
        if let Some(b) = &bearer {
            req = req.bearer_auth(b);
        }
        let shown = |e: &str| scrub(&scrub(e, raw.as_deref()), bearer.as_deref());
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_builder() => return Err(RpcError::NotSent(shown(&e.to_string()))),
            Err(e) => return Err(RpcError::Unknown(shown(&e.to_string()))),
        };
        let status = resp.status();
        let body = read_capped(resp, super::MAX_ANSWER_BYTES).await.map_err(|e| RpcError::Unknown(shown(&e)))?;
        let text = String::from_utf8_lossy(&body);
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) if status.is_client_error() => return Err(RpcError::Refused { code: i64::from(status.as_u16()), message: shown(&super::bounded(&text)) }),
            Err(_) => return Err(RpcError::Unknown(format!("it answered {status} with no JSON-RPC answer"))),
        };
        if let Some(e) = v.get("error") {
            return Err(RpcError::Refused { code: e["code"].as_i64().unwrap_or_default(), message: shown(&super::bounded(e["message"].as_str().unwrap_or_default())) });
        }
        v.get("result").cloned().ok_or_else(|| RpcError::Unknown(format!("it answered {status} with no result")))
    }
}
