//! The identity provider, as the owner sets it in Admin › Security (spec rev
//! 141, Security: "Sign-in policy (identity provider, two-factor, session
//! length)"). The config's `[sign_in.oidc]` is where a server starts; the
//! owner can point it at another OpenID Connect provider, or turn the
//! provider off so people sign in with a password kept here.
//!
//! Who signs in through the provider is who it says they are, the owner
//! included, so choosing it is choosing who can be the owner: only an owner
//! sets it, never an admin. And a provider that cannot sign the owner in
//! locks them out, so a switch is made only after a test sign-in through the
//! new provider, on its own page (the device-code grant), as the owner making
//! the change, within `IDP_TEST_VALID_SECONDS` before it. Turning the
//! provider off needs the owner's password set here. Either way the owner's
//! backup (their password here and a code printed on the server's machine)
//! stays, as it always has.
//!
//! The settings are validated before the test: only the fields named in
//! `SETTABLE`, an https issuer (or one on this machine), a client id, and a
//! status client's secret that is in the vault. The change is an
//! `admin_setting` event, audited first, and takes effect at once; a server
//! starting again builds the provider from it.

use super::mount::{audited, Audit};
use super::people::refuse;
use super::settings::Change;
use super::Admin;
use crate::config::{Config, Oidc};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::oidc::Provider;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// How long a passed test sign-in lets the owner switch to that provider.
pub const IDP_TEST_VALID_SECONDS: i64 = 600;
/// The provider settings an owner sets here. A certificate authority file
/// and the sign-in limits stay the config's.
pub const SETTABLE: [&str; 10] = ["issuer", "client_id", "person_claim", "scope", "label", "groups_claim", "roles_claim", "status_client", "status_credential", "status_admin_url"];

/// The provider people sign in through now, if any.
pub struct Slot(RwLock<Option<Arc<Provider>>>);

impl Slot {
    /// The owner's choice if they made one, else the config's. A choice that
    /// no longer builds (its secret gone from the vault, say) leaves the
    /// config's in place rather than stopping the server.
    pub fn at_boot(cfg: &Config, s: &crate::state::State, vault: Arc<crate::vault::Vault>) -> anyhow::Result<Slot> {
        let configured = cfg.sign_in.oidc.clone().map(|o| Provider::new(o, vault.clone())).transpose()?;
        let p = match &s.admin_settings.identity_provider {
            None => configured,
            Some(None) => None,
            Some(Some(v)) => match oidc_of(v).map_err(anyhow::Error::msg).and_then(|o| Provider::new(o, vault.clone())) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!("the identity provider the owner chose does not build, so the config's is used: {e:#}");
                    configured
                }
            },
        };
        Ok(Slot(RwLock::new(p.map(Arc::new))))
    }

    pub fn get(&self) -> Option<Arc<Provider>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, p: Option<Arc<Provider>>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = p;
    }
}

impl Kernel {
    /// Whether a password set here signs this person in: everyone's way in
    /// without a provider, the owner's backup alone with one
    /// (`Config::may_use_password`, for the provider in use now).
    pub fn may_use_password(&self, id: &str) -> bool {
        self.cfg.person(id).is_some_and(|p| self.oidc.get().is_none() || p.is_owner())
    }
}

/// A provider's settings, only the settable fields, as given.
fn settings_of(b: &Value) -> Result<Value, String> {
    let o = b.as_object().ok_or("The provider's settings are an object.")?;
    if let Some(k) = o.keys().find(|k| !SETTABLE.contains(&k.as_str())) {
        return Err(format!("{k} is not set here; the settings are {}.", SETTABLE.join(", ")));
    }
    let mut out = Map::new();
    for (k, v) in o {
        match v {
            Value::Null => {}
            Value::String(s) if s.trim().is_empty() => {}
            Value::String(s) => {
                out.insert(k.clone(), json!(s.trim()));
            }
            _ => return Err(format!("{k} is text.")),
        }
    }
    for k in ["issuer", "client_id"] {
        if !out.contains_key(k) {
            return Err(format!("The provider needs its {k}."));
        }
    }
    Ok(Value::Object(out))
}

fn oidc_of(v: &Value) -> Result<Oidc, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("The provider's settings do not read: {e}"))
}

/// Settings checked as far as they can be without the provider: their
/// fields, the addresses (https), and the status client's secret.
fn checked(k: &Kernel, b: &Value) -> Result<(Value, Provider), String> {
    let v = settings_of(b)?;
    let o = oidc_of(&v)?;
    if let Some(name) = &o.status_credential {
        if !matches!(k.vault.get(name), Ok(Some(_))) {
            return Err(format!("The vault holds no secret named {name}."));
        }
    }
    let p = Provider::new(o, k.vault.clone()).map_err(|e| format!("{e:#}"))?;
    Ok((v, p))
}

/// The settings in use, as the form shows them.
fn shown(k: &Kernel) -> Value {
    if let Some(chosen) = k.read(|s| s.admin_settings.identity_provider.clone()) {
        return json!({ "source": "owner", "settings": chosen });
    }
    match &k.cfg.sign_in.oidc {
        Some(o) => json!({ "source": "config", "settings": {
            "issuer": o.issuer, "client_id": o.client_id, "person_claim": o.person_claim, "scope": o.scope, "label": o.label,
            "groups_claim": o.groups_claim, "roles_claim": o.roles_claim, "status_client": o.status_client,
            "status_credential": o.status_credential, "status_admin_url": o.status_admin_url,
        } }),
        None => json!({ "source": "config", "settings": null }),
    }
}

/// A test sign-in under way or passed, by the owner making it.
struct Test {
    settings: Value,
    provider: Arc<Provider>,
    flow: String,
    passed: Option<DateTime<Utc>>,
}

fn tests() -> &'static Mutex<HashMap<String, Test>> {
    static T: OnceLock<Mutex<HashMap<String, Test>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/admin/identity-provider", get(current).put(set))
        .route("/v1/admin/identity-provider/test", post(test))
        .route("/v1/admin/identity-provider/test/poll", post(poll))
}

fn owner_only(a: &Admin) -> Option<Response> {
    (!a.person.is_owner()).then(|| refuse(StatusCode::FORBIDDEN, "owner_only", "Only an owner chooses the identity provider: who it signs in, it signs in as anyone here."))
}

async fn current(State(k): State<Arc<Kernel>>) -> Response {
    let mut v = shown(&k);
    v["settable"] = json!(SETTABLE);
    v["in_use"] = json!(k.oidc.get().map(|p| p.issuer().to_string()));
    Json(v).into_response()
}

/// Start a test sign-in through the provider these settings describe: the
/// owner approves it on the provider's own page.
async fn test(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Some(r) = owner_only(&a) {
        return r;
    }
    let (settings, provider) = match checked(&k, &b) {
        Ok(x) => x,
        Err(m) => return refuse(StatusCode::BAD_REQUEST, "invalid", &m),
    };
    let provider = Arc::new(provider);
    match provider.start(false).await {
        Ok(s) => {
            let started = json!({ "user_code": s.user_code, "verification_uri": s.verification_uri, "verification_uri_complete": s.verification_uri_complete, "expires_in": s.expires_in, "interval": s.interval });
            tests().lock().unwrap_or_else(|e| e.into_inner()).insert(a.person.id.clone(), Test { settings, provider, flow: s.flow, passed: None });
            Json(started).into_response()
        }
        Err(e) => refuse(StatusCode::BAD_GATEWAY, "provider", &format!("The provider did not start a sign-in: {e:#}")),
    }
}

/// Whether the test sign-in was approved, and as whom.
async fn poll(State(_k): State<Arc<Kernel>>, Extension(a): Extension<Admin>) -> Response {
    let Some((provider, flow)) = tests().lock().unwrap_or_else(|e| e.into_inner()).get(&a.person.id).map(|t| (t.provider.clone(), t.flow.clone())) else {
        return refuse(StatusCode::NOT_FOUND, "no_test", "Start a test sign-in first.");
    };
    use crate::identity::Polled;
    let (status, message) = match provider.poll(&flow).await {
        Ok(Polled::Pending) => ("pending", String::new()),
        Ok(Polled::Done { person, .. }) if person == a.person.id => {
            if let Some(t) = tests().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&a.person.id) {
                t.passed = Some(Utc::now());
            }
            ("passed", format!("You signed in through {} as yourself.", provider.label()))
        }
        Ok(Polled::Done { person, .. }) => ("failed", format!("{} signed you in as {person}, not as {}: people would not be who they are here.", provider.label(), a.person.id)),
        Ok(Polled::Failed(m) | Polled::Refused(m) | Polled::UseBrowser(m)) => ("failed", m),
        Ok(Polled::NeedsCode) => ("failed", "The provider asked for a code; finish on its page.".into()),
        Err(e) => ("failed", format!("{e:#}")),
    };
    Json(json!({ "status": status, "message": message })).into_response()
}

/// Switch to the provider just tested, or `{"provider": null}` to sign in
/// with passwords kept here.
async fn set(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    if let Some(r) = owner_only(&a) {
        return r;
    }
    let chosen: Option<(Value, Arc<Provider>)> = match b.get("provider") {
        Some(Value::Null) => {
            // Everyone signs in with a password kept here: the owner must have one.
            if !k.accounts.has_password(&a.person.id).await {
                return refuse(StatusCode::CONFLICT, "lockout", "Set your own password here first: with no provider, it is how you sign in.");
            }
            None
        }
        Some(p) => {
            let settings = match settings_of(p) {
                Ok(v) => v,
                Err(m) => return refuse(StatusCode::BAD_REQUEST, "invalid", &m),
            };
            let fresh = |t: &Test| t.settings == settings && t.passed.is_some_and(|at| Utc::now() - at < chrono::Duration::seconds(IDP_TEST_VALID_SECONDS));
            let passed = tests().lock().unwrap_or_else(|e| e.into_inner()).get(&a.person.id).filter(|t| fresh(t)).map(|t| t.provider.clone());
            let Some(provider) = passed else {
                return refuse(StatusCode::CONFLICT, "test_first", "Sign in through this provider as yourself first (Test sign-in), so the change cannot lock you out.");
            };
            Some((settings, provider))
        }
        None => return refuse(StatusCode::BAD_REQUEST, "invalid", "Send the provider's settings, or null for passwords kept here."),
    };
    let after = chosen.as_ref().map(|(v, _)| v.clone());
    let audit = Audit { action: "identity_provider_set".into(), target: Some("identity-provider".into()), before: Some(shown(&k)["settings"].clone()), after: Some(json!(after)) };
    let change = async {
        if let Err(e) = k.emit(&a.actor(), Ev::AdminSetting(Change::IdentityProvider { provider: after.clone() })).await {
            return refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}"));
        }
        k.oidc.set(chosen.map(|(_, p)| p));
        tests().lock().unwrap_or_else(|e| e.into_inner()).remove(&a.person.id);
        Json(shown(&k)).into_response()
    };
    audited(&k, &a, audit, change).await
}
