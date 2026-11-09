//! Connecting Link and keeping a saved card, from the person's own settings.
use super::{link, Card};
use crate::api::{err, who};
use crate::kernel::Kernel;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const SIGN_IN_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_SIGN_INS: usize = 256;
const MIN_CARD_DIGITS: usize = 12;
const MAX_CARD_DIGITS: usize = 19;

type K = State<Arc<Kernel>>;

struct SignIn {
    person: String,
    verifier: String,
    at: Instant,
}

/// Sign-ins under way, by their `state`.
fn sign_ins() -> &'static Mutex<HashMap<String, SignIn>> {
    static S: OnceLock<Mutex<HashMap<String, SignIn>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn random() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/payments", get(state))
        .route("/v1/payments/link", post(connect).delete(disconnect))
        .route("/v1/payments/link/callback", get(callback))
        .route("/v1/payments/card", put(save_card).delete(remove_card))
}

async fn state(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(settings) = k.cfg.payments.as_ref() else { return Json(json!({ "available": false })).into_response() };
    let card = k.vault.get(&super::card_name(&p.id)).ok().flatten().and_then(|raw| serde_json::from_str::<Card>(raw.expose()).ok());
    let last4 = card.map(|c| c.number.chars().skip(c.number.chars().count().saturating_sub(4)).collect::<String>());
    Json(json!({ "available": true, "link_offered": settings.link.is_some(), "link_connected": link::connected(&k, &p.id), "card_last4": last4 })).into_response()
}

async fn connect(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(s) = k.cfg.payments.as_ref().and_then(|c| c.link.as_ref()) else { return err(StatusCode::CONFLICT, "Link is not set up on this server") };
    let (state, verifier) = (random(), random());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    {
        let mut pending = sign_ins().lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, s| s.at.elapsed() < SIGN_IN_WINDOW);
        if pending.len() >= MAX_SIGN_INS {
            return err(StatusCode::TOO_MANY_REQUESTS, "too many sign-ins under way");
        }
        pending.insert(state.clone(), SignIn { person: p.id.clone(), verifier, at: Instant::now() });
    }
    match link::authorize_url(s, &state, &challenge) {
        Ok(url) => Json(json!({ "url": url })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct Returned {
    code: Option<String>,
    state: Option<String>,
}

/// Link sends the browser here, so `state` is what shows whose sign-in it is.
async fn callback(State(k): K, Query(back): Query<Returned>) -> Response {
    let Some(s) = k.cfg.payments.as_ref().and_then(|c| c.link.as_ref()) else { return err(StatusCode::CONFLICT, "Link is not set up on this server") };
    let taken = back.state.as_deref().and_then(|state| sign_ins().lock().unwrap_or_else(|e| e.into_inner()).remove(state)).filter(|s| s.at.elapsed() < SIGN_IN_WINDOW);
    let Some(sign_in) = taken else { return err(StatusCode::BAD_REQUEST, "that sign-in is not one we started") };
    let Some(code) = back.code else { return Html("Link was not connected. You can close this page.").into_response() };
    match link::exchange(&k, s, &sign_in.person, &code, &sign_in.verifier).await {
        Ok(()) => Html("Link is connected. You can close this page.").into_response(),
        Err(e) => {
            tracing::warn!("connecting Link: {e:#}");
            err(StatusCode::BAD_GATEWAY, "Link did not finish connecting")
        }
    }
}

async fn disconnect(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(s) = k.cfg.payments.as_ref().and_then(|c| c.link.as_ref()) else { return err(StatusCode::CONFLICT, "Link is not set up on this server") };
    match link::revoke(&k, s, &p.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

fn card_from(b: &Value) -> Result<Card, &'static str> {
    let digits = |key: &str| b[key].as_str().map(|v| v.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>()).unwrap_or_default();
    let (number, cvc) = (digits("number"), digits("cvc"));
    if !(MIN_CARD_DIGITS..=MAX_CARD_DIGITS).contains(&number.len()) || !number.bytes().all(|c| c.is_ascii_digit()) {
        return Err("that is not a card number");
    }
    if !(3..=4).contains(&cvc.len()) || !cvc.bytes().all(|c| c.is_ascii_digit()) {
        return Err("that is not a security code");
    }
    let (exp_month, exp_year) = (b["exp_month"].as_u64().unwrap_or_default(), b["exp_year"].as_u64().unwrap_or_default());
    if !(1..=12).contains(&exp_month) || !(2000..=2100).contains(&exp_year) {
        return Err("that is not an expiry date");
    }
    let text = |key: &str| b[key].as_str().unwrap_or_default().trim().to_string();
    Ok(Card { number, cvc, exp_month: exp_month as u32, exp_year: exp_year as u32, name: text("name"), postal_code: text("postal_code") })
}

async fn save_card(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if k.cfg.payments.is_none() {
        return err(StatusCode::CONFLICT, "payments are not set up on this server");
    }
    let card = match card_from(&b) {
        Ok(c) => c,
        Err(why) => return err(StatusCode::BAD_REQUEST, why),
    };
    let saved = serde_json::to_string(&card).map_err(anyhow::Error::from).and_then(|raw| k.vault.put(&super::card_name(&p.id), &raw));
    match saved {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

async fn remove_card(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.vault.remove(&super::card_name(&p.id)) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_card_must_look_like_one() {
        let good = json!({ "number": "4242 4242 4242 4242", "cvc": "100", "exp_month": 6, "exp_year": 2029, "name": " Jo " });
        let card = card_from(&good).unwrap();
        assert_eq!((card.number.as_str(), card.name.as_str()), ("4242424242424242", "Jo"));
        for (key, bad) in [("number", json!("12")), ("number", json!("4242-abcd-4242-4242")), ("cvc", json!("1")), ("exp_month", json!(13)), ("exp_year", json!(99))] {
            let mut b = good.clone();
            b[key] = bad;
            assert!(card_from(&b).is_err(), "{key}");
        }
    }
}
