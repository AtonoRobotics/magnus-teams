//! The agreements' routes: the documents as public pages, each person's
//! standing and answers, and the admin's list and invitation of testers.

use super::documents::{self, Versions, PRIVACY_PATH, TERMS_PATH, TESTERS_PATH};
use super::Change;
use crate::admin_app::Admin;
use crate::api::{err, internal, who};
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

/// What a person still has to answer before the app lets them in.
pub const CHANGED: &str = "the text changed since you opened it; read it again";

pub fn router(k: Arc<Kernel>) -> Router<Arc<Kernel>> {
    Router::new()
        .route(
            TERMS_PATH,
            get(|State(k): K| async move {
                if k.cfg.legal.is_set() {
                    page(&documents::terms(&k.cfg.legal))
                } else {
                    err(StatusCode::NOT_FOUND, NOT_SET_UP)
                }
            }),
        )
        .route(
            PRIVACY_PATH,
            get(|State(k): K| async move {
                if k.cfg.legal.is_set() {
                    page(&documents::privacy(&k.cfg.legal))
                } else {
                    err(StatusCode::NOT_FOUND, NOT_SET_UP)
                }
            }),
        )
        .route(
            TESTERS_PATH,
            get(|State(k): K| async move {
                if k.cfg.legal.is_set() {
                    page(&documents::testers(&k.cfg.legal))
                } else {
                    err(StatusCode::NOT_FOUND, NOT_SET_UP)
                }
            }),
        )
        .route("/v1/legal", get(standing))
        .route("/v1/legal/terms", post(accept_terms))
        .route("/v1/legal/usage", post(choose_usage))
        .route("/v1/legal/testers/join", post(join))
        .route("/v1/legal/testers/leave", post(leave))
        .route("/v1/legal/testers/decline", post(decline))
        .merge(crate::admin_app::guarded(Router::new().route("/v1/admin/testers", get(testers)).route("/v1/admin/testers/invite", post(invite)), k))
}

/// The answer to everything but the standing while no `[legal]` is set.
pub const NOT_SET_UP: &str = "this server's agreements are not set up yet";

fn page(d: &documents::Doc) -> Response {
    crate::admin_app::invite_page::shell(d.title, &documents::html(d))
}

/// A person's standing: what they accepted and chose, what is due, and
/// the documents as they read now.
pub fn standing_of(k: &Kernel, person: &str) -> Value {
    let l = &k.cfg.legal;
    if !l.is_set() {
        return json!({ "set_up": false, "terms_due": false, "usage_due": false });
    }
    let now = Versions::of(l);
    let (terms, usage, tester, invited) =
        k.read(|s| (s.legal.terms_of(person).cloned(), s.legal.usage_of(person).cloned(), s.legal.tester_of(person).cloned(), s.legal.invited_by(person).cloned()));
    let terms_due = terms.as_ref().is_none_or(|(t, p)| *t != now.terms || *p != now.privacy);
    json!({
        "terms_due": terms_due,
        "usage": usage.as_ref().map(|(share, v)| json!({ "share": share, "current": *v == now.usage })),
        "usage_due": usage.as_ref().is_none_or(|(_, v)| *v != now.usage),
        "tester": tester.as_ref().map(|v| json!({ "version": v, "current": *v == now.testers })),
        "invited_by": invited.and_then(|by| k.person(&by).map(|p| if p.name.is_empty() { p.id } else { p.name })),
        "documents": { "terms": documents::terms(l), "privacy": documents::privacy(l), "testers": documents::testers(l) },
        "usage_prompt": documents::usage_prompt(l),
        "contact": l.contact,
    })
}

async fn standing(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(standing_of(&k, &p.id)).into_response(),
        Err(r) => r,
    }
}

/// Record `c` for the signed-in person and answer with their standing.
async fn record(k: &Kernel, person: &str, c: Change) -> Response {
    if !k.cfg.legal.is_set() {
        return err(StatusCode::CONFLICT, NOT_SET_UP);
    }
    match k.emit(&format!("person:{person}"), Ev::Legal(c)).await {
        Ok(_) => Json(standing_of(k, person)).into_response(),
        Err(e) => internal(e),
    }
}

/// `{ "terms": <version>, "privacy": <version> }`, the versions shown.
async fn accept_terms(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let now = Versions::of(&k.cfg.legal);
    if b["terms"] != now.terms.as_str() || b["privacy"] != now.privacy.as_str() {
        return err(StatusCode::CONFLICT, CHANGED);
    }
    record(&k, &p.id, Change::TermsAccepted { person: p.id.clone(), terms: now.terms, privacy: now.privacy }).await
}

/// `{ "share": true|false, "version": <the prompt's version> }`.
async fn choose_usage(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(share) = b["share"].as_bool() else { return err(StatusCode::BAD_REQUEST, "share is true or false") };
    let now = Versions::of(&k.cfg.legal);
    if b["version"] != now.usage.as_str() {
        return err(StatusCode::CONFLICT, CHANGED);
    }
    record(&k, &p.id, Change::UsageChosen { person: p.id.clone(), share, version: now.usage }).await
}

/// `{ "version": <the Tester Terms' version> }`: joining is accepting them.
async fn join(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let now = Versions::of(&k.cfg.legal);
    if b["version"] != now.testers.as_str() {
        return err(StatusCode::CONFLICT, CHANGED);
    }
    record(&k, &p.id, Change::TesterJoined { person: p.id.clone(), version: now.testers }).await
}

async fn leave(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(version) = k.read(|s| s.legal.tester_of(&p.id).cloned()) else { return err(StatusCode::CONFLICT, "you are not in the tester program") };
    record(&k, &p.id, Change::TesterLeft { person: p.id.clone(), version }).await
}

async fn decline(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if k.read(|s| s.legal.invited_by(&p.id).is_none()) {
        return err(StatusCode::CONFLICT, "there is no invitation to decline");
    }
    record(&k, &p.id, Change::TesterDeclined { person: p.id.clone() }).await
}

/// Who is a tester, with whether they accepted the Tester Terms as they
/// read now, and who is invited (Admin › People).
async fn testers(State(k): K, Extension(_a): Extension<Admin>) -> Response {
    let now = Versions::of(&k.cfg.legal);
    let (testers, invited) = k.read(|s| (s.legal.testers().clone(), s.legal.invited().clone()));
    let testers: Vec<Value> = testers.into_iter().map(|(person, v)| json!({ "person": person, "version": v, "current": v == now.testers })).collect();
    let invited: Vec<Value> = invited.into_iter().map(|(person, by)| json!({ "person": person, "by": by })).collect();
    Json(json!({ "testers": testers, "invited": invited, "terms": TESTERS_PATH })).into_response()
}

/// `{ "person": <id> }`: ask them to join. Only they can join, by accepting
/// the Tester Terms themselves.
async fn invite(State(k): K, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if !k.cfg.legal.is_set() {
        return err(StatusCode::CONFLICT, NOT_SET_UP);
    }
    let Some(person) = b["person"].as_str().filter(|p| k.person(p).is_some()) else { return err(StatusCode::NOT_FOUND, "no such person") };
    if k.read(|s| s.legal.tester_of(person).is_some()) {
        return err(StatusCode::CONFLICT, "they are already a tester");
    }
    let c = Change::TesterInvited { person: person.into(), by: a.person.id.clone() };
    match k.emit(&a.actor(), Ev::Legal(c)).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    }
}
