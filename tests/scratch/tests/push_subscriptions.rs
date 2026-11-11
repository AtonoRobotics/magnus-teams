//! Push subscriptions against a stand-in provider
//! (decisions/2026-11-10-push-subscriptions.md): Graph's subscription
//! resource (made, extended in place, made again when Graph has dropped it,
//! deleted) and Google's web_hook channels (opened, replaced with the old
//! one stopped, stopped), each with the person's bearer token and the push
//! address, and a refusal said as one.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use magnus::push_subs::provider::{self, Endpoints, Kind};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

const BEARER: &str = "Bearer person-token";
const ADDRESS: &str = "https://teams.example.com/v1/push/microsoft-outlook/sam/abc";
const BASE: &str = "https://teams.example.com";

#[derive(Default)]
struct Provider {
    /// Every request: method, path, body.
    seen: Vec<(String, String, Value)>,
    /// Live subscriptions or channels, by id.
    live: BTreeMap<String, Value>,
    next: u32,
}

type S = State<Arc<Mutex<Provider>>>;

fn lock(p: &Mutex<Provider>) -> std::sync::MutexGuard<'_, Provider> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

fn authorised(h: &HeaderMap) -> Result<(), StatusCode> {
    (h.get("authorization").and_then(|v| v.to_str().ok()) == Some(BEARER)).then_some(()).ok_or(StatusCode::UNAUTHORIZED)
}

async fn graph_create(State(p): S, h: HeaderMap, Json(b): Json<Value>) -> Result<Json<Value>, StatusCode> {
    authorised(&h)?;
    let mut p = lock(&p);
    p.seen.push(("POST".into(), "/subscriptions".into(), b.clone()));
    p.next += 1;
    let id = format!("sub-{}", p.next);
    p.live.insert(id.clone(), b.clone());
    Ok(Json(json!({ "id": id, "expirationDateTime": b["expirationDateTime"], "resource": b["resource"] })))
}

async fn graph_patch(State(p): S, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Result<Json<Value>, StatusCode> {
    authorised(&h)?;
    let mut p = lock(&p);
    p.seen.push(("PATCH".into(), format!("/subscriptions/{id}"), b.clone()));
    if !p.live.contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(json!({ "id": id, "expirationDateTime": b["expirationDateTime"] })))
}

async fn graph_delete(State(p): S, h: HeaderMap, Path(id): Path<String>) -> StatusCode {
    if authorised(&h).is_err() {
        return StatusCode::UNAUTHORIZED;
    }
    let mut p = lock(&p);
    p.seen.push(("DELETE".into(), format!("/subscriptions/{id}"), Value::Null));
    if p.live.remove(&id).is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn start_page(State(p): S, h: HeaderMap) -> Result<Json<Value>, StatusCode> {
    authorised(&h)?;
    lock(&p).seen.push(("GET".into(), "/drive/v3/changes/startPageToken".into(), Value::Null));
    Ok(Json(json!({ "startPageToken": "4242" })))
}

async fn watch(State(p): S, h: HeaderMap, Query(q): Query<HashMap<String, String>>, Json(b): Json<Value>) -> Result<Json<Value>, StatusCode> {
    authorised(&h)?;
    let mut p = lock(&p);
    p.seen.push(("POST".into(), format!("watch?pageToken={}", q.get("pageToken").cloned().unwrap_or_default()), b.clone()));
    p.next += 1;
    let resource = format!("res-{}", p.next);
    p.live.insert(b["id"].as_str().unwrap_or_default().to_string(), json!({ "resourceId": resource }));
    // Google keeps the shorter of what was asked and its own limit.
    let asked: i64 = b["expiration"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit = (Utc::now() + Duration::days(3)).timestamp_millis();
    Ok(Json(json!({ "kind": "api#channel", "id": b["id"], "resourceId": resource, "expiration": asked.min(limit).to_string() })))
}

async fn stop(State(p): S, h: HeaderMap, Path(api): Path<String>, Json(b): Json<Value>) -> StatusCode {
    if authorised(&h).is_err() {
        return StatusCode::UNAUTHORIZED;
    }
    let mut p = lock(&p);
    p.seen.push(("POST".into(), format!("/{api}/v3/channels/stop"), b.clone()));
    let id = b["id"].as_str().unwrap_or_default().to_string();
    match p.live.get(&id) {
        Some(c) if c["resourceId"] == b["resourceId"] => {
            p.live.remove(&id);
            StatusCode::NO_CONTENT
        }
        _ => StatusCode::NOT_FOUND,
    }
}

async fn stand_in() -> std::io::Result<(Endpoints, Arc<Mutex<Provider>>)> {
    let p = Arc::new(Mutex::new(Provider::default()));
    let app = Router::new()
        .route("/graph/subscriptions", post(graph_create))
        .route("/graph/subscriptions/{id}", patch(graph_patch))
        .route("/graph/subscriptions/{id}", delete(graph_delete))
        .route("/google/drive/v3/changes/startPageToken", get(start_page))
        .route("/google/drive/v3/changes/watch", post(watch))
        .route("/google/calendar/v3/calendars/primary/events/watch", post(watch))
        .route("/google/{api}/v3/channels/stop", post(stop))
        .with_state(p.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let at = l.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(l, app).await;
    });
    Ok((Endpoints { graph: format!("http://{at}/graph"), google: format!("http://{at}/google") }, p))
}

fn graph() -> Option<Kind> {
    provider::kind("microsoft-outlook")
}

/// Graph: a subscription is made for the inbox's new mail at the push
/// address, extended in place, made again when Graph no longer has it, and
/// deleted; deleting one Graph has dropped is not an error.
#[tokio::test]
async fn a_graph_subscription_is_made_renewed_and_deleted() {
    let (ep, p) = stand_in().await.unwrap();
    let http = reqwest::Client::new();
    let now = Utc::now();
    let token = "person-token";

    let sub = provider::create(&http, &ep, &graph().unwrap(), token, ADDRESS, BASE, now).await.unwrap();
    let (_, _, body) = p.lock().unwrap().seen[0].clone();
    assert_eq!(
        (body["notificationUrl"].as_str(), body["resource"].as_str(), body["changeType"].as_str()),
        (Some(ADDRESS), Some("me/mailFolders('inbox')/messages"), Some("created"))
    );
    let asked = DateTime::parse_from_rfc3339(body["expirationDateTime"].as_str().unwrap()).unwrap();
    assert!(asked.signed_duration_since(now).num_minutes() < 10_080, "asked for longer than Graph allows: {body}");
    assert_eq!((sub.id.as_str(), sub.base.as_str()), ("sub-1", BASE));
    assert!(sub.expires > now + Duration::days(6), "the end Graph gave was not kept: {sub:?}");

    // Renewed in place, a day later, to a later end.
    let later = now + Duration::days(1);
    let renewed = provider::renew(&http, &ep, &graph().unwrap(), token, &sub, ADDRESS, later).await.unwrap();
    assert_eq!(renewed.id, sub.id, "a renewal made a new subscription");
    assert!(renewed.expires > sub.expires, "the renewal did not extend it: {renewed:?}");
    assert_eq!(p.lock().unwrap().seen.last().unwrap().1, "/subscriptions/sub-1");

    // Graph dropped it: the renewal makes it again.
    p.lock().unwrap().live.clear();
    let again = provider::renew(&http, &ep, &graph().unwrap(), token, &renewed, ADDRESS, later).await.unwrap();
    assert_eq!(again.id, "sub-2", "a dropped subscription was not made again");

    provider::delete(&http, &ep, &graph().unwrap(), token, &again).await.unwrap();
    assert!(p.lock().unwrap().live.is_empty(), "the subscription was not deleted");
    provider::delete(&http, &ep, &graph().unwrap(), token, &again).await.expect("deleting a subscription Graph no longer has failed");
}

/// Google: a Drive changes channel opens from the start page token at the
/// push address and keeps Google's own end; renewing opens a new channel
/// and stops the old; deleting stops it. A Calendar channel opens on the
/// primary calendar.
#[tokio::test]
async fn a_google_channel_is_opened_replaced_and_stopped() {
    let (ep, p) = stand_in().await.unwrap();
    let http = reqwest::Client::new();
    let now = Utc::now();
    let drive = provider::kind("google-drive").unwrap();
    let token = "person-token";

    let ch = provider::create(&http, &ep, &drive, token, ADDRESS, BASE, now).await.unwrap();
    {
        let seen = &p.lock().unwrap().seen;
        assert_eq!(seen[0].1, "/drive/v3/changes/startPageToken");
        assert_eq!(seen[1].1, "watch?pageToken=4242", "the channel did not start from the start page token");
        assert_eq!((seen[1].2["type"].as_str(), seen[1].2["address"].as_str()), (Some("web_hook"), Some(ADDRESS)));
    }
    assert_eq!(ch.resource.as_deref(), Some("res-1"));
    assert!(ch.expires < now + Duration::days(3) + Duration::minutes(1), "Google's shorter end was not kept: {ch:?}");

    let next = provider::renew(&http, &ep, &drive, token, &ch, ADDRESS, now + Duration::days(2)).await.unwrap();
    assert_ne!(next.id, ch.id, "a channel was renewed in place, which Google cannot do");
    {
        let pr = p.lock().unwrap();
        assert!(!pr.live.contains_key(&ch.id), "the replaced channel was not stopped");
        assert!(pr.live.contains_key(&next.id));
        let stop = pr.seen.iter().find(|s| s.1 == "/drive/v3/channels/stop").expect("no stop");
        assert_eq!((stop.2["id"].as_str(), stop.2["resourceId"].as_str()), (Some(ch.id.as_str()), Some("res-1")));
    }
    provider::delete(&http, &ep, &drive, token, &next).await.unwrap();
    assert!(p.lock().unwrap().live.is_empty(), "the channel was not stopped");

    let cal = provider::kind("google-calendar").unwrap();
    let c = provider::create(&http, &ep, &cal, token, ADDRESS, BASE, now).await.unwrap();
    provider::delete(&http, &ep, &cal, token, &c).await.unwrap();
    assert!(p.lock().unwrap().seen.iter().any(|s| s.1 == "/calendar/v3/channels/stop"), "the calendar channel was stopped at another API");
}

/// A provider that refuses the person's token is said as a refusal, with
/// its status, and nothing is taken as made.
#[tokio::test]
async fn a_refused_subscription_is_an_error() {
    let (ep, p) = stand_in().await.unwrap();
    let http = reqwest::Client::new();
    let e = provider::create(&http, &ep, &graph().unwrap(), "someone-else", ADDRESS, BASE, Utc::now()).await.unwrap_err();
    assert!(format!("{e:#}").contains("401"), "{e:#}");
    assert!(p.lock().unwrap().live.is_empty());
}
