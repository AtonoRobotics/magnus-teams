//! The apps' routes.

// The API: the organisation's apps, which the owner adds, classifies and
// removes; and each person's own sign-in to them, which only they make and
// end, and which only their runs use.

use super::admin::Refusal;
use super::slack::slack_oauth_callback;
use super::{allows, listing, relay, requests, suggested_risk, title, Standing, OAUTH};
use crate::api::{err, internal};
use crate::api_error::{refuse, ApiResult};
use crate::config::Connector;
use crate::extract::{Body, Caller, Owner};
use crate::kernel::Kernel;
use crate::{events::Ev, people::rights, policy::may_use_app};
use axum::{
    extract::{Path, State as AxState},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

type K = AxState<Arc<Kernel>>;

/// A kernel operation's answer as a route's.
fn answer(r: Result<Value, Refusal>, ok: StatusCode) -> Response {
    match r {
        Ok(_) if ok == StatusCode::NO_CONTENT => StatusCode::NO_CONTENT.into_response(),
        Ok(v) => (ok, Json(v)).into_response(),
        Err((st, m)) => err(st, m),
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/oauth/slack/callback", get(slack_oauth_callback))
        .route("/v1/connectors", axum::routing::get(list).post(add))
        .route("/v1/connectors/{name}", axum::routing::get(detail).delete(remove))
        .route("/v1/connectors/{name}/tools/{tool}", put(classify))
        .route("/v1/connectors/{name}/sign-in", post(sign_in))
        .route("/v1/connectors/{name}/sign-in/complete", post(sign_in_complete))
        .route("/v1/connectors/{name}/connection", delete(disconnect))
        .route("/v1/registrations/{provider}", put(set_registration))
        .merge(relay::router())
}

/// Alpha Vector provisions its client secret for a provider to this
/// appliance (its deployment calls this; no person types it, and nothing
/// shows it again), until the relay holds it (connectors spec rev 46). The
/// log records only that it was provisioned.
async fn set_registration(AxState(k): K, Owner(p): Owner, Path(provider): Path<String>, Body(b): Body<requests::Registration>) -> ApiResult {
    Ok(answer(k.set_registration(&format!("person:{}", p.id), &provider, &b.client_secret).await, StatusCode::NO_CONTENT))
}

/// The organisation's apps, each with whether the person asking is
/// signed in to it. The owner also gets each app's tools as the app lists
/// them now (through the owner's own sign-in), each with the owner's
/// classification (none: the worker is not offered it) and the
/// classification the app's own hints suggest.
async fn list(AxState(k): K, Caller(p): Caller) -> ApiResult {
    let added: Vec<String> = k.read(|s| s.apps.connectors().keys().cloned().collect());
    let mut out = vec![];
    for c in k.connectors() {
        // An app blocked for the person's groups, or for groups they are
        // not in, is not in their Library (connectors spec, Org policy).
        // The owner still manages it in Settings, marked as not theirs.
        let theirs = crate::policy::may_use_app(&p, &c);
        if !theirs && (!rights(&p).manages_shared_apps() || c.owner.is_some()) {
            continue;
        }
        let from_app = added.contains(&c.name) && k.cfg.connectors.iter().all(|f| f.name != c.name);
        let mut card = card(&k, &c, &p.id).await;
        card["for_you"] = json!(theirs);
        let secret = k.connector_secret(&c, &p.id).await.ok().flatten();
        let signed_in = c.credential.is_none() || secret.is_some();
        if !rights(&p).manages_shared_apps() {
            out.push(card);
            continue;
        }
        let (tools, problem) = if !signed_in {
            (vec![], Some(if c.auth.as_deref() == Some(OAUTH) { "sign in to this app".to_string() } else { "no credential is stored".to_string() }))
        } else {
            match k.app_tools(&c, secret.as_ref().map(|s| s.expose())).await {
                Ok(ts) => (
                    ts.iter()
                        .map(|t| {
                            // No description: it is screened, not shown (spec rev 140).
                            json!({ "name": t.name, "risk": c.tools.get(&t.name).map(|x| x.risk.clone()), "suggested": suggested_risk(&t.annotations) })
                        })
                        .collect(),
                    None,
                ),
                Err(e) => (vec![], Some(format!("{e:#}"))),
            }
        };
        let admin = json!({
            "url": c.url, "audience": c.audience, "label": c.label, "access_groups": c.access_groups, "blocked_groups": c.blocked_groups,
            "source": if c.catalog { "catalog" } else if from_app { "app" } else { "config" }, "problem": problem, "tools": tools,
        });
        for (k, v) in admin.as_object().into_iter().flatten() {
            card[k] = v.clone();
        }
        out.push(card);
    }
    Ok(Json(json!({ "connectors": out })).into_response())
}

/// A name for an app: what it calls itself, else the first part of its
/// host that says something ("mcp.notion.com" is "notion"), made unique.
pub(crate) fn name_from(url: &str, own: Option<&str>, taken: impl Fn(&str) -> bool) -> String {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
    let from_host =
        host.split('.').find(|p| !["mcp", "www", "api", "app", "server"].contains(p) && !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit())).unwrap_or("app").to_string();
    let base: String = own
        .map(|n| n.trim().replace([' ', '_'], "-"))
        .filter(|n| !n.is_empty())
        .unwrap_or(from_host)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();
    let base = if base.is_empty() { "app".to_string() } else { base };
    let mut name = base.clone();
    let mut n = 2;
    while taken(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// One app as a person's card shows it (connectors spec, The connector
/// card and Library): its name, one sentence on what Magnus can do with
/// it, where they stand, and, signed in, as whom, since when and when last
/// used. No protocol words.
async fn card(k: &Kernel, c: &Connector, person: &str) -> Value {
    let (standing, at, account) = k.standing(c, person).await;
    let (sentence, allows) = allows(c);
    let summary = c.summary.clone().unwrap_or(sentence);
    let last = k.recent_uses(&c.name, person, 1).first().map(|u| u["at"].clone());
    json!({
        "generated": c.owner.is_some(), "needs_token": c.owner.is_some() && c.credential.is_some(), "name": c.name, "title": c.title.clone().unwrap_or_else(|| title(&c.name)), "summary": summary, "allows": allows, "catalog": c.catalog,
        "state": standing.as_str(), "signed_in": standing == Standing::Connected, "signs_in": c.auth.as_deref() == Some(OAUTH),
        "connected_at": at.and_then(|s| chrono::DateTime::from_timestamp(s, 0)), "account": account, "last_used": last,
        // False only on an appliance Alpha Vector has not finished setting up.
        "ready": k.provider_ready(c), "watched": !c.watch.is_empty(), "instant": k.push_instant(c),
        // The address the desktop app listens on, when the provider
        // registered one exactly. Not shown.
        "redirect_uri": c.redirect_uri,
        "handoff": c.handoff,
        // The app asked for more access than the sign-in holds: approving
        // it is signing in again, asking for that too.
        "more_access": !k.wanted_scopes(c, person).is_empty(),
        "listing": listing(c),
    })
}

/// One app's card for the person asking, with the last things Magnus did
/// through it for them (from the event log).
async fn detail(AxState(k): K, Caller(p): Caller, Path(name): Path<String>) -> ApiResult {
    let Some(c) = k.connector(&name).filter(|c| may_use_app(&p, c) || c.owner.is_none() && rights(&p).manages_shared_apps()) else {
        return refuse(StatusCode::NOT_FOUND, "no such app");
    };
    let mut v = card(&k, &c, &p.id).await;
    v["for_you"] = json!(crate::policy::may_use_app(&p, &c));
    v["recent"] = json!(k.recent_uses(&c.name, &p.id, 10));
    Ok(Json(v).into_response())
}

/// Add an app by its MCP address.
async fn add(AxState(k): K, Owner(p): Owner, Json(b): Json<Value>) -> ApiResult {
    Ok(answer(k.add_connector(&format!("person:{}", p.id), &b).await, StatusCode::CREATED))
}

fn added(k: &Kernel, name: &str) -> Result<Connector, Response> {
    k.added_connector(name).map_err(|(st, m)| err(st, m))
}

/// Remove an app: its sign-in is revoked at the app and its credential deleted.
async fn remove(AxState(k): K, Owner(p): Owner, Path(name): Path<String>) -> ApiResult {
    Ok(answer(k.remove_connector(&format!("person:{}", p.id), &name).await, StatusCode::NO_CONTENT))
}

/// Classify one tool: its risk class, or null to stop offering it.
async fn classify(AxState(k): K, Owner(p): Owner, Path((name, tool)): Path<(String, String)>, Json(b): Json<Value>) -> ApiResult {
    added(&k, &name)?;
    let risk = b["risk"].as_str().map(String::from);
    if risk.as_deref().is_some_and(|r| crate::tools::Risk::parse(r).is_none()) {
        return refuse(StatusCode::BAD_REQUEST, "risk is read, reversible_write, irreversible, financial or prohibited");
    }
    // The sandbox is the appliance's own microVM: an app's tool acts at the
    // app, so classed so it would skip the contract and the Rule of Two.
    if risk.as_deref() == Some("sandbox") {
        return refuse(StatusCode::BAD_REQUEST, "sandbox is the server's own microVM, never an app's tool: risk is read, reversible_write, irreversible, financial or prohibited");
    }
    k.emit(&format!("person:{}", p.id), Ev::ToolClassified { connector: name, tool, risk }).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// A person signs in to one of the organisation's apps with their own
/// account; the sign-in is theirs alone.
async fn sign_in(AxState(k): K, Caller(p): Caller, Path(name): Path<String>, Body(b): Body<requests::SignIn>) -> ApiResult {
    let Some(c) = k.connector(&name).filter(|c| c.auth.as_deref() == Some(OAUTH)) else { return refuse(StatusCode::NOT_FOUND, "no app by that name signs in") };
    if !crate::policy::may_use_app(&p, &c) {
        return refuse(StatusCode::FORBIDDEN, "your organisation has not made this app available to you");
    }
    if k.read(|s| s.vault.kill_switch()) {
        return refuse(StatusCode::CONFLICT, "the kill switch is engaged");
    }
    Ok(match k.connector_sign_in(&c, &p.id, &b.redirect_uri).await {
        Ok((url, state)) => Json(json!({ "authorize_url": url, "state": state })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, &format!("{e:#}")),
    })
}

async fn sign_in_complete(AxState(k): K, Caller(p): Caller, Path(name): Path<String>, Body(b): Body<requests::SignInComplete>) -> ApiResult {
    let (Some(state), Some(code)) = (&b.state, &b.code) else { return refuse(StatusCode::BAD_REQUEST, "state and code are required") };
    Ok(match k.connector_sign_in_complete(&name, &p.id, state, code, b.iss.as_deref()).await {
        Ok(()) => {
            // Connected for the first time: Magnus opens a conversation about
            // it, and the tools its connect screen covered are classed.
            k.open_after_connect(&p.id, &name);
            k.signed_in_after_consent(&name, &p.id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, &format!("{e:#}")),
    })
}

/// A person ends their own sign-in to an app.
async fn disconnect(AxState(k): K, Caller(p): Caller, Path(name): Path<String>) -> ApiResult {
    // Turned off, an app keeps its sign-ins: each person can still end theirs.
    if !k.all_connectors().iter().any(|(c, _)| c.name == name) {
        return refuse(StatusCode::NOT_FOUND, "no such app");
    }
    Ok(match k.disconnect(&name, &p.id, &format!("person:{}", p.id)).await {
        // Where the app could not end the sign-in itself, the person is
        // told where they remove Magnus there (connectors spec, Lifecycle).
        Ok(Some((at_provider, detail))) => {
            let remove_at = (!at_provider).then(|| crate::catalog::manifests().iter().find(|m| m.id == name).and_then(|m| m.remove_access.clone())).flatten();
            Json(json!({ "at_provider": at_provider, "detail": detail, "remove_at": remove_at })).into_response()
        }
        Ok(None) => err(StatusCode::NOT_FOUND, "you are not signed in to this app"),
        Err(e) => internal(e),
    })
}
