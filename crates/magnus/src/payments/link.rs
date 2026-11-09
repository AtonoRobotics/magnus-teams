//! Stripe's Link wallet for agents: one person's OAuth tokens and their
//! spend requests. Its API is separate from Stripe's and takes the customer's
//! token, never a Stripe key.
use super::{Card, LinkSettings};
use crate::kernel::Kernel;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

const SCOPE: &str = "payment_methods.agentic";
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Early, so a token does not expire on its way.
const REFRESH_MARGIN_SECONDS: i64 = 60;

/// Why Link gave no spend request.
pub(super) enum Failure {
    /// Link cannot serve this person now. The saved card may stand in.
    Unavailable(String),
    /// Link understood and refused, so a different card must not stand in.
    Refused(String),
}

#[derive(Deserialize, Clone, Debug, Default)]
pub(super) struct Spend {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub approval_url: Option<String>,
    #[serde(default)]
    pub status_details: Value,
    #[serde(default)]
    pub card: Option<Value>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: i64,
}

pub(super) struct NewSpend<'a> {
    pub amount_minor: i64,
    pub currency: &'a str,
    pub merchant_name: &'a str,
    pub merchant_url: &'a str,
    pub context: &'a str,
    pub idempotency_key: &'a str,
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder().timeout(CALL_TIMEOUT).redirect(reqwest::redirect::Policy::none()).build().context("an HTTP client")
}

pub(super) fn token_name(person: &str) -> String {
    format!("payments/link/{person}")
}

fn refreshing() -> &'static tokio::sync::Mutex<()> {
    static R: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub(super) fn authorize_url(s: &LinkSettings, state: &str, challenge: &str) -> Result<String> {
    let mut url = url::Url::parse(&format!("{}/auth", s.login_url)).context("the Link login address")?;
    url.query_pairs_mut()
        .append_pair("key", &s.publishable_key)
        .append_pair("client_id", &s.client_id)
        .append_pair("redirect_uri", &s.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

async fn token_call(k: &Kernel, s: &LinkSettings, form: &[(&str, &str)]) -> Result<Tokens> {
    let secret = k.vault.get(&s.client_secret)?.context("the Link client secret is not in the vault")?;
    let mut all = vec![("client_id", s.client_id.as_str()), ("client_secret", secret.expose())];
    all.extend_from_slice(form);
    let r = http()?.post(format!("{}/auth/token", s.login_url)).bearer_auth(&s.publishable_key).form(&all).send().await.context("reach Link")?;
    anyhow::ensure!(r.status().is_success(), "Link would not give a token: {}", r.status());
    let v: Value = r.json().await.context("Link's answer")?;
    Ok(Tokens {
        access: v["access_token"].as_str().context("no access token")?.to_string(),
        refresh: v["refresh_token"].as_str().context("no refresh token")?.to_string(),
        expires_at: chrono::Utc::now().timestamp() + v["expires_in"].as_i64().unwrap_or_default(),
    })
}

pub(super) async fn exchange(k: &Kernel, s: &LinkSettings, person: &str, code: &str, verifier: &str) -> Result<()> {
    let form = [("grant_type", "authorization_code"), ("redirect_uri", s.redirect_uri.as_str()), ("code", code), ("code_verifier", verifier)];
    let tokens = token_call(k, s, &form).await?;
    k.vault.put(&token_name(person), &serde_json::to_string(&tokens)?)
}

pub(super) async fn revoke(k: &Kernel, s: &LinkSettings, person: &str) -> Result<()> {
    let name = token_name(person);
    let Some(raw) = k.vault.get(&name)? else { return Ok(()) };
    let tokens: Tokens = serde_json::from_str(raw.expose()).context("the stored Link sign-in")?;
    let secret = k.vault.get(&s.client_secret)?.context("the Link client secret is not in the vault")?;
    let form = [("client_id", s.client_id.as_str()), ("client_secret", secret.expose()), ("token", tokens.refresh.as_str()), ("token_type_hint", "refresh_token")];
    let r = http()?.post(format!("{}/auth/revoke", s.login_url)).bearer_auth(&s.publishable_key).form(&form).send().await.context("reach Link")?;
    anyhow::ensure!(r.status().is_success(), "Link would not end the connection: {}", r.status());
    k.vault.remove(&name)?;
    Ok(())
}

/// The person's access token, refreshed first if it is about to lapse. A
/// refresh token works once, so the new one is stored before anything else.
async fn access(k: &Kernel, s: &LinkSettings, person: &str) -> Result<Option<String>> {
    let name = token_name(person);
    let _one_at_a_time = refreshing().lock().await;
    let Some(raw) = k.vault.get(&name)? else { return Ok(None) };
    let tokens: Tokens = serde_json::from_str(raw.expose()).context("the stored Link sign-in")?;
    if tokens.expires_at - REFRESH_MARGIN_SECONDS > chrono::Utc::now().timestamp() {
        return Ok(Some(tokens.access));
    }
    let fresh = token_call(k, s, &[("grant_type", "refresh_token"), ("refresh_token", tokens.refresh.as_str())]).await?;
    k.vault.put(&name, &serde_json::to_string(&fresh)?)?;
    Ok(Some(fresh.access))
}

pub(super) fn connected(k: &Kernel, person: &str) -> bool {
    k.vault.get(&token_name(person)).is_ok_and(|t| t.is_some())
}

fn failure(context: &str, status: reqwest::StatusCode, body: &str) -> Failure {
    let why = format!("{context}: {status}: {}", body.chars().take(300).collect::<String>());
    if status.is_server_error() || matches!(status.as_u16(), 401 | 403 | 404 | 429) {
        Failure::Unavailable(why)
    } else {
        Failure::Refused(why)
    }
}

async fn token_for(k: &Kernel, s: &LinkSettings, person: &str) -> Result<String, Failure> {
    match access(k, s, person).await {
        Ok(Some(token)) => Ok(token),
        Ok(None) => Err(Failure::Unavailable("this person has not connected Link".into())),
        Err(e) => Err(Failure::Unavailable(format!("{e:#}"))),
    }
}

async fn send(request: reqwest::RequestBuilder, context: &str) -> Result<Spend, Failure> {
    let r = request.send().await.map_err(|e| Failure::Unavailable(format!("{context}: {e}")))?;
    let status = r.status();
    let body = r.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(failure(context, status, &body));
    }
    serde_json::from_str(&body).map_err(|e| Failure::Unavailable(format!("{context}: {e}")))
}

/// A spend request with its approval already asked for.
pub(super) async fn create(k: &Kernel, s: &LinkSettings, person: &str, new: &NewSpend<'_>) -> Result<Spend, Failure> {
    let token = token_for(k, s, person).await?;
    let client = http().map_err(|e| Failure::Unavailable(format!("{e:#}")))?;
    let body = json!({
        "amount": new.amount_minor,
        "currency": new.currency,
        "merchant_name": new.merchant_name,
        "merchant_url": new.merchant_url,
        "context": new.context,
        "credential_type": "card",
        "idempotency_key": new.idempotency_key,
        "request_approval": true,
        "test": s.test_mode,
    });
    send(client.post(format!("{}/spend_requests", s.api_url)).bearer_auth(token).json(&body), "create a spend request").await
}

pub(super) async fn retrieve(k: &Kernel, s: &LinkSettings, person: &str, id: &str, with_card: bool) -> Result<Spend, Failure> {
    let token = token_for(k, s, person).await?;
    let client = http().map_err(|e| Failure::Unavailable(format!("{e:#}")))?;
    let mut url = format!("{}/spend_requests/{id}", s.api_url);
    if with_card {
        url.push_str("?include=card");
    }
    send(client.get(url).bearer_auth(token), "read a spend request").await
}

/// The card on an approved request. Nothing here is logged or stored.
pub(super) fn card_of(spend: &Spend) -> Option<Card> {
    let c = spend.card.as_ref()?;
    let address = &c["billing_address"];
    Some(Card {
        number: c["number"].as_str()?.to_string(),
        cvc: c["cvc"].as_str()?.to_string(),
        exp_month: c["exp_month"].as_u64()? as u32,
        exp_year: c["exp_year"].as_u64()? as u32,
        name: address["name"].as_str().unwrap_or_default().to_string(),
        postal_code: address["postal_code"].as_str().unwrap_or_default().to_string(),
    })
}

/// When the card stops working, if Link says.
pub(super) fn valid_until(spend: &Spend) -> Option<chrono::DateTime<chrono::Utc>> {
    spend.card.as_ref()?["valid_until"].as_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn only_trouble_with_the_service_lets_the_saved_card_stand_in() {
        for code in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN, StatusCode::TOO_MANY_REQUESTS, StatusCode::BAD_GATEWAY] {
            assert!(matches!(failure("x", code, ""), Failure::Unavailable(_)), "{code}");
        }
        assert!(matches!(failure("x", StatusCode::BAD_REQUEST, ""), Failure::Refused(_)));
    }

    #[test]
    fn a_card_needs_its_number_and_code() {
        let full = json!({ "number": "4242", "cvc": "100", "exp_month": 6, "exp_year": 2029, "billing_address": { "name": "J", "postal_code": "94015" } });
        let spend = Spend { card: Some(full), ..Default::default() };
        let card = card_of(&spend).unwrap();
        assert_eq!((card.number.as_str(), card.exp_month, card.postal_code.as_str()), ("4242", 6, "94015"));
        assert!(card_of(&Spend { card: Some(json!({ "number": "4242" })), ..Default::default() }).is_none());
        assert!(card_of(&Spend::default()).is_none());
    }
}
