//! Knowledge in the admin console (admin console spec): the organisation's
//! shared ontology, context and memory, which agents curate in the org
//! context bundle, and an admin oversees, corrects and rolls back.
//!
//! - Ontology: the entity types (`/types/<id>.md`): create, edit, add or
//!   remove a relation (an edit), merge two, deprecate (status), delete (a
//!   type in use is merged first); every change a version, rolled back.
//! - Context: what every agent receives: the map (`/index.md` and each
//!   area's index), the structure (who does what: `/people/`), the
//!   conventions (terminology and standing facts: `/conventions/`) and the
//!   sources; each edited in place; the map as one person's agent sees it.
//! - Memory: the entries (`/entities/`), searched and filtered; provenance
//!   and versions; edit, delete, roll back; in bulk with a fresh password.
//! - Activity: every change agents make, with its before and after; one
//!   reverted, or a selection as a batch with a fresh password.
//!
//! A document is edited as OKF writes it: YAML frontmatter, then the body.
//! Every change is audited before it is made, and written as
//! `human:<admin>` through the kernel's own checks.

use super::session::Admin;
use super::users::{failed, recorded, refuse};
use super::Console;
use axum::extract::{Path, Query, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// An id that fits one path segment: a document's path without its `.md`,
/// with `~` for `/` (`entities~customer~acme` is `/entities/customer/acme.md`).
fn path_of(id: &str) -> Option<String> {
    let ok = !id.is_empty()
        && id.len() <= 300
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
        && !id.split('~').any(|s| s.is_empty() || s == "." || s == "..");
    ok.then(|| format!("/{}.md", id.replace('~', "/")))
}

pub fn id_of(path: &str) -> String {
    path.trim_start_matches('/').trim_end_matches(".md").replace('/', "~")
}

fn q(pairs: &[(&str, &str)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, v);
    }
    s.finish()
}

/// OKF text split into its frontmatter and body.
pub fn parse(text: &str) -> Result<(Value, String), String> {
    let t = text.replace("\r\n", "\n");
    let Some(rest) = t.strip_prefix("---\n") else { return Ok((json!({}), t)) };
    let Some(end) = rest.find("\n---") else { return Err("the frontmatter has no closing ---".into()) };
    let fm: Value = serde_yaml_ng::from_str(&rest[..end]).map_err(|e| format!("the frontmatter is not YAML: {e}"))?;
    let body = rest[end + 4..].trim_start_matches('\n').to_string();
    Ok((if fm.is_null() { json!({}) } else { fm }, body))
}

async fn list(c: &Console, kinds: &str) -> Response {
    match c.kernel.call(Method::GET, &format!("/v1/admin/knowledge?{}", q(&[("kinds", kinds)])), None).await {
        Ok(mut v) => {
            for d in v["documents"].as_array_mut().into_iter().flatten() {
                d["id"] = json!(id_of(d["path"].as_str().unwrap_or_default()));
            }
            Json(v).into_response()
        }
        Err(e) => failed(e),
    }
}

async fn doc(c: &Console, path: &str) -> Result<Value, Response> {
    let mut v = c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/doc?{}", q(&[("path", path)])), None).await.map_err(failed)?;
    v["id"] = json!(id_of(path));
    v["text"] = json!(crate::bundle::render(&v["frontmatter"], v["body"].as_str().unwrap_or_default()));
    for k in ["entities", "referenced_by"] {
        if let Some(a) = v[k].as_array().cloned() {
            v[k] = json!(a.iter().map(|p| json!({ "path": p, "id": id_of(p.as_str().unwrap_or_default()) })).collect::<Vec<_>>());
        }
    }
    Ok(v)
}

fn sub(a: &Admin) -> String {
    a.session.sub.clone()
}

/// Write a document from its OKF text, audited with its version before.
async fn write(c: &Console, a: &Admin, action: &str, path: &str, text: &str) -> Response {
    let (fm, body) = match parse(text) {
        Ok(x) => x,
        Err(m) => return refuse(StatusCode::BAD_REQUEST, "invalid", &m),
    };
    let before = c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/doc?{}", q(&[("path", path)])), None).await.ok().map(|d| json!({ "version": d["version"] }));
    let b = json!({ "sub": sub(a), "path": path, "frontmatter": fm, "body": body });
    match recorded(c, a, action, path, before, Some(json!({ "text": text })), c.kernel.call(Method::POST, "/v1/admin/knowledge/doc", Some(&b))).await {
        Ok(v) => Json(json!({ "id": id_of(path), "path": path, "version": v["version"] })).into_response(),
        Err(r) => r,
    }
}

// ---- Ontology ----------------------------------------------------------

pub async fn types(State(c): State<Arc<Console>>) -> Response {
    list(&c, "type").await
}

pub async fn get_type(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    let Some(p) = path_of(&format!("types~{id}")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such type.") };
    match doc(&c, &p).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Create a type: its id and its OKF text.
pub async fn create_type(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let id = b["id"].as_str().unwrap_or_default().trim().to_lowercase();
    let Some(p) = path_of(&format!("types~{id}")).filter(|_| !id.contains('~')) else {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "A type's id is letters, digits, - or _.");
    };
    if c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/doc?{}", q(&[("path", &p)])), None).await.is_ok() {
        return refuse(StatusCode::CONFLICT, "exists", "That type exists: edit it instead.");
    }
    write(&c, &a, "ontology_type_created", &p, b["text"].as_str().unwrap_or_default()).await
}

/// Edit a type: its whole text (`text`), or only its relations
/// (`relations`, a list of `{type, to, cardinality}`) or its status
/// (`status`: draft, stable, deprecated; an admin makes a type stable). A
/// new version either way.
pub async fn edit_type(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(p) = path_of(&format!("types~{id}")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such type.") };
    if let Some(t) = b["text"].as_str() {
        return write(&c, &a, "ontology_type_changed", &p, t).await;
    }
    let d = match doc(&c, &p).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    let mut fm = d["frontmatter"].clone();
    let action = if let Some(rels) = b.get("relations").and_then(|r| r.as_array()) {
        let ok = rels.iter().all(|r| {
            r["type"].as_str().is_some_and(|t| !t.trim().is_empty())
                && r["to"].as_str().is_none_or(|t| t.starts_with("/types/"))
                && r["cardinality"].as_str().is_none_or(|c| ["one", "many"].contains(&c))
        });
        if !ok {
            return refuse(StatusCode::BAD_REQUEST, "invalid", "Each relation has a name, the type it goes to, and a cardinality of one or many.");
        }
        fm["relations"] = json!(rels
            .iter()
            .map(|r| {
                let mut o = serde_json::Map::new();
                for k in ["type", "to", "cardinality"] {
                    if !r[k].is_null() {
                        o.insert(k.into(), r[k].clone());
                    }
                }
                Value::Object(o)
            })
            .collect::<Vec<_>>());
        "ontology_relations_changed"
    } else if let Some(st) = b["status"].as_str() {
        fm["status"] = json!(st);
        if st == "deprecated" {
            "ontology_type_deprecated"
        } else {
            "ontology_type_status_changed"
        }
    } else {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Give the text, the relations or the status.");
    };
    let text = crate::bundle::render(&fm, d["body"].as_str().unwrap_or_default());
    write(&c, &a, action, &p, &text).await
}

async fn remove(c: &Console, a: &Admin, action: &str, path: &str) -> Response {
    let body = json!({ "sub": sub(a), "path": path });
    let before = c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/doc?{}", q(&[("path", path)])), None).await.ok().map(|d| json!({ "version": d["version"] }));
    match recorded(c, a, action, path, before, None, c.kernel.call(Method::DELETE, "/v1/admin/knowledge/doc", Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

pub async fn delete_type(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let Some(p) = path_of(&format!("types~{id}")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such type.") };
    remove(&c, &a, "ontology_type_deleted", &p).await
}

pub async fn merge_types(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let (Some(from), Some(into)) = (b["from"].as_str().and_then(|x| path_of(&format!("types~{x}"))), b["into"].as_str().and_then(|x| path_of(&format!("types~{x}")))) else {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Name the type to merge and the one it goes into.");
    };
    let body = json!({ "sub": sub(&a), "from": from, "into": into });
    match recorded(&c, &a, "ontology_types_merged", &from, None, Some(json!({ "into": into })), c.kernel.call(Method::POST, "/v1/admin/knowledge/merge", Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

async fn history_of(c: &Console, path: &str) -> Response {
    match doc(c, path).await {
        Ok(v) => Json(json!({ "versions": v["versions"] })).into_response(),
        Err(r) => r,
    }
}

pub async fn type_history(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    let Some(p) = path_of(&format!("types~{id}")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such type.") };
    history_of(&c, &p).await
}

async fn rollback(c: &Console, a: &Admin, action: &str, path: &str, version: u64) -> Response {
    let body = json!({ "sub": sub(a), "path": path, "version": version });
    match recorded(c, a, action, path, None, Some(json!({ "to_version": version })), c.kernel.call(Method::POST, "/v1/admin/knowledge/rollback", Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

pub async fn type_rollback(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(p) = path_of(&format!("types~{id}")) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such type.") };
    rollback(&c, &a, "ontology_type_rolled_back", &p, b["version"].as_u64().unwrap_or(0)).await
}

/// One version's text, to compare or to read before rolling back.
pub async fn version(State(c): State<Arc<Console>>, Path((id, v)): Path<(String, u32)>) -> Response {
    let Some(p) = path_of(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such document.") };
    match c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/version?{}", q(&[("path", &p), ("version", &v.to_string())])), None).await {
        Ok(d) => Json(json!({ "version": v, "text": crate::bundle::render(&d["frontmatter"], d["body"].as_str().unwrap_or_default()) })).into_response(),
        Err(e) => failed(e),
    }
}

// ---- Context -----------------------------------------------------------

/// The sections of what every agent receives, each the bundle's kinds.
fn section_kinds(s: &str) -> Option<&'static str> {
    match s {
        "map" => Some("index"),
        "structure" => Some("people"),
        "conventions" => Some("convention"),
        "sources" => Some("source"),
        _ => None,
    }
}

pub async fn context(State(c): State<Arc<Console>>, Path(section): Path<String>) -> Response {
    let Some(k) = section_kinds(&section) else { return refuse(StatusCode::NOT_FOUND, "not_found", "The sections are map, structure, conventions and sources.") };
    list(&c, k).await
}

/// One item of the context, to edit in place.
pub async fn document(State(c): State<Arc<Console>>, Path(id): Path<String>) -> Response {
    let Some(p) = path_of(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such document.") };
    match doc(&c, &p).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

/// Edit an item of a section in place (`{id, text}`), or add one.
pub async fn edit_context(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(section): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(k) = section_kinds(&section) else { return refuse(StatusCode::NOT_FOUND, "not_found", "The sections are map, structure, conventions and sources.") };
    let Some(p) = b["id"].as_str().and_then(path_of) else { return refuse(StatusCode::BAD_REQUEST, "invalid", "Name the item.") };
    if crate::bundle::place(&p).map(|x| x.0) != Ok(k) {
        return refuse(StatusCode::BAD_REQUEST, "invalid", &format!("{p} is not in the {section} section."));
    }
    write(&c, &a, "context_changed", &p, b["text"].as_str().unwrap_or_default()).await
}

/// The map as one person's agent receives it.
pub async fn assembled(State(c): State<Arc<Console>>, Query(qs): Query<HashMap<String, String>>) -> Response {
    let person = qs.get("person").cloned().unwrap_or_default();
    match c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/assembled?{}", q(&[("person", &person)])), None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

// ---- Memory ------------------------------------------------------------

pub async fn memory(State(c): State<Arc<Console>>) -> Response {
    list(&c, "entity").await
}

fn memory_path(id: &str) -> Option<String> {
    path_of(id).filter(|p| p.starts_with("/entities/"))
}

pub async fn edit_memory(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(p) = memory_path(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such entry.") };
    write(&c, &a, "memory_changed", &p, b["text"].as_str().unwrap_or_default()).await
}

pub async fn delete_memory(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>) -> Response {
    let Some(p) = memory_path(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such entry.") };
    remove(&c, &a, "memory_deleted", &p).await
}

pub async fn memory_rollback(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(p) = memory_path(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such entry.") };
    rollback(&c, &a, "memory_rolled_back", &p, b["version"].as_u64().unwrap_or(0)).await
}

/// Delete, or roll back to their previous version, many entries at once:
/// a fresh password first. Each is done and answered on its own.
pub async fn memory_bulk(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    let action = b["action"].as_str().unwrap_or_default();
    if !matches!(action, "delete" | "rollback") {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "In bulk: delete or rollback.");
    }
    let ids: Vec<String> = b["ids"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
    if ids.is_empty() || ids.len() > 500 {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Choose from 1 to 500 entries.");
    }
    let mut results = vec![];
    for id in ids {
        let Some(p) = memory_path(&id) else {
            results.push(json!({ "id": id, "done": false, "why": "no such entry" }));
            continue;
        };
        let r = if action == "delete" {
            remove(&c, &a, "memory_deleted", &p).await
        } else {
            match doc(&c, &p).await {
                Ok(d) if d["version"].as_u64().unwrap_or(1) > 1 => rollback(&c, &a, "memory_rolled_back", &p, d["version"].as_u64().unwrap_or(1) - 1).await,
                Ok(_) => refuse(StatusCode::CONFLICT, "first_version", "It has no earlier version."),
                Err(r) => r,
            }
        };
        let ok = r.status().is_success();
        let why = if ok {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&axum::body::to_bytes(r.into_body(), 64 * 1024).await.unwrap_or_default()).map(|v| v["message"].clone()).unwrap_or(Value::Null)
        };
        results.push(json!({ "id": id, "done": ok, "why": why }));
    }
    Json(json!({ "results": results })).into_response()
}

// ---- Activity ----------------------------------------------------------

pub async fn activity(State(c): State<Arc<Console>>, Query(qs): Query<HashMap<String, String>>) -> Response {
    let agents = if qs.get("everyone").is_some_and(|e| e == "1") { "0" } else { "1" };
    match c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/activity?{}", q(&[("agents", agents)])), None).await {
        Ok(mut v) => {
            for ch in v["changes"].as_array_mut().into_iter().flatten() {
                ch["id"] = json!(id_of(ch["path"].as_str().unwrap_or_default()));
            }
            Json(v).into_response()
        }
        Err(e) => failed(e),
    }
}

/// One change, with what was there before and after.
pub async fn change(State(c): State<Arc<Console>>, Path(seq): Path<i64>) -> Response {
    match c.kernel.call(Method::GET, &format!("/v1/admin/knowledge/activity/{seq}"), None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failed(e),
    }
}

async fn revert(c: &Console, a: &Admin, seqs: Vec<i64>) -> Response {
    let body = json!({ "sub": sub(a), "seqs": seqs });
    match recorded(c, a, "knowledge_reverted", "activity", None, Some(json!({ "changes": seqs })), c.kernel.call(Method::POST, "/v1/admin/knowledge/revert", Some(&body))).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

pub async fn revert_one(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Path(seq): Path<i64>) -> Response {
    revert(&c, &a, vec![seq]).await
}

/// Undo a selection of changes as one batch: a fresh password first.
pub async fn revert_batch(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Err(r) = a.step_up() {
        return r;
    }
    let seqs: Vec<i64> = b["seqs"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
    if seqs.is_empty() || seqs.len() > 500 {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Choose from 1 to 500 changes.");
    }
    revert(&c, &a, seqs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_one_segment_and_never_leaves_the_bundle() {
        assert_eq!(path_of("entities~customer~acme").as_deref(), Some("/entities/customer/acme.md"));
        assert_eq!(id_of("/entities/customer/acme.md"), "entities~customer~acme");
        for bad in ["", "a~~b", "~a", "a~..~b", "a/b", "a%2Fb", "a b"] {
            assert!(path_of(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn okf_text_splits_into_frontmatter_and_body() {
        let (fm, body) = parse("---\ntype: Entity Type\ntitle: Customer\n---\n\nA buyer.\n").unwrap();
        assert_eq!((fm["title"].as_str(), body.as_str()), (Some("Customer"), "A buyer.\n"));
        assert_eq!(parse("Just a body.").unwrap().1, "Just a body.");
        assert!(parse("---\ntitle: [\n---\nx").is_err());
        assert!(parse("---\ntitle: x\n").is_err());
    }
}
