//! A relayed sign-in: started here, claimed and handed back by the relay.

use super::super::{challenge, fresh_tokens, granted, random, sign_in_target, Client, SignInTarget, PENDING_FOR};
use super::{now, open, timely, Message, Settings, Signed, B64};
use crate::api::{err, owner};
use crate::config::Connector;
use crate::kernel::Kernel;
use anyhow::{anyhow, bail, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A relayed sign-in waiting for its tokens, by nonce.
struct Waiting {
    app: String,
    person: String,
    provider: String,
    client: Client,
    verifier: Option<String>,
    /// The authorization server it went to, and whether that one names
    /// itself in its answer (RFC 9207): the relay passes on what it was told.
    issuer: String,
    iss_required: bool,
    /// The relay has claimed it: the next message for it is its handoff.
    claimed: bool,
    at: Instant,
}

fn waiting() -> &'static Mutex<HashMap<String, Waiting>> {
    static W: std::sync::OnceLock<Mutex<HashMap<String, Waiting>>> = std::sync::OnceLock::new();
    W.get_or_init(Default::default)
}

/// Makes a waiting sign-in older, as if started `by` ago.
#[cfg(test)]
pub(super) fn age(nonce: &str, by: std::time::Duration) {
    if let Some(w) = waiting().lock().unwrap_or_else(|e| e.into_inner()).get_mut(nonce) {
        w.at = Instant::now().checked_sub(by).unwrap_or(w.at);
    }
}

#[cfg(test)]
pub(super) fn is_waiting(nonce: &str) -> bool {
    waiting().lock().unwrap_or_else(|e| e.into_inner()).contains_key(nonce)
}

impl Kernel {
    /// The relay's settings, when `c` signs in through it.
    pub(crate) fn via_relay(&self, c: &Connector) -> Option<&Settings> {
        let s = self.cfg.relay.as_ref()?;
        c.registration.as_deref().filter(|p| s.relays(p)).map(|_| s)
    }

    /// Start a sign-in through the relay: the address to open in the
    /// person's browser, and its `state`, `<server id>.<nonce>`. The
    /// provider sends the browser to the relay's callback; no secret is
    /// sent from here.
    pub(crate) async fn relay_sign_in(&self, c: &Connector, person: &str, s: &Settings) -> Result<(String, String)> {
        let Some(provider) = c.registration.clone() else { bail!("{} has no provider registration to sign in through the relay", c.name) };
        let SignInTarget { asm, scopes, issuer, resource } = sign_in_target(c).await?;
        let field = |k: &str| asm[k].as_str().map(String::from);
        let (Some(authorize), Some(token)) = (field("authorization_endpoint"), field("token_endpoint")) else {
            bail!("the app's authorization server names no authorization or token endpoint")
        };
        if c.pkce && asm["code_challenge_methods_supported"].as_array().is_some_and(|m| !m.iter().any(|x| x == "S256")) {
            bail!("the app's authorization server does not support PKCE (S256), which MCP requires");
        }
        let shown = c.title.clone().unwrap_or_else(|| c.name.clone());
        let client_id = crate::registrations::client_id(&self.vault, &provider).map_err(|e| {
            tracing::warn!("{provider}'s client: {e:#}");
            anyhow!("{shown} cannot be connected on this server yet: Alpha Vector has not finished setting it up here")
        })?;
        let nonce = random();
        let state = format!("{}.{nonce}", s.server_id);
        let verifier = c.pkce.then(random);
        let mut u = url::Url::parse(&authorize)?;
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("response_type", "code").append_pair("client_id", &client_id).append_pair("redirect_uri", &s.callback).append_pair("state", &state);
            if let Some(v) = &verifier {
                q.append_pair("code_challenge", &challenge(v)).append_pair("code_challenge_method", "S256");
            }
            if !resource.is_empty() {
                q.append_pair("resource", &resource);
            }
            let mut asked = if c.scopes.is_empty() { scopes } else { c.scopes.clone() };
            for w in self.wanted_scopes(c, person) {
                if !asked.contains(&w) {
                    asked.push(w);
                }
            }
            if !asked.is_empty() {
                q.append_pair("scope", &asked.join(" "));
            }
            if authorize.starts_with("https://accounts.google.com/") {
                q.append_pair("access_type", "offline").append_pair("prompt", "consent");
            }
        }
        let client = Client { token_endpoint: token, revocation_endpoint: field("revocation_endpoint"), client_id, client_secret: None, resource, relay: Some(provider.clone()) };
        let iss_required = asm["authorization_response_iss_parameter_supported"] == true;
        let mut w = waiting().lock().unwrap_or_else(|e| e.into_inner());
        w.retain(|_, x| x.at.elapsed() < PENDING_FOR);
        w.insert(nonce, Waiting { app: c.name.clone(), person: person.into(), provider, client, verifier, issuer, iss_required, claimed: false, at: Instant::now() });
        Ok((u.to_string(), state))
    }
}

/// The relay's key, when a relay is configured.
fn relay_key(k: &Kernel) -> Result<([u8; 32], &Settings), String> {
    let s = k.cfg.relay.as_ref().ok_or("no relay is configured")?;
    Ok((s.key().map_err(|e| format!("{e:#}"))?, s))
}

/// The relay claims a nonce it has a code for: the answer, signed by this
/// server, says whose sign-in it is and gives its PKCE verifier. A nonce is
/// claimed once. The reason when refused.
pub(crate) fn claim(k: &Kernel, env: &Signed) -> Result<Signed, String> {
    let (key, s) = relay_key(k)?;
    let Message::Claim { server_id, nonce, iss, issued_at } = open(env, &key).map_err(|e| format!("{e:#}"))? else { return Err("not a claim".into()) };
    if server_id != s.server_id {
        return Err(format!("for server {server_id:?}, not this one"));
    }
    if !timely(issued_at) {
        return Err(format!("issued at {issued_at}, outside the allowed skew"));
    }
    let answer = {
        let mut m = waiting().lock().unwrap_or_else(|e| e.into_inner());
        let Some(w) = m.get_mut(&nonce).filter(|w| w.at.elapsed() < PENDING_FOR && !w.claimed) else { return Err("the nonce is unknown, expired or already claimed".into()) };
        // The answer must come from the server the sign-in went to (RFC 9207).
        let wrong = match iss.as_deref() {
            Some(i) => !crate::issuers::same(i, &w.issuer),
            None => w.iss_required,
        };
        if wrong {
            let went = w.issuer.clone();
            m.remove(&nonce);
            return Err(format!("the provider's answer named {iss:?}, not {went}, where the sign-in went"));
        }
        w.claimed = true;
        Message::Claimed {
            server_id: server_id.clone(),
            nonce: nonce.clone(),
            provider: w.provider.clone(),
            connector: w.app.clone(),
            person: w.person.clone(),
            code_verifier: w.verifier.clone(),
            issued_at: now(),
        }
    };
    k.relay_keys.seal(&k.vault, &answer).map_err(|e| format!("{e:#}"))
}

/// The relay hands over the tokens for a claimed nonce. They are kept as
/// any sign-in's are and the app is connected; the nonce is used up by any
/// handoff for it the relay signed. The reason when refused: nothing is kept.
pub(crate) async fn handoff(k: &Arc<Kernel>, env: &Signed) -> Result<(), String> {
    let (key, s) = relay_key(k)?;
    let Message::Handoff { server_id, nonce, connector, person, tokens, issued_at } = open(env, &key).map_err(|e| format!("{e:#}"))? else { return Err("not a handoff".into()) };
    if server_id != s.server_id {
        return Err(format!("for server {server_id:?}, not this one"));
    }
    if !timely(issued_at) {
        return Err(format!("issued at {issued_at}, outside the allowed skew"));
    }
    let w = waiting().lock().unwrap_or_else(|e| e.into_inner()).remove(&nonce);
    let w = w.filter(|w| w.at.elapsed() < PENDING_FOR).ok_or("the nonce is unknown, expired or used")?;
    if !w.claimed {
        return Err("the nonce was not claimed first".into());
    }
    if w.app != connector || w.person != person {
        return Err(format!("the handoff is for {connector} and {person}, the sign-in for {} and {}", w.app, w.person));
    }
    if k.read(|s| s.kill_switch) {
        return Err("the kill switch is engaged".into());
    }
    if !k.connector(&w.app).is_some_and(|c| k.via_relay(&c).is_some()) {
        return Err(format!("{} no longer signs in through the relay", w.app));
    }
    let mut t = fresh_tokens(granted(&tokens.as_answer())?, w.client);
    t.account = tokens.account.or(t.account);
    k.keep_sign_in(&w.app, &w.person, t).await.map_err(|e| format!("{e:#}"))?;
    // As a sign-in completed from the desktop app.
    k.open_after_connect(&w.person, &w.app);
    k.signed_in_after_consent(&w.app, &w.person).await;
    tracing::info!("{} connected {} through the relay", w.person, w.app);
    Ok(())
}

pub(crate) fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/relay", get(registration)).route("/v1/relay/claim", post(claim_route)).route("/v1/relay/handoff", post(handoff_route))
}

/// What the relay registers for this server: its id and public key.
async fn registration(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    let Some(s) = &k.cfg.relay else { return err(StatusCode::NOT_FOUND, "no relay is configured") };
    match k.relay_keys.public(&k.vault) {
        Ok(p) => Json(json!({ "server_id": s.server_id, "public_key": B64.encode(p), "callback": s.callback, "via_relay": s.via_relay })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

/// Refused, the relay is told only that; the reason is logged here.
fn refused(what: &str, why: &str) -> Response {
    tracing::warn!("relay {what} refused: {why}");
    err(StatusCode::FORBIDDEN, "refused")
}

async fn claim_route(State(k): State<Arc<Kernel>>, Json(env): Json<Signed>) -> Response {
    if k.cfg.relay.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match claim(&k, &env) {
        Ok(answer) => Json(answer).into_response(),
        Err(why) => refused("claim", &why),
    }
}

async fn handoff_route(State(k): State<Arc<Kernel>>, Json(env): Json<Signed>) -> Response {
    if k.cfg.relay.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match handoff(&k, &env).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(why) => refused("handoff", &why),
    }
}
