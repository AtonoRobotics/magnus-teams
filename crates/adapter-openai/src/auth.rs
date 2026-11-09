//! Sign-in and token refresh. Both OAuth kinds are device sign-ins: the person
//! approves on the provider's site, and completion polls until they have, so
//! nothing is pasted back into Magnus.
//!
//! `oauth_device` (RFC 8628, xAI for Grok): settings `auth` = { kind,
//! device_url, token_url, client_id, scopes }.
//!
//! `oauth_openai_device` (ChatGPT for Codex, as codex-rs `device_code_auth.rs` does
//! it): { kind, device_url (user code), poll_url, verification_url, token_url,
//! redirect_uri, client_id, account_claim (JSON pointer into the id token's
//! claims), account_header }. The poll returns an authorization code and its
//! PKCE verifier, which are exchanged at the token endpoint.

use base64::Engine;
use magnus_adapter_proto::{ErrorClass, Event, Sink};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// RFC 8628 §3.2: poll every 5 seconds when the provider names no interval.
const DEFAULT_INTERVAL_S: u64 = 5;

/// Seconds given as a number or a numeric string (OpenAI sends `"5"`).
fn secs(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.trim().parse().ok())
}

fn claims(id_token: &str) -> Value {
    let part = id_token.split('.').nth(1).unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn credential_from(a: &Value, tokens: &Value, previous: Option<&Value>) -> Value {
    let now = chrono::Utc::now().timestamp();
    let expires_at = tokens["expires_in"].as_i64().map(|s| now + s).unwrap_or(now + 3600);
    let id_token = tokens["id_token"].as_str().unwrap_or_default();
    let account = a["account_claim"]
        .as_str()
        .and_then(|p| claims(id_token).pointer(p).and_then(|v| v.as_str().map(String::from)))
        .or_else(|| previous.and_then(|p| p["account_id"].as_str().map(String::from)));
    json!({
        "access_token": tokens["access_token"],
        "refresh_token": tokens["refresh_token"].as_str().map(String::from).or_else(|| previous.and_then(|p| p["refresh_token"].as_str().map(String::from))),
        "expires_at": expires_at,
        "account_id": account,
    })
}

async fn token_request(http: &reqwest::Client, a: &Value, form: &[(&str, &str)]) -> Result<Value, (ErrorClass, String)> {
    let url = a["token_url"].as_str().ok_or((ErrorClass::InvalidRequest, "no token_url in settings".to_string()))?;
    let resp = http.post(url).form(form).send().await.map_err(|e| (ErrorClass::Unavailable, format!("token endpoint unreachable: {e}")))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if (200..300).contains(&status) && v["access_token"].is_string() {
        return Ok(v);
    }
    let err = v["error"].as_str().unwrap_or_default().to_string();
    let class = match (status, err.as_str()) {
        (_, "authorization_pending") | (_, "slow_down") => ErrorClass::Unavailable,
        (500..=599, _) => ErrorClass::Unavailable,
        _ => ErrorClass::AuthExpired,
    };
    Err((class, format!("token endpoint returned {status}: {}", if err.is_empty() { body.chars().take(200).collect() } else { err })))
}

/// A usable access token for an OAuth credential; refreshes when close to
/// expiry or when forced (after a 401).
pub async fn oauth_bearer(http: &reqwest::Client, settings: &Value, credential: &str, force: bool) -> Result<(String, Option<String>, Option<String>), (ErrorClass, String)> {
    let c: Value = serde_json::from_str(credential).map_err(|_| (ErrorClass::AuthExpired, "the stored sign-in is not readable; sign in again".to_string()))?;
    let a = &settings["auth"];
    let account = c["account_id"].as_str().map(String::from);
    let expires = c["expires_at"].as_i64().unwrap_or(0);
    if !force && expires - chrono::Utc::now().timestamp() > 300 {
        let t = c["access_token"].as_str().unwrap_or_default().to_string();
        return Ok((t, account, None));
    }
    let Some(refresh) = c["refresh_token"].as_str() else {
        return Err((ErrorClass::AuthExpired, "the sign-in expired and cannot be refreshed; sign in again".into()));
    };
    let client = a["client_id"].as_str().unwrap_or_default();
    let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client)];
    if let Some(s) = a["scopes"].as_str() {
        form.push(("scope", s));
    }
    let tokens = token_request(http, a, &form).await.map_err(|(cl, m)| {
        if cl == ErrorClass::Unavailable {
            (cl, m)
        } else {
            (ErrorClass::AuthExpired, format!("refresh failed, sign in again ({m})"))
        }
    })?;
    let next = credential_from(a, &tokens, Some(&c));
    let token = next["access_token"].as_str().unwrap_or_default().to_string();
    Ok((token, next["account_id"].as_str().map(String::from), Some(next.to_string())))
}

pub async fn begin(http: &reqwest::Client, settings: &Value, sink: &mut Sink) -> anyhow::Result<()> {
    let a = &settings["auth"];
    match a["kind"].as_str().unwrap_or("api_key") {
        "api_key" => {
            sink.send(&Event::AuthStep {
                flow: json!({ "kind": "api_key" }),
                url: settings["key_url"].as_str().map(String::from),
                user_code: None,
                instructions: "Create an API key with the provider and paste it here.".into(),
            })
            .await
        }
        "oauth_device" => {
            let url = a["device_url"].as_str().unwrap_or_default();
            let resp = http.post(url).form(&[("client_id", a["client_id"].as_str().unwrap_or_default()), ("scope", a["scopes"].as_str().unwrap_or_default())]).send().await;
            let v: Value = match resp {
                Ok(r) => r.json().await.unwrap_or(Value::Null),
                Err(e) => return sink.error(ErrorClass::Unavailable, format!("device endpoint unreachable: {e}")).await,
            };
            let (Some(device), Some(wait_s)) = (v["device_code"].as_str(), secs(&v["expires_in"])) else {
                return sink.error(ErrorClass::AuthExpired, format!("the provider refused the sign-in request: {v}")).await;
            };
            sink.send(&Event::AuthStep {
                flow: json!({ "kind": "oauth_device", "device_code": device, "interval": secs(&v["interval"]).unwrap_or(DEFAULT_INTERVAL_S), "wait_s": wait_s }),
                url: v["verification_uri_complete"].as_str().or_else(|| v["verification_uri"].as_str()).map(String::from),
                user_code: v["user_code"].as_str().map(String::from),
                instructions: "Open the link and approve the sign-in; check that it shows this code. Magnus finishes by itself.".into(),
            })
            .await
        }
        "oauth_openai_device" => {
            let url = a["device_url"].as_str().unwrap_or_default();
            let v: Value = match http.post(url).json(&json!({ "client_id": a["client_id"] })).send().await {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
                Ok(r) => {
                    let st = r.status().as_u16();
                    let body: String = r.text().await.unwrap_or_default().chars().take(200).collect();
                    return sink.error(ErrorClass::AuthExpired, format!("the provider refused the sign-in request: {st} {body}")).await;
                }
                Err(e) => return sink.error(ErrorClass::Unavailable, format!("device endpoint unreachable: {e}")).await,
            };
            let (Some(id), Some(code)) = (v["device_auth_id"].as_str(), v["user_code"].as_str().or_else(|| v["usercode"].as_str())) else {
                return sink.error(ErrorClass::AuthExpired, format!("the provider's answer has no sign-in code: {v}")).await;
            };
            // Wait as long as the code is valid (`expires_at`, RFC 3339).
            let Some(wait_s) =
                v["expires_at"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| (t.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64)
            else {
                return sink.error(ErrorClass::AuthExpired, format!("the provider's answer has no expiry: {v}")).await;
            };
            sink.send(&Event::AuthStep {
                flow: json!({ "kind": "oauth_openai_device", "device_auth_id": id, "user_code": code, "interval": secs(&v["interval"]).unwrap_or(DEFAULT_INTERVAL_S), "wait_s": wait_s }),
                url: a["verification_url"].as_str().map(String::from),
                user_code: Some(code.to_string()),
                instructions: "Open the link, sign in, and enter the code shown. Magnus finishes by itself.".into(),
            })
            .await
        }
        k => sink.error(ErrorClass::InvalidRequest, format!("unknown auth kind {k}")).await,
    }
}

pub async fn complete(http: &reqwest::Client, settings: &Value, flow: &Value, input: &Value, sink: &mut Sink) -> anyhow::Result<()> {
    let a = &settings["auth"];
    match flow["kind"].as_str().unwrap_or_default() {
        "api_key" => {
            let key = input["api_key"].as_str().unwrap_or_default().trim().to_string();
            if key.is_empty() {
                return sink.error(ErrorClass::AuthExpired, "no API key given").await;
            }
            // Prove the key works before the kernel stores it.
            let r = http.get(format!("{}{}", crate::base(settings), settings["models_path"].as_str().unwrap_or("/models"))).bearer_auth(&key).send().await;
            match r {
                Ok(r) if r.status().is_success() => sink.send(&Event::AuthDone { credential: key }).await,
                Ok(r) => {
                    let st = r.status().as_u16();
                    let (c, m) = crate::classify(st, &r.text().await.unwrap_or_default());
                    sink.error(c, m).await
                }
                Err(e) => sink.error(ErrorClass::Unavailable, format!("could not reach the provider: {e}")).await,
            }
        }
        "oauth_device" => {
            let device = flow["device_code"].as_str().unwrap_or_default();
            let mut interval = flow["interval"].as_u64().unwrap_or(DEFAULT_INTERVAL_S).max(1);
            let form = [("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("device_code", device), ("client_id", a["client_id"].as_str().unwrap_or_default())];
            let deadline = Instant::now() + Duration::from_secs(flow["wait_s"].as_u64().unwrap_or_default());
            loop {
                match token_request(http, a, &form).await {
                    Ok(t) => return sink.send(&Event::AuthDone { credential: credential_from(a, &t, None).to_string() }).await,
                    // Not approved yet, or a passing outage.
                    Err((ErrorClass::Unavailable, m)) if Instant::now() < deadline => {
                        if m.ends_with("slow_down") {
                            interval += 5; // RFC 8628 §3.5
                        }
                        tokio::time::sleep(Duration::from_secs(interval)).await;
                    }
                    Err((ErrorClass::Unavailable, _)) => return sink.error(ErrorClass::AuthExpired, "the sign-in was not approved in time; start again").await,
                    Err((c, m)) => return sink.error(c, m).await,
                }
            }
        }
        "oauth_openai_device" => {
            let poll = a["poll_url"].as_str().unwrap_or_default();
            let body = json!({ "device_auth_id": flow["device_auth_id"], "user_code": flow["user_code"] });
            let interval = flow["interval"].as_u64().unwrap_or(DEFAULT_INTERVAL_S).max(1);
            let deadline = Instant::now() + Duration::from_secs(flow["wait_s"].as_u64().unwrap_or_default());
            let granted: Value = loop {
                match http.post(poll).json(&body).send().await {
                    Ok(r) if r.status().is_success() => break r.json().await.unwrap_or(Value::Null),
                    // 403 and 404 mean not approved yet (codex-rs); a send error is a passing outage.
                    Ok(r) if matches!(r.status().as_u16(), 403 | 404) => {}
                    Ok(r) => return sink.error(ErrorClass::AuthExpired, format!("the provider refused the sign-in: {}", r.status())).await,
                    Err(_) => {}
                }
                if Instant::now() >= deadline {
                    return sink.error(ErrorClass::AuthExpired, "the sign-in was not approved in time; start again").await;
                }
                tokio::time::sleep(Duration::from_secs(interval)).await;
            };
            let form = [
                ("grant_type", "authorization_code"),
                ("code", granted["authorization_code"].as_str().unwrap_or_default()),
                ("redirect_uri", a["redirect_uri"].as_str().unwrap_or_default()),
                ("client_id", a["client_id"].as_str().unwrap_or_default()),
                ("code_verifier", granted["code_verifier"].as_str().unwrap_or_default()),
            ];
            match token_request(http, a, &form).await {
                Ok(t) => sink.send(&Event::AuthDone { credential: credential_from(a, &t, None).to_string() }).await,
                Err((c, m)) => sink.error(c, m).await,
            }
        }
        k => sink.error(ErrorClass::InvalidRequest, format!("unknown sign-in flow {k}")).await,
    }
}

/// The provider's revocation endpoint (RFC 7009): `revoke_url` in settings,
/// else the one its discovery document publishes, at the issuer (`issuer`,
/// else the token endpoint's origin).
async fn revocation_endpoint(http: &reqwest::Client, a: &Value) -> Result<String, String> {
    if let Some(u) = a["revoke_url"].as_str() {
        return Ok(u.to_string());
    }
    let issuer = match a["issuer"].as_str() {
        Some(i) => i.trim_end_matches('/').to_string(),
        None => {
            let t = reqwest::Url::parse(a["token_url"].as_str().ok_or("no token_url in settings")?).map_err(|e| format!("token_url: {e}"))?;
            t.origin().ascii_serialization()
        }
    };
    for doc in ["/.well-known/openid-configuration", "/.well-known/oauth-authorization-server"] {
        let Ok(r) = http.get(format!("{issuer}{doc}")).send().await else { continue };
        if let Ok(v) = r.json::<Value>().await {
            if let Some(u) = v["revocation_endpoint"].as_str() {
                return Ok(u.to_string());
            }
        }
    }
    Err(format!("{issuer} publishes no revocation endpoint"))
}

/// Revoke an OAuth sign-in at the provider: the refresh token (which ends the
/// grant), then the access token. An API key cannot be revoked from here.
pub async fn revoke(http: &reqwest::Client, settings: &Value, credential: &str, sink: &mut Sink) -> anyhow::Result<()> {
    let a = &settings["auth"];
    if a["kind"].as_str().unwrap_or("api_key") == "api_key" {
        return sink.send(&Event::AuthRevoked { at_provider: false, detail: "an API key is revoked in the provider's console".into() }).await;
    }
    let c: Value = serde_json::from_str(credential).unwrap_or(Value::Null);
    let url = match revocation_endpoint(http, a).await {
        Ok(u) => u,
        Err(m) => return sink.send(&Event::AuthRevoked { at_provider: false, detail: m }).await,
    };
    let client = a["client_id"].as_str().unwrap_or_default();
    let mut revoked = vec![];
    let mut failed = vec![];
    for (hint, token) in [("refresh_token", c["refresh_token"].as_str()), ("access_token", c["access_token"].as_str())] {
        let Some(token) = token.filter(|t| !t.is_empty()) else { continue };
        let form = [("token", token), ("token_type_hint", hint), ("client_id", client)];
        match http.post(&url).form(&form).send().await {
            Ok(r) if r.status().is_success() => revoked.push(hint),
            Ok(r) => failed.push(format!("{hint}: {}", r.status())),
            Err(e) => failed.push(format!("{hint}: {e}")),
        }
    }
    let at_provider = revoked.contains(&"refresh_token") || (!revoked.is_empty() && c["refresh_token"].as_str().is_none());
    let detail = if failed.is_empty() { format!("revoked at {url}") } else { format!("{url}: {}", failed.join("; ")) };
    sink.send(&Event::AuthRevoked { at_provider, detail }).await
}
