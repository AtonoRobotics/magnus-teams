//! The organisation's apps as an admin keeps them: the catalog, the apps
//! added by their address, who may use each, and the organisation's
//! credentials.

use super::discovery::{asks_sign_in, discover, own_name};
use super::requests::AddConnector;
use super::routes::name_from;
use super::standing::revoke;
use super::{credential_name, is_sign_in_of, title, SignInOwner, OAUTH};
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use serde_json::{json, Value};

/// A refusal: its status and what to tell the person.
pub type Refusal = (StatusCode, String);

pub(crate) fn refusal(st: StatusCode, m: impl Into<String>) -> Refusal {
    (st, m.into())
}

pub(crate) fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("connectors: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

/// The organisation's credential for an app, as an admin sets it: the app's
/// own key, or the client secret of the provider its people sign in
/// through. None: each person signs in with their own account, and the
/// organisation holds nothing for it.
pub enum OrgCredential {
    Key(String),
    Registration(String),
}

pub fn org_credential(c: &Connector) -> Option<OrgCredential> {
    if let Some(p) = c.registration.as_deref().filter(|p| crate::catalog::client(p).is_some_and(|cl| cl.needs_secret())) {
        return Some(OrgCredential::Registration(p.to_string()));
    }
    match (&c.credential, c.auth.as_deref()) {
        (Some(name), a) if a != Some(OAUTH) => Some(OrgCredential::Key(name.clone())),
        _ => None,
    }
}

impl Kernel {
    /// Every app as an admin sees it (admin console spec, Security ›
    /// Connectors and credentials): its status, how people reach it, its
    /// scopes, the organisation's credential (only whether it is set, and
    /// when), how many people are signed in, and when anyone last used it.
    pub fn admin_connectors(&self) -> Vec<Value> {
        let (added, changed, registered, people) = self.read(|s| {
            (
                s.apps.connectors().keys().cloned().collect::<Vec<String>>(),
                s.apps.credential_changed().clone(),
                s.apps.registrations().clone(),
                s.runs.values().map(|r| r.person.clone()).collect::<std::collections::BTreeSet<String>>(),
            )
        });
        let vault: Vec<String> = self.vault.names().unwrap_or_default();
        self.org_connectors()
            .into_iter()
            .map(|(c, on)| {
                let source = if self.cfg.connectors.iter().any(|f| f.name == c.name) {
                    "config"
                } else if added.contains(&c.name) {
                    "added"
                } else {
                    "catalog"
                };
                let (kind, set, key) = match org_credential(&c) {
                    Some(OrgCredential::Registration(p)) => ("client_secret", registered.contains(&p), Some(format!("registration:{p}"))),
                    Some(OrgCredential::Key(name)) => ("key", vault.contains(&name), Some(c.name.clone())),
                    None => ("none", false, None),
                };
                let at = key.as_ref().and_then(|k| changed.get(k)).map(|(_, at)| *at);
                // People only: a shared project's own sign-in is not a person's.
                let signed_in = vault.iter().filter(|n| matches!(SignInOwner::of_name(n, &c.name), Some(SignInOwner::Person(_)))).count();
                let last_used = people.iter().filter_map(|p| self.recent_uses(&c.name, p, 1).first().and_then(|u| u["at"].as_str().map(String::from))).max();
                // Its tools are accepted when someone connects it (spec rev
                // 140): nothing waits on an admin's review.
                let status = if !on {
                    "off"
                } else if kind != "none" && !set {
                    "needs_credential"
                } else {
                    "ready"
                };
                json!({
                    "name": c.name, "title": c.title.clone().unwrap_or_else(|| title(&c.name)), "source": source, "enabled": on, "status": status,
                    "sign_in": if c.auth.as_deref() == Some(OAUTH) { "own_account" } else if c.credential.is_some() { "org_key" } else { "none" },
                    "scopes": c.scopes, "url": c.url, "audience": c.audience, "label": c.label, "access_groups": c.access_groups, "blocked_groups": c.blocked_groups,
                    "registration": c.registration,
                    "credential": { "kind": kind, "set": set, "changed_at": at },
                    "signed_in": signed_in, "last_used": last_used, "tools": c.tools.len(),
                })
            })
            .collect()
    }

    /// Every app but the ones a person built for themselves, which are theirs
    /// alone: an admin neither reads their definition nor sets their token.
    fn org_connectors(&self) -> Vec<(Connector, bool)> {
        self.all_connectors().into_iter().filter(|(c, _)| c.owner.is_none()).collect()
    }

    /// Add an app by its MCP address, as `actor`. Whether it needs a
    /// sign-in is asked of the app itself.
    pub(crate) async fn add_connector(&self, actor: &str, b: &AddConnector) -> Result<Value, Refusal> {
        let s = |f: &str| f.trim().to_string();
        let (mut name, url, audience, label) = (s(&b.name), s(&b.url), s(&b.audience), s(&b.label));
        let taken = |n: &str| self.all_connectors().iter().any(|(c, _)| c.name == n) || crate::tools::builtin_apps().contains(&n) || n.starts_with(crate::desktop::APP_PREFIX);
        // Never a key or token in the address, checked before the app is asked.
        match url::Url::parse(&url) {
            Ok(u) if u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none() => {}
            Ok(_) => return Err(refusal(
                StatusCode::BAD_REQUEST,
                "the app's address takes no user, password, query or fragment: Magnus never takes a key or token for an app, and each person signs in to it with their own account",
            )),
            Err(_) => return Err(refusal(StatusCode::BAD_REQUEST, "the app's address is not a URL")),
        }
        // The address is the one field the owner fills in (connectors spec):
        // the name comes from the app. (The app's form sends what it returns
        // and what reaches it at their most guarded; they stay named here.)
        if name.is_empty() {
            let own = own_name(&self.app_egress(), &url).await;
            name = name_from(&url, own.as_deref(), taken);
        }
        if name.is_empty() || name.contains("__") || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            return Err(refusal(StatusCode::BAD_REQUEST, "the name must be letters, digits, '-' or '_', without '__'"));
        }
        if taken(&name) {
            return Err(refusal(StatusCode::CONFLICT, "an app with this name exists"));
        }
        if crate::tools::Audience::parse(&audience).is_none() {
            return Err(refusal(StatusCode::BAD_REQUEST, "audience is private, internal or public: who can see what the worker sends there"));
        }
        // Named, never assumed: an empty label would read as "principal", and
        // the app's results would taint nothing.
        if label.is_empty() || crate::tools::Label::parse(&label).is_none() {
            return Err(refusal(StatusCode::BAD_REQUEST, "label is what the app returns: principal, private, untrusted, or private+untrusted"));
        }
        if !url.starts_with("https://") && !(url.starts_with("http://") && self.cfg.web.allow_private_networks) {
            return Err(refusal(StatusCode::BAD_REQUEST, "the app's address must be https://"));
        }
        // Never a key, token or client details: people sign in to an app
        // with their own accounts, and nobody types one into Magnus
        // (connectors spec, Remote MCP connectors and Client registration).
        if b.token.as_deref().is_some_and(|t| !t.trim().is_empty()) || b.client_id.as_ref().is_some_and(|c| !c.is_null()) {
            return Err(refusal(StatusCode::BAD_REQUEST, "Magnus never takes a key, token or client details for an app: each person signs in to it with their own account"));
        }
        let oauth = match asks_sign_in(&self.app_egress(), &url).await {
            Ok(None) => false,
            Ok(Some(meta)) => {
                let (asm, _, _) = discover(&self.app_egress(), &url, meta).await.map_err(|e| {
                    refusal(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!(
                            "This server asks for a key or token instead of a sign-in ({e:#}), so it cannot be added: people connect an app by signing in with their own accounts"
                        ),
                    )
                })?;
                // Alpha Vector's own clients are for the catalog's providers;
                // an added server registers Magnus itself.
                if asm["registration_endpoint"].as_str().is_none() {
                    return Err(refusal(StatusCode::UNPROCESSABLE_ENTITY, "This server's sign-in does not let Magnus register itself, and Magnus never asks for client details, so it cannot be added. Alpha Vector can bring it into the catalog with its own registration."));
                }
                true
            }
            Err(e) => return Err(refusal(StatusCode::BAD_GATEWAY, format!("{e:#}"))),
        };
        // A server that asks for no sign-in is called without a credential.
        let c = Connector {
            owner: None,
            filter_login_codes: false,
            keep_login_codes: false,
            name: name.clone(),
            transport: crate::transport::MCP_HTTP.into(),
            url,
            credential: oauth.then(|| credential_name(&name)),
            audience,
            label,
            access_groups: b.access_groups.clone(),
            blocked_groups: b.blocked_groups.clone(),
            system_of_record: b.system_of_record,
            idempotency: "none".into(),
            tools: Default::default(),
            auth: oauth.then(|| OAUTH.to_string()),
            client_id: None,
            scopes: vec![],
            registration: None,
            title: None,
            summary: None,
            catalog: false,
            hosts: vec![],
            issuer: None,
            pkce: true,
            redirect_uri: None,
            handoff: None,
            destinations: Default::default(),
            sent: None,
            read_back: Default::default(),
            watch: vec![],
        };
        self.emit(actor, Ev::ConnectorAdded { connector: c }).await.map_err(failed)?;
        Ok(json!({ "name": name, "sign_in": oauth }))
    }

    /// An app added from the app or the console (not the config file's, not
    /// the catalog's).
    pub fn added_connector(&self, name: &str) -> Result<Connector, Refusal> {
        if self.cfg.connectors.iter().any(|c| c.name == name) {
            return Err(refusal(StatusCode::CONFLICT, "this app is set in the server's config file; change it there"));
        }
        // One a person built is theirs alone (`generated.rs`), never the organisation's to change.
        match self.read(|s| s.apps.connectors().get(name).filter(|c| c.owner.is_none()).cloned()) {
            Some(c) => Ok(c),
            None if crate::catalog::manifest(name).is_some() => Err(refusal(StatusCode::CONFLICT, "this app is in the catalog: turn it off instead")),
            None => Err(refusal(StatusCode::NOT_FOUND, "no such app")),
        }
    }

    /// Revoke every sign-in to an app (each person's own, at the app, and
    /// the old shared one) and its key. The number revoked.
    pub(crate) async fn revoke_all(&self, c: &Connector) -> Result<usize, Refusal> {
        let names: Vec<String> = self.vault.names().map_err(failed)?.into_iter().filter(|n| is_sign_in_of(n, &c.name) || c.credential.as_ref() == Some(n)).collect();
        let mut n = 0;
        for cred in names {
            // A sign-in ends through the store, so no refresh on its way writes it back.
            let removed = if is_sign_in_of(&cred, &c.name) { self.connecting.store.end(&self.vault, &cred) } else { self.vault.remove(&cred) };
            match removed.map_err(failed)? {
                Some(s) if c.auth.as_deref() == Some(OAUTH) => {
                    let (done, detail) = revoke(self, s.expose()).await;
                    tracing::info!("{}: revoked {cred} {done}: {detail}", c.name);
                    n += 1;
                }
                Some(_) => n += 1,
                None => {}
            }
        }
        Ok(n)
    }

    /// Remove an added app: every sign-in to it revoked, its key deleted,
    /// and everything that names it gone with it (`gone.rs`).
    pub async fn remove_connector(&self, actor: &str, name: &str) -> Result<Value, Refusal> {
        let c = self.added_connector(name)?;
        let n = self.app_gone(actor, &c).await?;
        Ok(json!({ "sign_ins_revoked": n }))
    }

    /// Turn an app on or off for the organisation, or set who may use it:
    /// the groups it is for (empty: everyone) and the groups it is blocked
    /// for (connectors spec, Org policy).
    pub async fn connector_settings(
        &self,
        actor: &str,
        name: &str,
        enabled: Option<bool>,
        access_groups: Option<Vec<String>>,
        blocked_groups: Option<Vec<String>>,
    ) -> Result<Value, Refusal> {
        let Some((c, on)) = self.org_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
        let clean = |g: Option<Vec<String>>| g.map(|g| g.into_iter().map(|x| x.trim().to_string()).collect::<Vec<String>>());
        let (access_groups, blocked_groups) = (clean(access_groups), clean(blocked_groups));
        if [&access_groups, &blocked_groups].iter().any(|l| l.as_ref().is_some_and(|g| g.iter().any(|g| g.is_empty() || g.len() > 100))) {
            return Err(refusal(StatusCode::BAD_REQUEST, "each group is a group's name"));
        }
        let had = self.read(|s| s.apps.connector_settings().get(name).cloned()).unwrap_or_default();
        let groups = access_groups.or(had.access_groups);
        let blocked = blocked_groups.or(had.blocked_groups);
        let enabled = enabled.unwrap_or(on);
        self.emit(actor, Ev::ConnectorSettings { name: name.to_string(), enabled, access_groups: groups.clone(), blocked_groups: blocked.clone() }).await.map_err(failed)?;
        Ok(json!({ "name": name, "enabled": enabled, "access_groups": groups.unwrap_or(c.access_groups), "blocked_groups": blocked.unwrap_or(c.blocked_groups) }))
    }

    /// Set, or rotate, the organisation's credential for an app. Nothing
    /// shows it again: the log records only that it was set.
    pub async fn set_connector_credential(&self, actor: &str, name: &str, value: &str) -> Result<Value, Refusal> {
        let Some((c, _)) = self.org_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
        let value = value.trim();
        if value.is_empty() || value.len() > 16 * 1024 {
            return Err(refusal(StatusCode::BAD_REQUEST, "give the credential"));
        }
        match org_credential(&c) {
            Some(OrgCredential::Registration(p)) => self.set_registration(actor, &p, value).await,
            Some(OrgCredential::Key(cred)) => {
                self.vault.put(&cred, value).map_err(failed)?;
                self.emit(actor, Ev::CredentialSet { connector: name.to_string(), set: true }).await.map_err(failed)?;
                Ok(json!({ "set": true }))
            }
            None => Err(refusal(StatusCode::CONFLICT, "people sign in to this app with their own accounts: the organisation holds no credential for it")),
        }
    }

    /// The organisation's client secret with a provider (Google's, for its
    /// apps), until the relay holds it (connectors spec rev 46).
    pub async fn set_registration(&self, actor: &str, provider: &str, secret: &str) -> Result<Value, Refusal> {
        if crate::catalog::client(provider).is_none_or(|c| !c.needs_secret()) {
            return Err(refusal(StatusCode::NOT_FOUND, "no catalog provider takes a secret by that name"));
        }
        self.provision_registration(actor, provider, None, Some(secret)).await?;
        Ok(json!({ "set": true }))
    }

    /// Revoke the organisation's credential for an app, and every person's
    /// sign-in to it, at the app. A provider's client secret serves all of
    /// its apps: revoking it ends the sign-ins to each of them.
    pub async fn revoke_connector_credential(&self, actor: &str, name: &str) -> Result<Value, Refusal> {
        let Some((c, _)) = self.org_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
        match org_credential(&c) {
            Some(OrgCredential::Registration(p)) => {
                let mut n = 0;
                for (other, _) in self.all_connectors().into_iter().filter(|(o, _)| o.registration.as_deref() == Some(p.as_str())) {
                    n += self.revoke_all(&other).await?;
                }
                crate::registrations::clear_secret(&self.vault, &p).map_err(failed)?;
                self.emit(actor, Ev::RegistrationSet { provider: p, set: false }).await.map_err(failed)?;
                Ok(json!({ "sign_ins_revoked": n }))
            }
            Some(OrgCredential::Key(_)) => {
                let n = self.revoke_all(&c).await?;
                self.emit(actor, Ev::CredentialSet { connector: name.to_string(), set: false }).await.map_err(failed)?;
                Ok(json!({ "sign_ins_revoked": n }))
            }
            None => Ok(json!({ "sign_ins_revoked": self.revoke_all(&c).await? })),
        }
    }
}
