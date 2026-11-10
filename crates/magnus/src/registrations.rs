//! Alpha Vector's client with each catalog provider, as this server holds
//! it (connectors spec rev 46: "the appliance getting client IDs from the
//! relay"; until the relay exists, the release provisions them). The vault
//! entry `registration_<provider>` holds the client's secret and, when the
//! release provisioned one, its client ID: the server's own registration
//! with the provider, which may differ from the catalog's
//! (`catalog/registrations.toml`, compiled in, used when none is
//! provisioned). A client ID and its secret are one pair: sign-in uses
//! both from here, or the catalog's ID with the provisioned secret.
//! Nothing here ever answers with either value
//! (decisions/2026-11-10-provisioned-client-ids.md).

use crate::connectors::Refusal;
use crate::control::{actor, done, no_actor};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::vault::Vault;
use anyhow::{anyhow, Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::put;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

/// The longest client ID taken: providers issue far shorter ones.
pub const CLIENT_ID_MAX_BYTES: usize = 512;
/// The longest client secret taken, as for an app's key.
pub const CLIENT_SECRET_MAX_BYTES: usize = 16 * 1024;

fn stored(vault: &Vault, provider: &str) -> Result<serde_json::Map<String, Value>> {
    match vault.get(&crate::catalog::registration_name(provider))? {
        Some(raw) => match serde_json::from_str(raw.expose()).context("stored registration")? {
            Value::Object(m) => Ok(m),
            _ => Err(anyhow!("stored registration is not an object")),
        },
        None => Ok(serde_json::Map::new()),
    }
}

/// The client `provider`'s apps sign in through on this server: the client
/// ID provisioned here, else the catalog's, and its secret when the
/// provider takes one. An error when it takes one this server lacks.
pub fn client(vault: &Vault, provider: &str) -> Result<(String, Option<String>)> {
    let catalog = crate::catalog::client(provider).ok_or_else(|| anyhow!("no catalog client with {provider}"))?;
    let s = stored(vault, provider)?;
    let id = s.get("client_id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| catalog.client_id.clone());
    let secret = match catalog.needs_secret() {
        true => Some(s.get("client_secret").and_then(Value::as_str).ok_or_else(|| anyhow!("no client secret with {provider} here yet"))?.to_string()),
        false => None,
    };
    Ok((id, secret))
}

/// Keep what is given for `provider`, its client ID, its secret or both,
/// beside what is kept already. Whether a secret was set.
pub fn provision(vault: &Vault, provider: &str, client_id: Option<&str>, secret: Option<&str>) -> Result<bool, Refusal> {
    let failed = |e: anyhow::Error| -> Refusal {
        tracing::error!("registration: {e:#}");
        (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    };
    let Some(catalog) = crate::catalog::client(provider) else { return Err((StatusCode::NOT_FOUND, "no catalog provider by that name".into())) };
    let client_id = client_id.map(str::trim);
    let secret = secret.map(str::trim);
    if secret.is_some() && !catalog.needs_secret() {
        return Err((StatusCode::NOT_FOUND, "no catalog provider takes a secret by that name".into()));
    }
    if secret.is_some_and(|s| s.is_empty() || s.len() > CLIENT_SECRET_MAX_BYTES) {
        return Err((StatusCode::BAD_REQUEST, "give the provider's client secret".into()));
    }
    if client_id.is_some_and(|id| id.is_empty() || id.len() > CLIENT_ID_MAX_BYTES || id.chars().any(|c| c.is_whitespace() || c.is_control())) {
        return Err((StatusCode::BAD_REQUEST, "give the provider's client ID".into()));
    }
    if client_id.is_none() && secret.is_none() {
        return Err((StatusCode::BAD_REQUEST, "give the provider's client ID or secret".into()));
    }
    let mut s = stored(vault, provider).map_err(failed)?;
    if let Some(id) = client_id {
        s.insert("client_id".into(), json!(id));
    }
    if let Some(secret) = secret {
        s.insert("client_secret".into(), json!(secret));
    }
    vault.put(&crate::catalog::registration_name(provider), &Value::Object(s).to_string()).map_err(failed)?;
    Ok(secret.is_some())
}

/// Forget `provider`'s secret, keeping its provisioned client ID: a secret
/// set again later pairs with the same client.
pub fn clear_secret(vault: &Vault, provider: &str) -> Result<()> {
    let mut s = stored(vault, provider)?;
    s.remove("client_secret");
    match s.is_empty() {
        true => vault.remove(&crate::catalog::registration_name(provider)).map(|_| ()),
        false => vault.put(&crate::catalog::registration_name(provider), &Value::Object(s).to_string()),
    }
}

impl Kernel {
    /// Provision Alpha Vector's client with `provider`, as `actor`. The log
    /// records only that a secret was set, never a value.
    pub async fn provision_registration(&self, actor: &str, provider: &str, client_id: Option<&str>, secret: Option<&str>) -> Result<Value, Refusal> {
        let secret_set = provision(&self.vault, provider, client_id, secret)?;
        if secret_set {
            self.emit(actor, Ev::RegistrationSet { provider: provider.to_string(), set: true }).await.map_err(|e| {
                tracing::error!("registration: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            })?;
        }
        Ok(json!({ "client_id": if client_id.is_some() { "provisioned" } else { "kept" }, "secret": secret_set }))
    }
}

/// The control socket's routes for the organisation's credentials: an
/// app's, and a provider's client.
pub fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/connectors/{name}/credential", put(set_credential).delete(revoke_credential)).route("/v1/admin/registrations/{provider}", put(set_registration))
}

async fn set_credential(State(k): State<Arc<Kernel>>, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(k.set_connector_credential(&actor, &name, b["value"].as_str().unwrap_or_default()).await)
}

async fn revoke_credential(State(k): State<Arc<Kernel>>, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(k.revoke_connector_credential(&actor, &name).await)
}

/// The release's provisioning: `client_id`, `client_secret`, or both.
async fn set_registration(State(k): State<Arc<Kernel>>, Path(provider): Path<String>, Json(b): Json<Value>) -> Response {
    let Some(actor) = actor(&b) else { return no_actor() };
    done(k.provision_registration(&actor, &provider, b["client_id"].as_str(), b["client_secret"].as_str()).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        (d, v)
    }

    /// The magnus-teams server has its own clients with its providers
    /// (2026-11-10): sign-in must use the client ID provisioned beside the
    /// secret, not the catalog's, or the provider refuses the pair.
    #[test]
    fn sign_in_uses_the_provisioned_client_id_with_its_secret() {
        let (_d, v) = vault();
        assert!(client(&v, "github").is_err(), "a confidential client with no secret here");
        assert_eq!(provision(&v, "github", Some(" teams-id "), Some("s3cret")), Ok(true));
        assert_eq!(client(&v, "github").unwrap(), ("teams-id".to_string(), Some("s3cret".to_string())));
        // The owner rotating the secret keeps the provisioned client.
        assert_eq!(provision(&v, "github", None, Some("rotated")), Ok(true));
        assert_eq!(client(&v, "github").unwrap(), ("teams-id".to_string(), Some("rotated".to_string())));
        // Revoking forgets the secret and keeps the client.
        clear_secret(&v, "github").unwrap();
        assert!(client(&v, "github").is_err());
        assert_eq!(provision(&v, "github", None, Some("again")), Ok(true));
        assert_eq!(client(&v, "github").unwrap().0, "teams-id");
    }

    /// With nothing provisioned but a secret, the catalog's client ID.
    #[test]
    fn without_a_provisioned_client_id_the_catalogs_is_used() {
        let (_d, v) = vault();
        provision(&v, "slack", None, Some("s")).unwrap();
        let catalog = crate::catalog::client("slack").unwrap();
        assert_eq!(client(&v, "slack").unwrap(), (catalog.client_id.clone(), Some("s".to_string())));
    }

    /// A public client takes a client ID and never a secret.
    #[test]
    fn a_public_client_takes_an_id_and_no_secret() {
        let (_d, v) = vault();
        assert_eq!(client(&v, "vercel").unwrap().1, None);
        assert_eq!(provision(&v, "vercel", None, Some("s")).unwrap_err().0, StatusCode::NOT_FOUND);
        assert_eq!(provision(&v, "vercel", Some("cl_teams"), None), Ok(false));
        assert_eq!(client(&v, "vercel").unwrap(), ("cl_teams".to_string(), None));
        clear_secret(&v, "vercel").unwrap();
        assert_eq!(client(&v, "vercel").unwrap().0, "cl_teams");
    }

    #[test]
    fn what_is_not_a_client_is_refused() {
        let (_d, v) = vault();
        assert_eq!(provision(&v, "asana", Some("id"), Some("s")).unwrap_err().0, StatusCode::NOT_FOUND);
        for bad in ["", "two words", "line\nbreak"] {
            assert_eq!(provision(&v, "github", Some(bad), Some("s")).unwrap_err().0, StatusCode::BAD_REQUEST, "{bad:?}");
        }
        assert_eq!(provision(&v, "github", Some(&"x".repeat(CLIENT_ID_MAX_BYTES + 1)), None).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert_eq!(provision(&v, "github", None, Some(" ")).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert_eq!(provision(&v, "github", None, None).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert!(v.get(&crate::catalog::registration_name("github")).unwrap().is_none(), "nothing kept from a refusal");
    }
}
