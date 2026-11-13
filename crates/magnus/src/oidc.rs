//! Sign-in through an OpenID Connect provider (Keycloak, for one).
//!
//! People type their username and password in the app, as they do for any
//! other app. The appliance checks them with the provider's password grant
//! (Keycloak calls it Direct access grants) over TLS, with the code from an
//! authenticator app when the person uses one, and keeps neither. The
//! password grant answers a wrong password and a missing code alike, so a
//! service account that may only view users tells the appliance who uses a
//! code.
//!
//! A passkey can't be used in the app's form, so it signs in in the
//! person's own browser with the authorization code and PKCE (RFC 7636),
//! coming back to a loopback address on their computer (RFC 8252): the
//! appliance makes the verifier, state and nonce and keeps them, the app
//! passes back the code, and the appliance exchanges it.
//!
//! A browser on the appliance's own page can sign in with the device-code
//! grant (RFC 8628): the person approves a code on the provider's page, and
//! the appliance collects the ID token itself.
//!
//! The ID token comes straight from the provider's token endpoint over TLS,
//! so TLS stands in for checking its signature (OpenID Connect Core 1.0,
//! 3.1.3.7, step 6); the issuer, audience, expiry and nonce are still
//! checked.

use crate::config::Oidc as Cfg;
pub use crate::identity::{Begun, Polled, Rights};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The longest one call to the identity provider may take.
const PROVIDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Deserialize, Clone, Debug)]
struct Discovery {
    issuer: String,
    token_endpoint: String,
    authorization_endpoint: Option<String>,
    device_authorization_endpoint: Option<String>,
}

/// A sign-in in the person's browser, by its `state`.
struct CodeFlow {
    verifier: String,
    nonce: String,
    redirect_uri: String,
    expires: DateTime<Utc>,
    app: bool,
}

struct Pending {
    device_code: String,
    /// PKCE for the device grant too: a provider that requires it for the
    /// client (Keycloak does, once S256 is set) requires it here.
    verifier: String,
    interval: Duration,
    next_poll: DateTime<Utc>,
    expires: DateTime<Utc>,
    app: bool,
}

/// What the person needs to approve a sign-in.
pub struct Started {
    pub flow: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    pub interval: i64,
}

/// A list of names at a JSON pointer. Keycloak writes groups as paths
/// (`/finance`); the leading slash is not part of the name.
fn names_at(c: &Value, pointer: &Option<String>) -> Option<Vec<String>> {
    let p = pointer.as_deref()?;
    let v = c.pointer(p).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    Some(v.iter().filter_map(|x| x.as_str()).map(|s| s.trim_start_matches('/').to_string()).filter(|s| !s.is_empty()).collect())
}

pub struct Provider {
    cfg: Cfg,
    /// Holds the status client's secret.
    vault: Arc<crate::vault::Vault>,
    http: reqwest::Client,
    /// For Keycloak's admin API, trusting `status_admin_ca` too.
    admin_http: reqwest::Client,
    discovery: tokio::sync::OnceCell<Discovery>,
    pending: Mutex<BTreeMap<String, Pending>>,
    code_flows: Mutex<BTreeMap<String, CodeFlow>>,
}

/// Only https, except on this machine (for tests and a local provider).
fn checked(url: &str) -> Result<url::Url> {
    let u = url::Url::parse(url).with_context(|| format!("{url} is not an address"))?;
    let local =
        matches!(u.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback()) || matches!(u.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback()) || u.host_str() == Some("localhost");
    if u.scheme() != "https" && !(u.scheme() == "http" && local) {
        bail!("{url} is not https");
    }
    Ok(u)
}

/// The claims of an ID token, without checking its signature (see above).
fn claims(id_token: &str) -> Result<Value> {
    let payload = id_token.split('.').nth(1).ok_or_else(|| anyhow!("the ID token is not a JWT"))?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).context("the ID token does not decode")?;
    serde_json::from_slice(&raw).context("the ID token's claims do not parse")
}

/// The person an ID token names, after checking it is for us and current.
fn person(c: &Value, issuer: &str, client_id: &str, claim: &str, now: DateTime<Utc>) -> Result<String> {
    if c["iss"].as_str() != Some(issuer) {
        bail!("the ID token is from {}, not {issuer}", c["iss"]);
    }
    let aud_ok = match &c["aud"] {
        Value::String(a) => a == client_id,
        Value::Array(a) => a.iter().any(|x| x.as_str() == Some(client_id)),
        _ => false,
    };
    if !aud_ok || c.get("azp").and_then(|v| v.as_str()).is_some_and(|azp| azp != client_id) {
        bail!("the ID token is not for client {client_id}");
    }
    let exp = c["exp"].as_i64().ok_or_else(|| anyhow!("the ID token has no expiry"))?;
    if exp <= now.timestamp() {
        bail!("the ID token has expired");
    }
    let who = c[claim].as_str().ok_or_else(|| anyhow!("the ID token has no {claim} claim"))?;
    Ok(who.trim().to_lowercase())
}

/// What the provider's admin API says of a person's account, for the
/// password typed in the app.
#[derive(Debug, PartialEq)]
pub struct Account {
    pub enabled: bool,
    /// A code from an authenticator app, which the password grant checks.
    pub otp: bool,
    /// A second factor or a step only the provider's own page handles: a
    /// security key or passkey, or a required action (a new password,
    /// setting up the authenticator app). Keycloak's password grant skips
    /// both, so the app's password alone must not sign such a person in.
    pub browser_only: bool,
}

impl Account {
    pub fn of(user: &Value, creds: &[Value]) -> Account {
        let kinds: Vec<&str> = creds.iter().filter_map(|c| c["type"].as_str()).collect();
        Account {
            enabled: user["enabled"].as_bool().unwrap_or(false),
            otp: kinds.contains(&"otp"),
            browser_only: kinds.iter().any(|k| k.starts_with("webauthn")) || user["requiredActions"].as_array().is_some_and(|a| !a.is_empty()),
        }
    }
}

impl Provider {
    pub fn new(cfg: Cfg, vault: Arc<crate::vault::Vault>) -> Result<Provider> {
        checked(&cfg.issuer)?;
        // No redirects: a 307 from the token endpoint would replay the POST,
        // password or client secret included, to wherever it points, plain
        // http among them. Every address used is checked by `checked`.
        let http = reqwest::Client::builder().timeout(PROVIDER_TIMEOUT).redirect(reqwest::redirect::Policy::none()).build()?;
        if let Some(u) = &cfg.status_admin_url {
            checked(u)?;
        }
        let mut admin = reqwest::Client::builder().timeout(PROVIDER_TIMEOUT).redirect(reqwest::redirect::Policy::none());
        if let Some(ca) = &cfg.status_admin_ca {
            let pem = std::fs::read(ca).with_context(|| format!("reading status_admin_ca {}", ca.display()))?;
            admin = admin.add_root_certificate(reqwest::Certificate::from_pem(&pem).with_context(|| format!("{} is not a PEM certificate", ca.display()))?);
        }
        let admin_http = admin.build()?;
        Ok(Provider { cfg, vault, http, admin_http, discovery: tokio::sync::OnceCell::new(), pending: Mutex::new(BTreeMap::new()), code_flows: Mutex::new(BTreeMap::new()) })
    }

    pub fn issuer(&self) -> &str {
        &self.cfg.issuer
    }

    /// What the sign-in button names.
    pub fn label(&self) -> String {
        self.cfg.label.clone().unwrap_or_else(|| url::Url::parse(&self.cfg.issuer).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| self.cfg.issuer.clone()))
    }

    async fn discovery(&self) -> Result<&Discovery> {
        self.discovery
            .get_or_try_init(|| async {
                let url = format!("{}/.well-known/openid-configuration", self.cfg.issuer.trim_end_matches('/'));
                let d: Discovery = self.http.get(&url).send().await?.error_for_status()?.json().await.with_context(|| format!("read {url}"))?;
                if d.issuer != self.cfg.issuer {
                    bail!("{url} names issuer {}, not {}", d.issuer, self.cfg.issuer);
                }
                checked(&d.token_endpoint)?;
                for e in [&d.authorization_endpoint, &d.device_authorization_endpoint].into_iter().flatten() {
                    checked(e)?;
                }
                Ok(d)
            })
            .await
    }

    /// Room for one more waiting sign-in, after forgetting the expired.
    fn room(&self, now: DateTime<Utc>) -> Result<()> {
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let mut c = self.code_flows.lock().unwrap_or_else(|e| e.into_inner());
        p.retain(|_, f| f.expires > now);
        c.retain(|_, f| f.expires > now);
        if p.len() + c.len() >= self.cfg.max_pending {
            bail!("too many sign-ins are waiting; try again in a few minutes");
        }
        Ok(())
    }

    /// The person and their rights from a token response, after checking
    /// the ID token is for us, current, and (in a browser sign-in) answers
    /// the nonce we sent.
    fn signed_in(&self, d: &Discovery, b: &Value, nonce: Option<&str>, now: DateTime<Utc>) -> Result<(String, Rights)> {
        let id_token = b["id_token"].as_str().ok_or_else(|| anyhow!("{} sent no ID token (is \"openid\" in the scope?)", self.label()))?;
        let c = claims(id_token)?;
        let person = person(&c, &d.issuer, &self.cfg.client_id, &self.cfg.person_claim, now)?;
        if let Some(n) = nonce {
            if c["nonce"].as_str() != Some(n) {
                bail!("the ID token does not answer this sign-in's nonce");
            }
        }
        Ok((
            person,
            Rights {
                groups: names_at(&c, &self.cfg.groups_claim),
                roles: names_at(&c, &self.cfg.roles_claim),
                subject: c["sub"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from),
            },
        ))
    }

    /// Begin a sign-in in the person's browser that comes back to
    /// `redirect_uri`, a loopback address on their computer.
    pub async fn begin(&self, redirect_uri: &str, login_hint: Option<&str>, app: bool) -> Result<Begun> {
        crate::connectors::loopback(redirect_uri)?;
        let now = Utc::now();
        self.room(now)?;
        let d = self.discovery().await?;
        let endpoint = d.authorization_endpoint.as_deref().ok_or_else(|| anyhow!("{} does not offer the authorization code grant", self.label()))?;
        let (verifier, state, nonce) = (crate::connectors::random(), crate::connectors::random(), crate::connectors::random());
        let mut u = url::Url::parse(endpoint)?;
        u.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &self.cfg.scope)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &crate::connectors::challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        if let Some(h) = login_hint {
            u.query_pairs_mut().append_pair("login_hint", h);
        }
        let expires_in = i64::from(self.cfg.sign_in_minutes) * 60;
        self.code_flows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(state.clone(), CodeFlow { verifier, nonce, redirect_uri: redirect_uri.into(), expires: now + Duration::seconds(expires_in), app });
        Ok(Begun { state, authorize_url: u.into(), expires_in })
    }

    /// Finish a browser sign-in with what came back to the loopback: the
    /// code, the state it answers, and the issuer if the provider names it
    /// (RFC 9207). Each state finishes once.
    pub async fn finish(&self, state: &str, code: &str, iss: Option<&str>) -> Result<Polled> {
        let now = Utc::now();
        let Some(f) = self.code_flows.lock().unwrap_or_else(|e| e.into_inner()).remove(state) else { return Ok(Polled::Failed("This sign-in is over. Start again.".into())) };
        if f.expires <= now {
            return Ok(Polled::Failed("The sign-in took too long. Start again.".into()));
        }
        let d = self.discovery().await?;
        if iss.is_some_and(|i| i != d.issuer) {
            tracing::warn!("oidc: a sign-in came back from {}, not {}", iss.unwrap_or_default(), d.issuer);
            return Ok(Polled::Failed(format!("The sign-in came back from somewhere other than {}. Start again.", self.label())));
        }
        let r = self
            .http
            .post(&d.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", f.redirect_uri.as_str()),
                ("client_id", self.cfg.client_id.as_str()),
                ("code_verifier", f.verifier.as_str()),
            ])
            .send()
            .await?;
        let status = r.status();
        let b: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            let why = b["error_description"].as_str().or(b["error"].as_str()).unwrap_or(status.as_str()).to_string();
            return Ok(Polled::Failed(format!("{} refused the sign-in: {why}", self.label())));
        }
        match self.signed_in(d, &b, Some(&f.nonce), now) {
            Ok((person, rights)) => Ok(Polled::Done { person, app: f.app, rights }),
            Err(e) => {
                tracing::warn!("oidc: refused an ID token: {e:#}");
                Ok(Polled::Failed(format!("{} answered, but not with a sign-in for this appliance.", self.label())))
            }
        }
    }

    /// Check a username and password with the provider's password grant,
    /// with the code from the person's authenticator app if they gave one.
    /// Whether the provider still has this person, turned on: false when it
    /// has them turned off or no longer knows them. Asked of the account
    /// they are bound to (`sub`) when they are: another account later given
    /// their username is not them.
    pub async fn active(&self, username: &str, sub: Option<&str>) -> Result<bool> {
        let d = self.discovery().await?;
        let account = match sub {
            Some(id) => self.account_by_id(d, id).await?,
            None => self.account(d, username).await?,
        };
        Ok(account.is_some_and(|a| a.enabled))
    }

    pub async fn password(&self, username: &str, password: &str, code: Option<&str>, app: bool) -> Result<Polled> {
        let now = Utc::now();
        let d = self.discovery().await?;
        let mut form =
            vec![("grant_type", "password"), ("client_id", self.cfg.client_id.as_str()), ("scope", self.cfg.scope.as_str()), ("username", username), ("password", password)];
        if let Some(c) = code {
            form.push(("otp", c));
        }
        let r = self.http.post(&d.token_endpoint).form(&form).send().await?;
        let status = r.status();
        let b: Value = r.json().await.unwrap_or_default();
        if status.is_success() {
            // The password grant checks the password and an authenticator
            // code, nothing more: a person with a security key, or a step
            // still to do, signs in on the provider's page. Not knowing, the
            // page too: the provider ignores a code from a person without an
            // authenticator app, so a code proves nothing until it knows.
            let to_page = || Polled::UseBrowser(format!("Sign in on the {} page that just opened in your browser.", self.label()));
            match self.account(d, username).await {
                Ok(Some(a)) if a.enabled && !a.browser_only => {}
                Ok(_) => return Ok(to_page()),
                Err(e) => {
                    tracing::warn!("oidc: cannot tell whether {username} needs the provider's page (a security key, a step to do), so sending them there: {e:#}");
                    return Ok(to_page());
                }
            }
            return match self.signed_in(d, &b, None, now) {
                Ok((person, rights)) => Ok(Polled::Done { person, app, rights }),
                Err(e) => {
                    tracing::warn!("oidc: refused an ID token: {e:#}");
                    Ok(Polled::Failed(format!("{} answered, but not with a sign-in for this appliance.", self.label())))
                }
            };
        }
        let why = b["error_description"].as_str().unwrap_or_default();
        match b["error"].as_str().unwrap_or_default() {
            // Keycloak: required actions are waiting, such as a new password
            // or setting up the authenticator app, which only its own page does.
            "invalid_grant" if why == "Account is not fully set up" => {
                Ok(Polled::UseBrowser(format!("Your {} account needs finishing. Do that on the page that just opened in your browser.", self.label())))
            }
            "invalid_grant" if code.is_none() => match self.account(d, username).await {
                Ok(Some(a)) if a.browser_only => Ok(Polled::UseBrowser(format!("Sign in on the {} page that just opened in your browser.", self.label()))),
                Ok(Some(a)) if a.otp => Ok(Polled::NeedsCode),
                Ok(Some(_)) => Ok(Polled::Refused("The name or password is wrong.".into())),
                Ok(None) => Ok(Polled::UseBrowser(format!("Sign in on the {} page that just opened in your browser.", self.label()))),
                // Not knowing, the password may be right and only the code
                // missing: Keycloak's own page asks for whatever it needs.
                Err(e) => {
                    tracing::warn!("oidc: cannot tell whether {username} uses a code, so sending them to the browser: {e:#}");
                    Ok(Polled::UseBrowser(format!("Sign in on the {} page that just opened in your browser.", self.label())))
                }
            },
            "invalid_grant" => Ok(Polled::Refused("The name, password or code is wrong.".into())),
            // Its page still signs the person in; the owner is told what to change.
            "unauthorized_client" => {
                tracing::warn!("oidc: {} does not let client {} sign in with a password typed in the app; turn on Direct access grants for it", self.label(), self.cfg.client_id);
                Ok(Polled::UseBrowser(format!("Sign in on the {} page that just opened in your browser.", self.label())))
            }
            other => bail!(
                "{} refused the sign-in: {}",
                self.label(),
                if why.is_empty() {
                    if other.is_empty() {
                        status.as_str()
                    } else {
                        other
                    }
                } else {
                    why
                }
            ),
        }
    }

    /// Whether the person with this username uses a code from an
    /// authenticator app, asked as the status client; None when the
    /// provider has no such username. Keycloak's admin API is at the issuer
    /// with `/admin` before `/realms/`.
    async fn account(&self, d: &Discovery, username: &str) -> Result<Option<Account>> {
        let (admin, token) = self.status_admin(d).await?;
        let mut u = url::Url::parse(&format!("{admin}/users"))?;
        u.query_pairs_mut().append_pair("username", username).append_pair("exact", "true");
        let users: Vec<Value> = self.admin_http.get(u).bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        let Some(user) = users.first() else { return Ok(None) };
        let Some(id) = user["id"].as_str() else { return Ok(None) };
        let creds: Vec<Value> = self.admin_http.get(format!("{admin}/users/{id}/credentials")).bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        Ok(Some(Account::of(user, &creds)))
    }

    /// Whether the realm has a person with this address (#211): its admin
    /// API's users, asked as the status client. Whatever stops the read
    /// counts as not there: the gate asks rather than passing on an
    /// unchecked claim.
    pub async fn in_directory(&self, address: &str) -> Result<bool> {
        let found: Result<bool> = async {
            let d = self.discovery().await?;
            let (admin, token) = self.status_admin(d).await?;
            let mut u = url::Url::parse(&format!("{admin}/users"))?;
            u.query_pairs_mut().append_pair("email", address).append_pair("exact", "true");
            let users: Vec<Value> = self.admin_http.get(u).bearer_auth(&token).send().await?.error_for_status()?.json().await?;
            Ok(users.iter().any(|u| u["email"].as_str().is_some_and(|e| e.eq_ignore_ascii_case(address))))
        }
        .await;
        match found {
            Ok(b) => Ok(b),
            Err(e) => {
                tracing::warn!("directory: cannot tell whether {address} is a realm person: {e:#}");
                Ok(false)
            }
        }
    }

    /// The account with this id (`sub`), as `account`; None when the
    /// provider no longer has it.
    async fn account_by_id(&self, d: &Discovery, id: &str) -> Result<Option<Account>> {
        let (admin, token) = self.status_admin(d).await?;
        let mut u = url::Url::parse(&format!("{admin}/users"))?;
        u.path_segments_mut().map_err(|_| anyhow!("{admin} is not an address"))?.push(id);
        let r = self.admin_http.get(u.clone()).bearer_auth(&token).send().await?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let user: Value = r.error_for_status()?.json().await?;
        u.path_segments_mut().map_err(|_| anyhow!("{admin} is not an address"))?.push("credentials");
        let creds: Vec<Value> = self.admin_http.get(u).bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        Ok(Some(Account::of(&user, &creds)))
    }

    /// The provider's admin API, and a token for it as the status client.
    async fn status_admin(&self, d: &Discovery) -> Result<(String, String)> {
        let (Some(client), Some(credential)) = (&self.cfg.status_client, &self.cfg.status_credential) else {
            bail!("no status_client is set, so people who use a code can only sign in in the browser")
        };
        let secret = self.vault.get(credential)?.ok_or_else(|| anyhow!("the vault holds no secret named {credential}"))?;
        let admin = match d.issuer.split_once("/realms/") {
            Some((base, realm)) => format!("{}/admin/realms/{realm}", self.cfg.status_admin_url.as_deref().map(|u| u.trim_end_matches('/')).unwrap_or(base)),
            None => bail!("{} is not a Keycloak realm, whose admin API says who uses a code", d.issuer),
        };
        let r = self.http.post(&d.token_endpoint).form(&[("grant_type", "client_credentials"), ("client_id", client.as_str()), ("client_secret", secret.expose())]).send().await?;
        let status = r.status();
        let b: Value = r.json().await.unwrap_or_default();
        let token = match b["access_token"].as_str() {
            Some(t) if status.is_success() => t.to_string(),
            _ => bail!("the status client could not sign in: {}", b["error_description"].as_str().or(b["error"].as_str()).unwrap_or(status.as_str())),
        };
        Ok((admin, token))
    }

    pub async fn start(&self, app: bool) -> Result<Started> {
        let now = Utc::now();
        self.room(now)?;
        let d = self.discovery().await?;
        let endpoint = d.device_authorization_endpoint.as_deref().ok_or_else(|| anyhow!("{} does not offer the device-code grant", self.label()))?;
        let verifier = crate::connectors::random();
        let challenge = crate::connectors::challenge(&verifier);
        let r = self
            .http
            .post(endpoint)
            .form(&[("client_id", self.cfg.client_id.as_str()), ("scope", self.cfg.scope.as_str()), ("code_challenge", challenge.as_str()), ("code_challenge_method", "S256")])
            .send()
            .await?;
        let status = r.status();
        let b: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{} refused to start a sign-in: {}", self.label(), b["error_description"].as_str().or(b["error"].as_str()).unwrap_or(status.as_str()));
        }
        let field = |k: &str| b[k].as_str().map(String::from).ok_or_else(|| anyhow!("{} sent no {k}", self.label()));
        let expires_in = b["expires_in"].as_i64().ok_or_else(|| anyhow!("{} sent no expires_in", self.label()))?;
        // RFC 8628 3.2: without an interval, wait 5 seconds between polls.
        let interval = b["interval"].as_i64().unwrap_or(5);
        let flow = crate::kernel::new_id("signin");
        let started = Started {
            flow: flow.clone(),
            user_code: field("user_code")?,
            verification_uri: field("verification_uri")?,
            verification_uri_complete: b["verification_uri_complete"].as_str().map(String::from),
            expires_in,
            interval,
        };
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(
            flow,
            Pending { device_code: field("device_code")?, verifier, interval: Duration::seconds(interval), next_poll: now, expires: now + Duration::seconds(expires_in), app },
        );
        Ok(started)
    }

    /// Ask the provider once whether the sign-in was approved. Asking more
    /// often than the provider allows answers Pending without asking it.
    pub async fn poll(&self, flow: &str) -> Result<Polled> {
        let now = Utc::now();
        let (device_code, verifier, app) = {
            let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let Some(f) = p.get(flow) else { return Ok(Polled::Failed("This sign-in is over. Start again.".into())) };
            if f.expires <= now {
                p.remove(flow);
                return Ok(Polled::Failed("The sign-in code expired. Start again.".into()));
            }
            if now < f.next_poll {
                return Ok(Polled::Pending);
            }
            (f.device_code.clone(), f.verifier.clone(), f.app)
        };
        let d = self.discovery().await?;
        let r = self
            .http
            .post(&d.token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code.as_str()),
                ("client_id", self.cfg.client_id.as_str()),
                ("code_verifier", verifier.as_str()),
            ])
            .send()
            .await?;
        let status = r.status();
        let b: Value = r.json().await.unwrap_or_default();
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if status.is_success() {
            p.remove(flow);
            return match self.signed_in(d, &b, None, now) {
                Ok((person, rights)) => Ok(Polled::Done { person, app, rights }),
                Err(e) => {
                    tracing::warn!("oidc: refused an ID token: {e:#}");
                    Ok(Polled::Failed(format!("{} answered, but not with a sign-in for this appliance.", self.label())))
                }
            };
        }
        let Some(f) = p.get_mut(flow) else { return Ok(Polled::Failed("This sign-in is over. Start again.".into())) };
        match b["error"].as_str().unwrap_or_default() {
            "authorization_pending" => {
                f.next_poll = now + f.interval;
                Ok(Polled::Pending)
            }
            // RFC 8628 3.5: slow down by 5 seconds from now on.
            "slow_down" => {
                f.interval = f.interval + Duration::seconds(5);
                f.next_poll = now + f.interval;
                Ok(Polled::Pending)
            }
            "access_denied" => {
                p.remove(flow);
                Ok(Polled::Failed("The sign-in was declined.".into()))
            }
            "expired_token" => {
                p.remove(flow);
                Ok(Polled::Failed("The sign-in code expired. Start again.".into()))
            }
            other => {
                p.remove(flow);
                let why = b["error_description"].as_str().unwrap_or(if other.is_empty() { status.as_str() } else { other });
                Ok(Polled::Failed(format!("{} refused the sign-in: {why}", self.label())))
            }
        }
    }
}

#[async_trait::async_trait]
impl crate::identity::Identity for Provider {
    fn label(&self) -> String {
        Provider::label(self)
    }
    async fn begin(&self, redirect_uri: &str, login_hint: Option<&str>, app: bool) -> Result<Begun> {
        Provider::begin(self, redirect_uri, login_hint, app).await
    }
    async fn finish(&self, state: &str, code: &str, iss: Option<&str>) -> Result<Polled> {
        Provider::finish(self, state, code, iss).await
    }
    async fn password(&self, username: &str, password: &str, code: Option<&str>, app: bool) -> Result<Polled> {
        Provider::password(self, username, password, code, app).await
    }
    async fn active(&self, username: &str, sub: Option<&str>) -> Result<bool> {
        Provider::active(self, username, sub).await
    }
    async fn in_directory(&self, address: &str) -> Result<bool> {
        Provider::in_directory(self, address).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_security_key_or_a_step_to_do_sends_the_person_to_the_page() {
        let user = json!({ "id": "u1", "enabled": true, "requiredActions": [] });
        assert_eq!(Account::of(&user, &[json!({ "type": "password" })]), Account { enabled: true, otp: false, browser_only: false });
        assert!(Account::of(&user, &[json!({ "type": "otp" })]).otp);
        for key in ["webauthn", "webauthn-passwordless"] {
            assert!(Account::of(&user, &[json!({ "type": "password" }), json!({ "type": key })]).browser_only, "{key}");
        }
        let todo = json!({ "id": "u1", "enabled": true, "requiredActions": ["CONFIGURE_TOTP"] });
        assert!(Account::of(&todo, &[]).browser_only);
        assert!(!Account::of(&json!({ "id": "u1", "enabled": false }), &[]).enabled);
    }

    #[test]
    fn an_id_token_must_be_ours_current_and_name_someone() {
        let now = Utc::now();
        let iss = "https://id.example/realms/m";
        let good = json!({ "iss": iss, "aud": "magnus", "azp": "magnus", "exp": now.timestamp() + 60, "preferred_username": "Samuel" });
        assert_eq!(person(&good, iss, "magnus", "preferred_username", now).unwrap(), "samuel");
        let mut c = good.clone();
        c["aud"] = json!(["account", "magnus"]);
        assert!(person(&c, iss, "magnus", "preferred_username", now).is_ok());
        for (k, v) in [("iss", json!("https://other/realms/m")), ("aud", json!("someone-else")), ("azp", json!("someone-else")), ("exp", json!(now.timestamp() - 1))] {
            let mut c = good.clone();
            c[k] = v;
            assert!(person(&c, iss, "magnus", "preferred_username", now).is_err(), "{k} was not checked");
        }
        assert!(person(&good, iss, "magnus", "email", now).is_err());
    }

    /// A token endpoint that answers with a redirect does not get the
    /// password resent elsewhere: a 307 replays the POST, body included, and
    /// can point at plain http. Keycloak never redirects, so only here.
    #[tokio::test]
    async fn a_redirecting_token_endpoint_never_gets_the_password_resent() {
        use axum::{
            routing::{get, post},
            Router,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        let stolen = Arc::new(AtomicUsize::new(0));
        let s = stolen.clone();
        let thief = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let thief_url = format!("http://{}/steal", thief.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(
                thief,
                Router::new().route(
                    "/steal",
                    post(move || async move {
                        s.fetch_add(1, Ordering::SeqCst);
                        "{}"
                    }),
                ),
            )
            .await
        });
        let idp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let iss = format!("http://{}", idp.local_addr().unwrap());
        let (i, t) = (iss.clone(), thief_url.clone());
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(move || async move { axum::Json(json!({ "issuer": i, "token_endpoint": format!("{i}/token") })) }))
            .route("/token", post(move || async move { (axum::http::StatusCode::TEMPORARY_REDIRECT, [("location", t)]) }));
        tokio::spawn(async move { axum::serve(idp, app).await });
        let cfg: Cfg = toml::from_str(&format!("issuer = \"{iss}\"\nclient_id = \"magnus\"")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = Provider::new(cfg, Arc::new(crate::vault::Vault::open(dir.path()).unwrap())).unwrap();
        let _ = p.password("alice", "hunter2", None, false).await;
        assert_eq!(stolen.load(Ordering::SeqCst), 0, "the password was resent to where the token endpoint redirected");
    }

    /// Keycloak always answers with the nonce it was sent, so only here can a
    /// token for another sign-in be shown to be refused.
    #[test]
    fn a_browser_sign_in_needs_its_own_nonce() {
        let iss = "https://id.example/realms/m";
        let cfg: Cfg = toml::from_str(&format!("issuer = \"{iss}\"\nclient_id = \"magnus\"")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = Provider::new(cfg, Arc::new(crate::vault::Vault::open(dir.path()).unwrap())).unwrap();
        let d = Discovery { issuer: iss.into(), token_endpoint: format!("{iss}/token"), authorization_endpoint: None, device_authorization_endpoint: None };
        let b64 = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        let now = Utc::now();
        let token = |nonce: &str| json!({ "id_token": format!("{}.{}.sig", b64(&json!({})), b64(&json!({ "iss": iss, "aud": "magnus", "exp": now.timestamp() + 60, "preferred_username": "carol", "nonce": nonce }))) });
        assert_eq!(p.signed_in(&d, &token("n-1"), Some("n-1"), now).unwrap().0, "carol");
        assert!(p.signed_in(&d, &token("n-2"), Some("n-1"), now).is_err(), "a token for another sign-in was taken");
        assert!(p.signed_in(&d, &json!({ "id_token": format!("{}.{}.sig", b64(&json!({})), b64(&json!({ "iss": iss, "aud": "magnus", "exp": now.timestamp() + 60, "preferred_username": "carol" }))) }), Some("n-1"), now).is_err(), "a token with no nonce was taken");
    }

    /// The directory holds a realm person's address and no other; a read
    /// that fails counts as not there, so the gate asks (#211).
    #[tokio::test]
    async fn the_directory_holds_only_realm_addresses() {
        use axum::{routing::get, Router};
        let idp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", idp.local_addr().unwrap());
        let iss = format!("{base}/realms/org");
        let (i, b) = (iss.clone(), base.clone());
        let app = Router::new()
            .route(
                "/realms/org/.well-known/openid-configuration",
                get(move || async move { axum::Json(json!({ "issuer": i, "token_endpoint": format!("{b}/realms/org/token") })) }),
            )
            .route("/realms/org/token", axum::routing::post(move || async { axum::Json(json!({ "access_token": "t" })) }))
            .route(
                "/admin/realms/org/users",
                get(|q: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                    // A loose server: answers whatever the case.
                    let hit = q.get("email").is_some_and(|e| e.eq_ignore_ascii_case("bob@corp.example")) && q.get("exact").is_some_and(|e| e == "true");
                    axum::Json(if hit { json!([{ "id": "u1", "email": "bob@corp.example" }]) } else { json!([]) })
                }),
            );
        tokio::spawn(async move { axum::serve(idp, app).await });
        let cfg: Cfg = toml::from_str(&format!("issuer = \"{iss}\"\nclient_id = \"magnus\"\nstatus_client = \"status\"\nstatus_credential = \"status-secret\"\n")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let vault = Arc::new(crate::vault::Vault::open(dir.path()).unwrap());
        vault.put("status-secret", "s3cret").unwrap();
        let p = Provider::new(cfg, vault).unwrap();
        assert!(p.in_directory("bob@corp.example").await.unwrap());
        assert!(p.in_directory("BOB@corp.example").await.unwrap(), "addresses compare case-blind");
        assert!(!p.in_directory("mallory@evil.example").await.unwrap());
        // No status client, no read: not there, never an error out.
        let cfg: Cfg = toml::from_str(&format!("issuer = \"{iss}\"\nclient_id = \"magnus\"\n")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = Provider::new(cfg, Arc::new(crate::vault::Vault::open(dir.path()).unwrap())).unwrap();
        assert!(!p.in_directory("bob@corp.example").await.unwrap());
    }

    #[test]
    fn a_jwt_payload_decodes() {
        let b64 = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        let t = format!("{}.{}.sig", b64(&json!({ "alg": "RS256" })), b64(&json!({ "sub": "x" })));
        assert_eq!(claims(&t).unwrap()["sub"], "x");
        assert!(claims("nope").is_err());
    }

    #[test]
    fn only_https_except_on_this_machine() {
        assert!(checked("https://id.example/realms/m").is_ok());
        assert!(checked("http://127.0.0.1:8080/realms/m").is_ok());
        assert!(checked("http://localhost:8080/realms/m").is_ok());
        assert!(checked("http://id.example/realms/m").is_err());
    }
}
