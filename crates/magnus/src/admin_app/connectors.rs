//! Connectors in the Admin section (spec rev 141, Connectors): "Which
//! connectors people may connect, which are connected for the whole
//! workspace, and who has connected what." Turning a connector off or
//! limiting it to groups is the console's control route; so are adding,
//! removing and the organisation's credential for one. Suspending one at
//! once is the kill switch's connector level, in the Admin top bar.

use super::mount::{audited, body_json, control, pass, Audit};
use super::Admin;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/connectors", get(list).post(pass(Some("connector_added"))))
        .route("/v1/admin/connectors/{name}", patch(pass(Some("connector_updated"))).delete(pass(Some("connector_removed"))))
        .route("/v1/admin/connectors/{name}/credential", put(set_credential).delete(revoke_credential))
}

/// Each person who connected `app`, from their own sign-in in the vault
/// (`connector_<app>__<person>`) and from the connect screens they accepted.
pub fn connected(vault: &[String], consents: &BTreeMap<String, BTreeSet<String>>, app: &str) -> Vec<String> {
    let prefix = format!("{}__", crate::connectors::credential_name(app));
    let mut people: BTreeSet<String> = vault.iter().filter_map(|n| n.strip_prefix(&prefix)).filter(|p| !p.is_empty()).map(String::from).collect();
    people.extend(consents.get(app).into_iter().flatten().cloned());
    people.into_iter().collect()
}

/// Every connector with who connected it and the workspaces it is granted
/// to, for the roles each grant names.
async fn list(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let r = control(&k, &a, Method::GET, "/v1/admin/connectors", None).await;
    if !r.status().is_success() {
        return r;
    }
    let mut v = body_json(r).await;
    let vault = k.vault.names().unwrap_or_default();
    let (consents, grants) = k.read(|s| {
        let grants: Vec<(String, String, String, Vec<crate::team::Role>)> = s
            .team
            .all()
            .flat_map(|w| w.connectors.iter().filter(|(_, r)| !r.is_empty()).map(|(app, r)| (app.clone(), w.id.clone(), w.name.clone(), r.clone())).collect::<Vec<_>>())
            .collect();
        (s.tools.consents.clone(), grants)
    });
    if let Some(list) = v["connectors"].as_array_mut() {
        for c in list {
            let name = c["name"].as_str().unwrap_or_default().to_string();
            let people: Vec<Value> = connected(&vault, &consents, &name)
                .into_iter()
                .map(|p| json!({ "id": p.clone(), "name": k.person(&p).map(|x| x.name).filter(|n| !n.is_empty()).unwrap_or(p) }))
                .collect();
            c["connected_by"] = json!(people);
            c["workspaces"] = json!(grants.iter().filter(|g| g.0 == name).map(|g| json!({ "id": g.1, "name": g.2, "roles": g.3 })).collect::<Vec<_>>());
        }
    }
    Json(v).into_response()
}

/// The organisation's credential for an app: in, never out, and never in
/// the audit.
async fn set_credential(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let audit = Audit { action: "connector_credential_set".into(), target: Some(format!("connectors/{name}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::PUT, &format!("/v1/admin/connectors/{name}/credential"), Some(json!({ "value": b["value"] })))).await
}

async fn revoke_credential(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(name): Path<String>) -> Response {
    let audit = Audit { action: "connector_credential_revoked".into(), target: Some(format!("connectors/{name}")), ..Default::default() };
    audited(&k, &a, audit, control(&k, &a, Method::DELETE, &format!("/v1/admin/connectors/{name}/credential"), None)).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn who_connected_comes_from_their_sign_ins_and_connect_screens() {
        let vault = vec![format!("{}__ann", crate::connectors::credential_name("gmail")), format!("{}__bo", crate::connectors::credential_name("gmail_x")), "other".into()];
        let consents = [("gmail".to_string(), ["cy".to_string()].into_iter().collect())].into_iter().collect();
        assert_eq!(super::connected(&vault, &consents, "gmail"), vec!["ann".to_string(), "cy".to_string()]);
    }
}
