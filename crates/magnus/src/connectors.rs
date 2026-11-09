//! The person's apps over MCP, added and signed in to from the app (the
//! record's product scope: email, calendar, files, chat, business systems).
//!
//! Apps in the config file stay as written there; apps the owner adds live in
//! the log. Sign-in follows MCP's authorization spec (2025-06-18): the app's
//! protected-resource metadata names its authorization server; Magnus
//! registers itself there (RFC 7591) unless the owner gave a client id, and
//! signs in with the authorization code and PKCE, the code coming back to a
//! loopback address on the person's computer (RFC 8252), where the desktop
//! app catches it. Tokens are kept in the vault and refreshed when they
//! expire; the kill switch revokes them (RFC 7009).

use crate::config::Connector;
use crate::kernel::Kernel;
use crate::vault::Secret;
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const OAUTH: &str = "oauth";

/// The redirect Slack registers for a distributed app. The token exchange
/// sends this same address. The browser is sent on to `SLACK_HANDOFF`,
/// which the desktop app is listening on.
pub const SLACK_OAUTH_CALLBACK: &str = "https://magnus.pyrallon.ai/oauth/slack/callback";
pub const SLACK_HANDOFF: &str = "http://127.0.0.1:17321/callback";

/// The vault name of an app's credential: the organisation's key for an
/// app that takes one, or, before connections were each person's own, the
/// one sign-in everyone's runs used (no longer used; revoked with the app).
pub fn credential_name(app: &str) -> String {
    format!("connector_{app}")
}

/// The vault name of one person's own sign-in to an app (connectors spec:
/// the organisation provides the connector, each person signs in with
/// their own account, and no one else acts through it). App names never
/// hold `__`, so the person is what follows the first one.
pub fn connection_name(app: &str, person: &str) -> String {
    format!("connector_{app}__{person}")
}

/// Whether a vault name is one of an app's sign-ins: a person's own, or
/// the old shared one.
pub fn is_sign_in_of(name: &str, app: &str) -> bool {
    name == credential_name(app) || name.starts_with(&format!("{}__", credential_name(app)))
}

/// A sign-in waiting for its code.
struct Pending {
    app: String,
    /// Who is signing in: the sign-in becomes theirs alone.
    person: String,
    verifier: String,
    redirect_uri: String,
    client: Client,
    /// The authorization server this sign-in went to, and whether it says
    /// so in its answer (RFC 9207): an answer from another one is refused.
    issuer: String,
    iss_required: bool,
    /// Whether this sign-in sent a PKCE challenge. The verifier is sent
    /// back only then.
    pkce: bool,
    /// The vault name of the registered client this sign-in used again,
    /// forgotten if the token endpoint no longer knows it.
    kept: Option<String>,
    at: Instant,
}

/// Where tokens come from, and who Magnus is there. Kept with the tokens.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Client {
    token_endpoint: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    resource: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Tokens {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    expires_at: Option<i64>,
    /// Seconds an access token lasts, as last said: kept for a refresh
    /// answer that does not say (expires_in is only recommended).
    #[serde(default)]
    lifetime: Option<i64>,
    client: Client,
    /// When the person signed in (Unix seconds).
    #[serde(default)]
    connected_at: Option<i64>,
    /// The account they signed in as, where the app's answer names it.
    #[serde(default)]
    account: Option<String>,
    /// The app refused to refresh it: the person signed Magnus out there, or
    /// the sign-in expired. It stays until they sign in again or disconnect,
    /// and is not tried again meanwhile (connectors spec, Lifecycle).
    #[serde(default)]
    lapsed: bool,
    /// Access the app asked for that this sign-in does not hold (its
    /// step-up challenge): the next sign-in asks for it too.
    #[serde(default)]
    wants: Vec<String>,
}

/// The account a sign-in's answer names, from its ID token's claims, for
/// the person to see which account they connected (not for any decision:
/// the token came straight from the app's token endpoint).
fn account_of(answer: &Value) -> Option<String> {
    use base64::Engine;
    if let Some(claims) = answer["id_token"].as_str().and_then(|t| t.split('.').nth(1)) {
        if let Ok(raw) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(claims.trim_end_matches('=')) {
            if let Ok(c) = serde_json::from_slice::<Value>(&raw) {
                if let Some(named) = ["email", "preferred_username", "name"].iter().find_map(|k| c[*k].as_str().map(String::from)) {
                    return Some(named);
                }
            }
        }
    }
    // Slack names the person and the workspace on the token answer, and
    // sends no ID token unless identity scopes were asked for.
    let user = answer["authed_user"]["id"].as_str();
    let team = answer["team"]["name"].as_str().or(answer["team"]["id"].as_str());
    match (user, team) {
        (Some(u), Some(t)) => Some(format!("{u} in {t}")),
        (Some(u), None) => Some(u.to_string()),
        (None, Some(t)) => Some(t.to_string()),
        _ => None,
    }
}

/// What a token answer granted. A Slack install that also has a bot token
/// puts that at the top and the person's under `authed_user`; the person
/// is who Magnus acts as, so the user token is the one kept. An answer
/// with `ok: false` (Slack's refusals are HTTP 200) is an error.
struct Grant {
    access: String,
    refresh: Option<String>,
    expires: Option<i64>,
    account: Option<String>,
}

fn granted(v: &Value) -> Result<Grant, String> {
    if v["ok"] == false {
        return Err(format!("the sign-in was refused ({})", v["error"].as_str().unwrap_or("no reason given")));
    }
    let user = &v["authed_user"];
    let as_user = v["token_type"].as_str() == Some("bot") && user["access_token"].as_str().is_some();
    let access = if as_user { user["access_token"].as_str() } else { v["access_token"].as_str() };
    let Some(access) = access else { return Err("the sign-in returned no access token".into()) };
    let refresh = if as_user { user["refresh_token"].as_str().or(v["refresh_token"].as_str()) } else { v["refresh_token"].as_str() };
    let expires = if as_user { user["expires_in"].as_i64().or(v["expires_in"].as_i64()) } else { v["expires_in"].as_i64() };
    Ok(Grant { access: access.to_string(), refresh: refresh.map(str::to_string), expires, account: account_of(v) })
}

/// Slack says a refresh token is dead with HTTP 200 and `ok: false`. Those
/// end the sign-in; a limit or a blip does not.
fn dead_token(v: &Value) -> bool {
    v["ok"] == false && matches!(v["error"].as_str(), Some("invalid_refresh_token" | "invalid_grant" | "token_revoked" | "invalid_auth" | "token_expired" | "account_inactive"))
}

/// Where one person stands with one of the organisation's apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standing {
    /// Signed in and working.
    Connected,
    /// Signed in once, and the sign-in no longer works (refused at refresh,
    /// or revoked at the app): one button signs in again.
    NeedsAttention,
    /// Not signed in.
    Available,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Connected => "connected",
            Standing::NeedsAttention => "needs_attention",
            Standing::Available => "available",
        }
    }
}

/// An app's name as people read it: "google-calendar" is "Google calendar".
pub fn title(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// What an app lets Magnus do, in plain words, from the classes of its
/// tools: one sentence for its card, and one line per kind of action.
pub fn allows(c: &Connector) -> (String, Vec<String>) {
    const KINDS: [(&str, &str, &str); 4] = [
        ("read", "read", "Reads"),
        ("reversible_write", "make changes that can be undone", "Changes that can be undone"),
        ("irreversible", "make changes that cannot be undone, when allowed", "Changes that cannot be undone"),
        ("financial", "move money, when allowed", "Moves money"),
    ];
    let mut can = vec![];
    let mut lines = vec![];
    for (risk, verb, head) in KINDS {
        let tools: Vec<String> = c.tools.iter().filter(|(_, t)| t.risk == risk).map(|(n, _)| n.replace(['_', '-'], " ")).collect();
        if !tools.is_empty() {
            can.push(verb);
            lines.push(format!("{head}: {}", tools.join(", ")));
        }
    }
    let name = title(&c.name);
    let sentence = match can.as_slice() {
        [] => format!("Magnus can work in {name} once your organisation sets it up."),
        [one] => format!("Magnus can {one} in {name}."),
        [rest @ .., last] => format!("Magnus can {} and {last} in {name}.", rest.join(", ")),
    };
    (sentence, lines)
}

/// A sign-in left unfinished this long is dropped.
const PENDING_FOR: Duration = Duration::from_secs(15 * 60);

fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static P: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

/// One refresh at a time: apps that rotate refresh tokens accept each once.
fn refreshing() -> &'static tokio::sync::Mutex<()> {
    static R: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(30)).build().expect("http client")
}

/// How long a refused registration waits before trying again: what the
/// server's `Retry-After` asks, capped (the seconds form; a date falls
/// back to the backoff), else 1 s, 2 s, 4 s. The sign-in is interactive,
/// so the waits stay short.
fn registration_backoff(headers: &reqwest::header::HeaderMap, attempt: u32) -> Duration {
    if let Some(s) = headers.get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()) {
        return Duration::from_secs(s).min(Duration::from_secs(10));
    }
    Duration::from_secs(1 << attempt.min(2))
}

/// A provider may not know a client it registered a moment ago: GitLab.com
/// answered the sign-in address built from one with 401 invalid_client, "The
/// requested OAuth 2.0 Client does not exist" (master run 42105845733,
/// #170), and the person would land on that error. After registering, the
/// address is handed out once the provider's sign-in answers it with
/// anything else, asked every quarter second for ten seconds at most in all,
/// however slowly it answers (how long the person may wait for the sign-in
/// page); then it is handed out as it is. Redirects are not followed: the
/// answer is the provider's own.
async fn until_client_known(authorize: &str) {
    let known = async {
        while unknown(authorize).await {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), known).await;
}

fn no_redirects() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build().expect("http client")
}

/// The provider's sign-in says it does not know the client (401
/// invalid_client, as GitLab.com answers).
async fn unknown(authorize: &str) -> bool {
    match no_redirects().get(authorize).send().await {
        Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => r.text().await.unwrap_or_default().contains("invalid_client"),
        _ => false,
    }
}

/// The provider's sign-in says it does not know the client: a 400 or 401
/// whose answer names the client, as GitLab.com answers 401 invalid_client
/// before anyone signs in and Cloudflare's provider library answers
/// invalid_request "Invalid client_id". A 429, a 403 or a server error says
/// nothing about the client, and the kept one stays (review of #215).
/// Redirects are not followed.
async fn refused(authorize: &str) -> bool {
    match no_redirects().get(authorize).send().await {
        Ok(r) if matches!(r.status().as_u16(), 400 | 401) => r.text().await.unwrap_or_default().to_lowercase().contains("client"),
        _ => false,
    }
}

/// Where the client Magnus registered at `registration` for a redirect is
/// kept. Any port on a loopback IP address is the same redirect (RFC 8252
/// §7.3; GitLab.com's Doorkeeper and Cloudflare's provider library match it
/// so), so one client serves every sign-in from the person's computer.
/// `localhost` is a name, matched exactly.
fn registered_client_name(registration: &str, redirect_uri: &str) -> String {
    let redirect = match url::Url::parse(redirect_uri) {
        Ok(mut u) if u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "[::1]")) => {
            let _ = u.set_port(None);
            u.to_string()
        }
        _ => redirect_uri.to_string(),
    };
    format!("registered_client {registration} {redirect}")
}

pub(crate) fn random() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

pub(crate) fn challenge(verifier: &str) -> String {
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
}

/// RFC 8252 loopback redirects only: the code goes to the person's own
/// computer, never to another host.
pub(crate) fn loopback(uri: &str) -> Result<()> {
    let u = url::Url::parse(uri).context("the redirect address is not a URL")?;
    let host = u.host_str().unwrap_or_default();
    if u.scheme() != "http" || !matches!(host, "127.0.0.1" | "[::1]" | "localhost") || u.port().is_none() {
        bail!("the redirect address must be http://127.0.0.1:<port>/... on the person's computer");
    }
    Ok(())
}

/// The well-known address for `suffix` of `base`, path-aware (RFC 8414 §3,
/// RFC 9728 §3): `{origin}/.well-known/{suffix}{path}`, then without the path.
fn well_known(base: &str, suffix: &str) -> Result<Vec<String>> {
    let u = url::Url::parse(base)?;
    let origin = u.origin().ascii_serialization();
    let path = u.path().trim_end_matches('/');
    let mut out = vec![];
    if !path.is_empty() {
        out.push(format!("{origin}/.well-known/{suffix}{path}"));
    }
    out.push(format!("{origin}/.well-known/{suffix}"));
    Ok(out)
}

async fn first_json(c: &reqwest::Client, urls: &[String]) -> Option<Value> {
    for u in urls {
        if let Ok(r) = c.get(u).header("accept", "application/json").send().await {
            if r.status().is_success() {
                if let Ok(v) = r.json::<Value>().await {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// What an app calls itself in its protected-resource metadata
/// (`resource_name`, RFC 9728), from the address its sign-in answer names
/// or the standard one. None when it does not say. (The name a server gives
/// at initialize is often its software's, "StatelessServer", so it is not
/// used.)
async fn own_name(url: &str) -> Option<String> {
    let r = http()
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "magnus", "version": env!("CARGO_PKG_VERSION") } } }))
        .send()
        .await
        .ok()?;
    let named = r.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).and_then(|h| {
        let i = h.find("resource_metadata=\"")? + "resource_metadata=\"".len();
        Some(h[i..].split('"').next()?.to_string())
    });
    // RFC 9728 §3.1: the metadata's standard address is the resource's
    // path under /.well-known/oauth-protected-resource.
    let standard = url::Url::parse(url).ok().map(|u| format!("{}/.well-known/oauth-protected-resource{}", u.origin().ascii_serialization(), u.path().trim_end_matches('/')));
    let urls: Vec<String> = named.into_iter().chain(standard).collect();
    first_json(&http(), &urls).await?["resource_name"].as_str().map(String::from)
}

/// Whether an app asks for a sign-in: an unauthenticated initialize answered
/// with 401. Its protected-resource metadata, when it says where.
pub async fn asks_sign_in(url: &str) -> Result<Option<Option<String>>> {
    let r = http()
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "magnus", "version": env!("CARGO_PKG_VERSION") } } }))
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;
    if r.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    let meta = r.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).and_then(|h| {
        let i = h.find("resource_metadata=\"")? + "resource_metadata=\"".len();
        Some(h[i..].split('"').next()?.to_string())
    });
    Ok(Some(meta))
}

fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The app's authorization server metadata: from its protected-resource
/// metadata (RFC 9728), then the server's own (RFC 8414, else OpenID). The
/// metadata must be for this app and that server (RFC 9728 §3.3, RFC 8414
/// §3.3): metadata for another resource or another issuer is refused, so
/// a sign-in never goes where the app's metadata was not about.
async fn discover(app_url: &str, resource_metadata: Option<String>) -> Result<(Value, Vec<String>, String)> {
    let c = http();
    let prm_urls = match resource_metadata {
        Some(u) => vec![u],
        None => well_known(app_url, "oauth-protected-resource")?,
    };
    let prm = first_json(&c, &prm_urls).await.ok_or_else(|| anyhow!("the app does not say where to sign in (no protected-resource metadata)"))?;
    if let Some(r) = prm["resource"].as_str() {
        if !same_url(r, app_url) {
            bail!("the app's sign-in metadata is for {r}, not {app_url}");
        }
    }
    let issuer = prm["authorization_servers"][0].as_str().ok_or_else(|| anyhow!("the app names no authorization server"))?.to_string();
    let scopes: Vec<String> = prm["scopes_supported"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
    Ok((server_metadata(&issuer).await?, scopes, issuer))
}

/// An authorization server's metadata (RFC 8414, else OpenID), which must
/// name that server as its issuer.
async fn server_metadata(issuer: &str) -> Result<Value> {
    let c = http();
    let mut urls = well_known(&issuer, "oauth-authorization-server")?;
    urls.extend(well_known(&issuer, "openid-configuration")?);
    urls.push(format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/')));
    let asm = first_json(&c, &urls).await.ok_or_else(|| anyhow!("{issuer} publishes no authorization server metadata"))?;
    match asm["issuer"].as_str() {
        Some(i) if same_url(i, issuer) => {}
        other => bail!("the authorization server metadata found for {issuer} names issuer {}", other.unwrap_or("none")),
    }
    Ok(asm)
}

impl Kernel {
    /// The organisation's apps that are on: the config file's, those the
    /// owner added, and the whole catalog, already set up (connectors spec
    /// rev 46), each with who may use it as an admin set it. An app an
    /// admin turned off is offered to no one and reached by no run.
    pub fn connectors(&self) -> Vec<Connector> {
        self.all_connectors().into_iter().filter(|(_, on)| *on).map(|(c, _)| c).collect()
    }

    /// Every app, on or off (admin console spec, Security › Connectors and
    /// credentials).
    pub fn all_connectors(&self) -> Vec<(Connector, bool)> {
        let mut all = self.cfg.connectors.clone();
        let (added, settings) = self.read(|s| (s.connectors.values().cloned().collect::<Vec<Connector>>(), s.connector_settings.clone()));
        all.extend(added.into_iter().filter(|c| self.cfg.connectors.iter().all(|f| f.name != c.name)));
        for m in crate::catalog::manifests() {
            if all.iter().all(|c| c.name != m.id) {
                all.push(crate::catalog::connector(m));
            }
        }
        all.into_iter()
            .map(|mut c| match settings.get(&c.name) {
                Some(set) => {
                    if let Some(g) = &set.access_groups {
                        c.access_groups = g.clone();
                    }
                    if let Some(g) = &set.blocked_groups {
                        c.blocked_groups = g.clone();
                    }
                    (c, set.enabled)
                }
                None => (c, true),
            })
            .collect()
    }

    /// Whether a catalog connector's provider client is complete on this
    /// appliance: its secret, when it has one, provisioned by Alpha Vector.
    /// Only a deployment that has not yet had it is incomplete.
    pub fn provider_ready(&self, c: &Connector) -> bool {
        match c.registration.as_deref() {
            None => true,
            Some(p) => crate::catalog::client(p).is_some_and(|cl| !cl.needs_secret() || self.read(|s| s.registrations.contains(p))),
        }
    }

    pub fn connector(&self, name: &str) -> Option<Connector> {
        self.connectors().into_iter().find(|c| c.name == name)
    }

    /// The bearer token for an app, for `person`: the organisation's key
    /// for an app that takes one, or for an app people sign in to, the
    /// person's own access token, refreshed when it has expired. None when
    /// there is none (they have not signed in, or the refresh failed: they
    /// sign in again).
    pub async fn connector_secret(&self, c: &Connector, person: &str) -> Result<Option<Secret>> {
        let Some(org) = &c.credential else { return Ok(None) };
        if c.auth.as_deref() != Some(OAUTH) {
            return self.vault.get(org);
        }
        let name = &connection_name(&c.name, person);
        let _g = refreshing().lock().await;
        let Some(raw) = self.vault.get(name)? else { return Ok(None) };
        let mut t: Tokens = serde_json::from_str(raw.expose()).context("stored sign-in")?;
        if t.lapsed {
            return Ok(None);
        }
        // A minute early, so a token does not expire on its way.
        let now = chrono::Utc::now().timestamp();
        if t.expires_at.is_some_and(|at| at - 60 <= now) {
            let Some(refresh) = t.refresh_token.clone() else { return Ok(None) };
            let mut form =
                vec![("grant_type", "refresh_token".to_string()), ("refresh_token", refresh), ("client_id", t.client.client_id.clone()), ("resource", t.client.resource.clone())];
            form.retain(|(k, v)| *k != "resource" || !v.is_empty());
            if let Some(s) = &t.client.client_secret {
                form.push(("client_secret", s.clone()));
            }
            let r = http().post(&t.client.token_endpoint).form(&form).send().await;
            let v: Value = match r {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
                Ok(r) => {
                    tracing::warn!("refresh for {}: HTTP {}", c.name, r.status());
                    // Refused (not a failure to reach it): the sign-in no
                    // longer works until the person signs in again.
                    if r.status().is_client_error() {
                        t.lapsed = true;
                        self.vault.put(name, &serde_json::to_string(&t)?)?;
                    }
                    return Ok(None);
                }
                Err(e) => {
                    tracing::warn!("refresh for {}: {e}", c.name);
                    return Ok(None);
                }
            };
            if dead_token(&v) {
                t.lapsed = true;
                self.vault.put(name, &serde_json::to_string(&t)?)?;
                return Ok(None);
            }
            let Ok(g) = granted(&v) else { return Ok(None) };
            t.access_token = g.access;
            if let Some(r) = g.refresh {
                t.refresh_token = Some(r);
            }
            if g.account.is_some() {
                t.account = g.account;
            }
            // A token that had a lifetime keeps one: without it, it would
            // never be refreshed again and sign the person out at expiry.
            t.lifetime = g.expires.or(t.lifetime).or(Some(3600));
            t.expires_at = t.lifetime.map(|s| now + s);
            self.vault.put(name, &serde_json::to_string(&t)?)?;
        }
        Ok(Some(Secret::new(t.access_token)))
    }

    /// Register Magnus at an authorization server for a redirect address
    /// (RFC 7591), and keep the client under `name` for later sign-ins. A
    /// throttled or failing server (429, 5xx) is tried again, briefly: a
    /// provider under load refused a sign-in that a wait would save (#315).
    async fn register_client(&self, name: &str, registration: &str, redirect_uri: &str) -> Result<(String, Option<String>)> {
        let body = json!({
            "client_name": "Magnus", "redirect_uris": [redirect_uri], "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"], "token_endpoint_auth_method": "none"
        });
        let mut last = String::new();
        for attempt in 0..3 {
            let r = http().post(registration).json(&body).send().await?;
            if r.status().is_success() {
                let v: Value = r.json().await?;
                let id = v["client_id"].as_str().ok_or_else(|| anyhow!("registration returned no client id"))?.to_string();
                let secret = v["client_secret"].as_str().map(String::from);
                self.vault.put(name, &json!({ "client_id": id, "client_secret": secret }).to_string())?;
                return Ok((id, secret));
            }
            last = format!("HTTP {}", r.status());
            let retryable = r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS || r.status().is_server_error();
            if !retryable || attempt == 2 {
                break;
            }
            tracing::warn!("registration for {name} refused with {last}; trying again");
            tokio::time::sleep(registration_backoff(r.headers(), attempt)).await;
        }
        bail!("the app's authorization server refused to register Magnus: {last}");
    }

    /// Start signing in to an app: register if needed, and return the
    /// address to open in the person's browser.
    pub async fn connector_sign_in(&self, c: &Connector, person: &str, redirect_uri: &str) -> Result<(String, String)> {
        if let Some(fixed) = &c.redirect_uri {
            if redirect_uri != fixed {
                bail!("sign in to {} from the Magnus app on this computer", c.title.as_deref().unwrap_or(&c.name));
            }
        }
        // A loopback address, or Slack's one registered https address. The
        // token exchange repeats whichever was sent here.
        if loopback(redirect_uri).is_err() && redirect_uri != SLACK_OAUTH_CALLBACK {
            loopback(redirect_uri)?;
        }
        // A brokered connector's sign-in is the provider's own, for its
        // API: at the authorization server its manifest names, with no
        // resource indicator (RFC 8707 names an MCP server a token is for;
        // the provider's API takes the plain OAuth 2.0 request).
        let brokered = c.transport == crate::brokered::BROKERED;
        let (asm, scopes, issuer) = match &c.issuer {
            Some(issuer) if brokered => (server_metadata(issuer).await?, vec![], issuer.clone()),
            None if brokered => bail!("{} names no authorization server to sign in at", c.name),
            _ => {
                let meta = asks_sign_in(&c.url).await?.unwrap_or(None);
                discover(&c.url, meta).await?
            }
        };
        let resource = if brokered { String::new() } else { c.url.clone() };
        let s = |k: &str| asm[k].as_str().map(String::from);
        let (Some(authorize), Some(token)) = (s("authorization_endpoint"), s("token_endpoint")) else {
            bail!("the app's authorization server names no authorization or token endpoint")
        };
        if c.pkce && asm["code_challenge_methods_supported"].as_array().is_some_and(|m| !m.iter().any(|x| x == "S256")) {
            bail!("the app's authorization server does not support PKCE (S256), which MCP requires");
        }
        // A catalog connector signs in through Alpha Vector's own client
        // with its provider, from the catalog; its secret, when it has one,
        // is the one Alpha Vector provisioned to this appliance.
        let shown = c.title.clone().unwrap_or_else(|| c.name.clone());
        let registered: Option<(String, Option<String>)> = match &c.registration {
            Some(provider) => {
                let client = crate::catalog::client(provider).ok_or_else(|| anyhow!("{} cannot be connected on this appliance yet", shown))?;
                let secret = match client.needs_secret() {
                    true => {
                        let raw = self
                            .vault
                            .get(&crate::catalog::registration_name(provider))?
                            .ok_or_else(|| anyhow!("{} cannot be connected on this appliance yet: Alpha Vector has not finished setting it up here", shown))?;
                        let v: Value = serde_json::from_str(raw.expose()).context("stored registration")?;
                        Some(v["client_secret"].as_str().ok_or_else(|| anyhow!("{} cannot be connected on this appliance yet", shown))?.to_string())
                    }
                    false => None,
                };
                Some((client.client_id.clone(), secret))
            }
            None => None,
        };
        // A client Magnus registered itself is kept and used again for
        // every later sign-in at that server, by anyone on the appliance:
        // gitlab.com lets one address register about ten an hour (#208).
        let mut kept: Option<(String, String)> = None;
        let (mut client_id, mut client_secret, fresh) = match (registered, &c.client_id, s("registration_endpoint")) {
            (Some((id, secret)), _, _) => (id, secret, false),
            (None, Some(id), _) => (id.clone(), None, false),
            (None, None, Some(reg)) => {
                let name = registered_client_name(&reg, redirect_uri);
                let stored = self.vault.get(&name)?.and_then(|raw| serde_json::from_str::<Value>(raw.expose()).ok());
                let had = stored.as_ref().and_then(|v| v["client_id"].as_str().map(|id| (id.to_string(), v["client_secret"].as_str().map(String::from))));
                kept = Some((name.clone(), reg.clone()));
                match had {
                    Some((id, secret)) => (id, secret, false),
                    None => {
                        let (id, secret) = self.register_client(&name, &reg, redirect_uri).await?;
                        (id, secret, true)
                    }
                }
            }
            (None, None, None) => bail!("the app's authorization server does not let Magnus register itself, and Magnus never asks for client details"),
        };
        let verifier = random();
        let state = random();
        let wanted = self.wanted_scopes(c, person);
        let address = |client_id: &str| -> Result<url::Url> {
            let mut u = url::Url::parse(&authorize)?;
            let mut q = u.query_pairs_mut();
            q.append_pair("response_type", "code").append_pair("client_id", client_id).append_pair("redirect_uri", redirect_uri).append_pair("state", &state);
            if c.pkce {
                q.append_pair("code_challenge", &challenge(&verifier)).append_pair("code_challenge_method", "S256");
            }
            if !resource.is_empty() {
                q.append_pair("resource", &resource);
            }
            // Exactly what the app's manifest asks for, where it names it.
            let mut scopes = if c.scopes.is_empty() { scopes.clone() } else { c.scopes.clone() };
            // More access the app asked of this person's sign-in.
            for w in wanted.iter().cloned() {
                if !scopes.contains(&w) {
                    scopes.push(w);
                }
            }
            if !scopes.is_empty() {
                q.append_pair("scope", &scopes.join(" "));
            }
            // A refresh token that lasts (Google gives one only when asked
            // for offline access).
            if authorize.starts_with("https://accounts.google.com/") {
                q.append_pair("access_type", "offline").append_pair("prompt", "consent");
            }
            drop(q);
            Ok(u)
        };
        let mut u = address(&client_id)?;
        if fresh {
            until_client_known(u.as_str()).await;
        } else if let Some((name, reg)) = &kept {
            // The provider may have dropped a client kept from before: its
            // sign-in refuses the address. Magnus registers again, once.
            if refused(u.as_str()).await {
                (client_id, client_secret) = self.register_client(name, reg, redirect_uri).await?;
                u = address(&client_id)?;
                until_client_known(u.as_str()).await;
            }
        }
        let client = Client { token_endpoint: token, revocation_endpoint: s("revocation_endpoint"), client_id, client_secret, resource };
        let mut p = pending().lock().unwrap();
        p.retain(|_, x| x.at.elapsed() < PENDING_FOR);
        let iss_required = asm["authorization_response_iss_parameter_supported"] == true;
        p.insert(
            state.clone(),
            Pending {
                app: c.name.clone(),
                person: person.into(),
                verifier,
                redirect_uri: redirect_uri.into(),
                client,
                issuer,
                iss_required,
                pkce: c.pkce,
                kept: kept.map(|(name, _)| name),
                at: Instant::now(),
            },
        );
        Ok((u.to_string(), state))
    }

    /// Finish a sign-in with the code the browser brought back.
    /// Only the person who started it can finish it.
    pub async fn connector_sign_in_complete(&self, app: &str, person: &str, state: &str, code: &str, iss: Option<&str>) -> Result<()> {
        // Taken only by the person who started it: anyone else's attempt
        // leaves it waiting for them.
        let p = {
            let mut m = pending().lock().unwrap();
            match m.get(state) {
                Some(p) if p.app == app && p.person == person => m.remove(state),
                _ => None,
            }
        };
        let p = p.filter(|p| p.at.elapsed() < PENDING_FOR).ok_or_else(|| anyhow!("this sign-in expired or is unknown; start again"))?;
        // The answer must come from the server the sign-in went to (RFC
        // 9207): otherwise the code and verifier would go to this one's token
        // endpoint for another server's code (a mix-up).
        match iss {
            Some(i) if !same_url(i, &p.issuer) => bail!("the sign-in was answered by {i}, not {}; start again", p.issuer),
            None if p.iss_required => bail!("{} says it names itself in its answer, and this answer names no one; start again", p.issuer),
            _ => {}
        }
        let c = &p.client;
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", p.redirect_uri.clone()),
            ("client_id", c.client_id.clone()),
            ("code_verifier", p.verifier.clone()),
            ("resource", c.resource.clone()),
        ];
        form.retain(|(k, v)| *k != "resource" || !v.is_empty());
        if !p.pkce {
            form.retain(|(k, _)| *k != "code_verifier");
        }
        if let Some(s) = &c.client_secret {
            form.push(("client_secret", s.clone()));
        }
        let r = http().post(&c.token_endpoint).form(&form).send().await?;
        if !r.status().is_success() {
            let status = r.status();
            // A kept client the provider dropped: the next sign-in registers.
            if let Some(name) = &p.kept {
                if r.text().await.unwrap_or_default().contains("invalid_client") {
                    let _ = self.vault.remove(name);
                }
            }
            bail!("the app's authorization server refused the sign-in: HTTP {status}");
        }
        let v: Value = r.json().await?;
        let g = granted(&v).map_err(|e| anyhow!(e))?;
        let t = Tokens {
            access_token: g.access,
            refresh_token: g.refresh,
            expires_at: g.expires.map(|s| chrono::Utc::now().timestamp() + s),
            lifetime: g.expires,
            client: p.client,
            connected_at: Some(chrono::Utc::now().timestamp()),
            account: g.account,
            lapsed: false,
            wants: vec![],
        };
        let account = t.account.clone();
        self.vault.put(&connection_name(app, person), &serde_json::to_string(&t)?)?;
        self.live_tools.lock().await.remove(&connection_name(app, person));
        self.emit(&format!("person:{person}"), Ev::AppConnected { app: app.to_string(), person: person.to_string(), account }).await?;
        Ok(())
    }

    /// Where `person` stands with an app, and when they signed in and as
    /// whom, where known.
    pub async fn standing(&self, c: &Connector, person: &str) -> (Standing, Option<i64>, Option<String>) {
        if c.credential.is_none() || c.auth.as_deref() != Some(OAUTH) {
            return (if self.connected(c, person) { Standing::Connected } else { Standing::Available }, None, None);
        }
        let Ok(Some(raw)) = self.vault.get(&connection_name(&c.name, person)) else { return (Standing::Available, None, None) };
        let t: Option<Tokens> = serde_json::from_str(raw.expose()).ok();
        let (at, account) = t.map(|t| (t.connected_at, t.account)).unwrap_or_default();
        let works = self.connector_secret(c, person).await.ok().flatten().is_some();
        (if works { Standing::Connected } else { Standing::NeedsAttention }, at, account)
    }

    /// What Magnus did through an app for a person, newest first: each
    /// call's tool in plain words, its task, when, and whether it worked.
    pub fn recent_uses(&self, app: &str, person: &str, n: usize) -> Vec<Value> {
        let prefix = format!("{app}__");
        let mut uses: Vec<(chrono::DateTime<chrono::Utc>, Value)> = self.read(|s| {
            s.runs
                .values()
                .filter(|r| r.person == person)
                .flat_map(|r| r.intents.values().filter(|i| i.tool.starts_with(&prefix)).map(move |i| (r, i)))
                .map(|(r, i)| {
                    let done = i.result.as_ref().map(|x| !x.is_error);
                    (i.at, json!({ "action": i.tool[prefix.len()..].replace(['_', '-'], " "), "task": r.task, "at": i.at, "worked": done }))
                })
                .collect()
        });
        uses.sort_by(|a, b| b.0.cmp(&a.0));
        uses.into_iter().take(n).map(|(_, v)| v).collect()
    }

    /// The app asked for access the person's sign-in does not hold: kept on
    /// it, so their next sign-in asks for it (MCP authorization, step-up).
    pub fn want_scopes(&self, app: &str, person: &str, scopes: &[String]) -> Result<()> {
        let name = connection_name(app, person);
        let Some(raw) = self.vault.get(&name)? else { return Ok(()) };
        let mut t: Tokens = serde_json::from_str(raw.expose()).context("stored sign-in")?;
        for s in scopes {
            if !t.wants.contains(s) {
                t.wants.push(s.clone());
            }
        }
        self.vault.put(&name, &serde_json::to_string(&t)?)
    }

    /// The access the app asked of the person's sign-in that it does not hold.
    pub fn wanted_scopes(&self, c: &Connector, person: &str) -> Vec<String> {
        self.vault.get(&connection_name(&c.name, person)).ok().flatten().and_then(|raw| serde_json::from_str::<Tokens>(raw.expose()).ok()).map(|t| t.wants).unwrap_or_default()
    }

    /// Whether the person's sign-in to an app was refused by the app: they
    /// need to sign in again.
    pub fn lapsed(&self, c: &Connector, person: &str) -> bool {
        c.auth.as_deref() == Some(OAUTH)
            && self.vault.get(&connection_name(&c.name, person)).ok().flatten().and_then(|raw| serde_json::from_str::<Tokens>(raw.expose()).ok()).is_some_and(|t| t.lapsed)
    }

    /// Whether `person` can use an app now: it takes no sign-in, or they
    /// have signed in to it themselves.
    pub fn connected(&self, c: &Connector, person: &str) -> bool {
        match (&c.credential, c.auth.as_deref()) {
            (None, _) => true,
            (Some(_), Some(OAUTH)) => self.vault.names().is_ok_and(|n| n.contains(&connection_name(&c.name, person))),
            (Some(org), _) => self.vault.names().is_ok_and(|n| n.contains(org)),
        }
    }

    /// End one person's sign-in to an app (connectors spec, Lifecycle):
    /// first revoked at the app, then deleted here, whatever the app said,
    /// and recorded. None: they were not signed in. Some: whether the app
    /// itself ended the sign-in, and why not.
    pub async fn disconnect(&self, app: &str, person: &str) -> Result<Option<(bool, String)>> {
        let name = connection_name(app, person);
        let Some(s) = self.vault.get(&name)? else { return Ok(None) };
        let (done, detail) = revoke(s.expose()).await;
        self.vault.remove(&name)?;
        self.live_tools.lock().await.remove(&name);
        self.brokered.stop(Some(app), Some(person)).await;
        tracing::info!("{person} disconnected {app}: revoked {done}: {detail}");
        self.emit(&format!("person:{person}"), Ev::AppDisconnected { app: app.to_string(), person: person.to_string(), at_provider: done, detail: detail.clone() }).await?;
        Ok(Some((done, detail)))
    }
}

/// Revoke a signed-in app's tokens at its authorization server (RFC 7009):
/// whether it was done there, and what happened.
pub async fn revoke(stored: &str) -> (bool, String) {
    let Ok(t) = serde_json::from_str::<Tokens>(stored) else { return (false, "the stored sign-in could not be read".into()) };
    let Some(endpoint) = t.client.revocation_endpoint.clone() else {
        return (false, "the app publishes no revocation endpoint; its tokens stay valid until they expire".into());
    };
    let mut done = vec![];
    for (token, hint) in [(t.refresh_token.clone(), "refresh_token"), (Some(t.access_token.clone()), "access_token")] {
        let Some(token) = token else { continue };
        let mut form = vec![("token", token), ("token_type_hint", hint.to_string()), ("client_id", t.client.client_id.clone())];
        if let Some(s) = &t.client.client_secret {
            form.push(("client_secret", s.clone()));
        }
        match http().post(&endpoint).form(&form).send().await {
            Ok(r) if r.status().is_success() => done.push(hint),
            Ok(r) => return (false, format!("the app refused to revoke its {hint}: HTTP {}", r.status())),
            Err(e) => return (false, format!("could not reach the app's revocation endpoint: {e}")),
        }
    }
    (true, format!("revoked by the app ({})", done.join(", ")))
}

/// The owner's starting classification for a tool, from the app's own hints
/// (MCP tool annotations). Hints are the app's claims, so a tool that says
/// nothing is assumed to do the most.
pub fn suggested_risk(annotations: &Value) -> &'static str {
    match (annotations["readOnlyHint"].as_bool(), annotations["destructiveHint"].as_bool()) {
        (Some(true), _) => "read",
        (_, Some(false)) => "reversible_write",
        _ => "irreversible",
    }
}

// The API: the organisation's apps, which the owner adds, classifies and
// removes; and each person's own sign-in to them, which only they make and
// end, and which only their runs use.

use crate::api::{err, internal, owner, who};
use crate::events::Ev;
use axum::{
    extract::{Path, Query, State as AxState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use std::sync::Arc;

type K = AxState<Arc<Kernel>>;

/// A refusal: its status and what to tell the person.
pub type Refusal = (StatusCode, String);

fn refusal(st: StatusCode, m: impl Into<String>) -> Refusal {
    (st, m.into())
}

fn failed(e: anyhow::Error) -> Refusal {
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
                s.connectors.keys().cloned().collect::<Vec<String>>(),
                s.credential_changed.clone(),
                s.registrations.clone(),
                s.runs.values().map(|r| r.person.clone()).collect::<std::collections::BTreeSet<String>>(),
            )
        });
        let vault: Vec<String> = self.vault.names().unwrap_or_default();
        self.all_connectors()
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
                let signed_in = vault.iter().filter(|n| n.starts_with(&format!("{}__", credential_name(&c.name)))).count();
                let last_used = people.iter().filter_map(|p| self.recent_uses(&c.name, p, 1).first().and_then(|u| u["at"].as_str().map(String::from))).max();
                // A catalog app's tools are classified by its signed manifest;
                // one added by its address waits for the owner's review.
                let unreviewed = !c.catalog && c.tools.is_empty();
                let status = if !on {
                    "off"
                } else if kind != "none" && !set {
                    "needs_credential"
                } else if unreviewed {
                    "needs_review"
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

    /// Add an app by its MCP address, as `actor`. Whether it needs a
    /// sign-in is asked of the app itself.
    pub async fn add_connector(&self, actor: &str, b: &Value) -> Result<Value, Refusal> {
        let s = |f: &str| b[f].as_str().map(str::trim).unwrap_or_default().to_string();
        let (mut name, url, audience, label) = (s("name"), s("url"), s("audience"), s("label"));
        let taken = |n: &str| self.all_connectors().iter().any(|(c, _)| c.name == n) || crate::tools::builtin_apps().contains(&n);
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
            let own = own_name(&url).await;
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
        if !s("token").is_empty() || !b["client_id"].is_null() {
            return Err(refusal(StatusCode::BAD_REQUEST, "Magnus never takes a key, token or client details for an app: each person signs in to it with their own account"));
        }
        let oauth = match asks_sign_in(&url).await {
            Ok(None) => false,
            Ok(Some(meta)) => {
                let (asm, _, _) = discover(&url, meta).await.map_err(|e| {
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
            name: name.clone(),
            transport: "mcp_http".into(),
            url,
            credential: oauth.then(|| credential_name(&name)),
            audience,
            label,
            access_groups: b["access_groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect(),
            blocked_groups: b["blocked_groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect(),
            system_of_record: b["system_of_record"].as_bool().unwrap_or(false),
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
            watch: vec![],
        };
        self.emit(actor, Ev::ConnectorAdded { connector: c }).await.map_err(failed)?;
        Ok(json!({ "name": name, "sign_in": oauth }))
    }

    /// An app added from the app or the console (not the config file's, not
    /// the catalog's).
    pub fn added_connector(&self, name: &str) -> Result<Connector, Refusal> {
        if self.cfg.connectors.iter().any(|c| c.name == name) {
            return Err(refusal(StatusCode::CONFLICT, "this app is set in the appliance's config file; change it there"));
        }
        match self.read(|s| s.connectors.get(name).cloned()) {
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
            match self.vault.remove(&cred).map_err(failed)? {
                Some(s) if c.auth.as_deref() == Some(OAUTH) => {
                    let (done, detail) = revoke(s.expose()).await;
                    tracing::info!("{}: revoked {cred} {done}: {detail}", c.name);
                    n += 1;
                }
                Some(_) => n += 1,
                None => {}
            }
        }
        Ok(n)
    }

    /// Remove an added app: every sign-in to it revoked, its key deleted.
    pub async fn remove_connector(&self, actor: &str, name: &str) -> Result<Value, Refusal> {
        let c = self.added_connector(name)?;
        let n = self.revoke_all(&c).await?;
        self.emit(actor, Ev::ConnectorRemoved { name: name.to_string() }).await.map_err(failed)?;
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
        let Some((c, on)) = self.all_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
        let clean = |g: Option<Vec<String>>| g.map(|g| g.into_iter().map(|x| x.trim().to_string()).collect::<Vec<String>>());
        let (access_groups, blocked_groups) = (clean(access_groups), clean(blocked_groups));
        if [&access_groups, &blocked_groups].iter().any(|l| l.as_ref().is_some_and(|g| g.iter().any(|g| g.is_empty() || g.len() > 100))) {
            return Err(refusal(StatusCode::BAD_REQUEST, "each group is a group's name"));
        }
        let had = self.read(|s| s.connector_settings.get(name).cloned()).unwrap_or_default();
        let groups = access_groups.or(had.access_groups);
        let blocked = blocked_groups.or(had.blocked_groups);
        let enabled = enabled.unwrap_or(on);
        self.emit(actor, Ev::ConnectorSettings { name: name.to_string(), enabled, access_groups: groups.clone(), blocked_groups: blocked.clone() }).await.map_err(failed)?;
        Ok(json!({ "name": name, "enabled": enabled, "access_groups": groups.unwrap_or(c.access_groups), "blocked_groups": blocked.unwrap_or(c.blocked_groups) }))
    }

    /// Set, or rotate, the organisation's credential for an app. Nothing
    /// shows it again: the log records only that it was set.
    pub async fn set_connector_credential(&self, actor: &str, name: &str, value: &str) -> Result<Value, Refusal> {
        let Some((c, _)) = self.all_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
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
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(refusal(StatusCode::BAD_REQUEST, "give the provider's client secret"));
        }
        self.vault.put(&crate::catalog::registration_name(provider), &json!({ "client_secret": secret }).to_string()).map_err(failed)?;
        self.emit(actor, Ev::RegistrationSet { provider: provider.to_string(), set: true }).await.map_err(failed)?;
        Ok(json!({ "set": true }))
    }

    /// Revoke the organisation's credential for an app, and every person's
    /// sign-in to it, at the app. A provider's client secret serves all of
    /// its apps: revoking it ends the sign-ins to each of them.
    pub async fn revoke_connector_credential(&self, actor: &str, name: &str) -> Result<Value, Refusal> {
        let Some((c, _)) = self.all_connectors().into_iter().find(|(c, _)| c.name == name) else { return Err(refusal(StatusCode::NOT_FOUND, "no such app")) };
        match org_credential(&c) {
            Some(OrgCredential::Registration(p)) => {
                let mut n = 0;
                for (other, _) in self.all_connectors().into_iter().filter(|(o, _)| o.registration.as_deref() == Some(p.as_str())) {
                    n += self.revoke_all(&other).await?;
                }
                self.vault.remove(&crate::catalog::registration_name(&p)).map_err(failed)?;
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

/// A kernel operation's answer as a route's.
fn answer(r: Result<Value, Refusal>, ok: StatusCode) -> Response {
    match r {
        Ok(_) if ok == StatusCode::NO_CONTENT => StatusCode::NO_CONTENT.into_response(),
        Ok(v) => (ok, Json(v)).into_response(),
        Err((st, m)) => err(st, m),
    }
}

/// Whether `state` is a Slack sign-in still waiting, started with the
/// registered https redirect. The pending entry stays: the desktop finishes it.
fn slack_sign_in_open(state: &str) -> bool {
    pending().lock().unwrap().get(state).is_some_and(|p| p.app == "slack" && p.redirect_uri == SLACK_OAUTH_CALLBACK && p.at.elapsed() < PENDING_FOR)
}

/// The desktop's loopback address with only this sign-in's answer. The code
/// is not written anywhere else.
pub(crate) fn slack_handoff_url(state: &str, code: Option<&str>, iss: Option<&str>, error: Option<&str>) -> String {
    let mut u = url::Url::parse(SLACK_HANDOFF).expect("the handoff is a url");
    {
        let mut q = u.query_pairs_mut();
        if let Some(c) = code.filter(|s| !s.is_empty()) {
            q.append_pair("code", c);
        }
        q.append_pair("state", state);
        if let Some(i) = iss.filter(|s| !s.is_empty()) {
            q.append_pair("iss", i);
        }
        if let Some(e) = error.filter(|s| !s.is_empty()) {
            q.append_pair("error", e);
        }
    }
    u.to_string()
}

/// Slack's browser comes here. A waiting sign-in is sent on to the desktop
/// app; anything else is an empty 404. The code is not logged.
async fn slack_oauth_callback(Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(state) = q.get("state").map(String::as_str).filter(|s| slack_sign_in_open(s)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let loc = slack_handoff_url(state, q.get("code").map(String::as_str), q.get("iss").map(String::as_str), q.get("error").map(String::as_str));
    let Ok(location) = loc.parse() else { return StatusCode::NOT_FOUND.into_response() };
    let mut r = Response::new(axum::body::Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    r.headers_mut().insert(axum::http::header::LOCATION, location);
    r.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    r.headers_mut().insert(axum::http::header::REFERRER_POLICY, axum::http::HeaderValue::from_static("no-referrer"));
    r
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
}

/// Alpha Vector provisions its client secret for a provider to this
/// appliance (its deployment calls this; no person types it, and nothing
/// shows it again), until the relay holds it (connectors spec rev 46). The
/// log records only that it was provisioned.
async fn set_registration(AxState(k): K, h: HeaderMap, Path(provider): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    answer(k.set_registration(&format!("person:{}", p.id), &provider, b["client_secret"].as_str().unwrap_or_default()).await, StatusCode::NO_CONTENT)
}

macro_rules! owner {
    ($k:expr, $h:expr) => {
        match owner(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

/// The organisation's apps, each with whether the person asking is
/// signed in to it. The owner also gets each app's tools as the app lists
/// them now (through the owner's own sign-in), each with the owner's
/// classification (none: the worker is not offered it) and the
/// classification the app's own hints suggest.
async fn list(AxState(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let added: Vec<String> = k.read(|s| s.connectors.keys().cloned().collect());
    let mut out = vec![];
    for c in k.connectors() {
        // An app blocked for the person's groups, or for groups they are
        // not in, is not in their Library (connectors spec, Org policy).
        // The owner still manages it in Settings, marked as not theirs.
        let theirs = crate::boundary::may_use(&p, &c);
        if !theirs && (!p.is_owner() || c.owner.is_some()) {
            continue;
        }
        let from_app = added.contains(&c.name) && k.cfg.connectors.iter().all(|f| f.name != c.name);
        let mut card = card(&k, &c, &p.id).await;
        card["for_you"] = json!(theirs);
        let secret = k.connector_secret(&c, &p.id).await.ok().flatten();
        let signed_in = c.credential.is_none() || secret.is_some();
        if !p.is_owner() {
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
                            json!({
                                "name": t.name, "description": t.description, "annotations": t.annotations,
                                "risk": c.tools.get(&t.name).map(|x| x.risk.clone()), "suggested": suggested_risk(&t.annotations),
                            })
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
    Json(json!({ "connectors": out })).into_response()
}

/// A name for an app: what it calls itself, else the first part of its
/// host that says something ("mcp.notion.com" is "notion"), made unique.
fn name_from(url: &str, own: Option<&str>, taken: impl Fn(&str) -> bool) -> String {
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

/// What the Library shows of an app beyond its card's sentence (connectors
/// spec, Library): from its catalog manifest; an app the organisation added
/// by its address has no publisher, and no review mark.
pub(crate) fn listing(c: &Connector) -> Value {
    match crate::catalog::manifest(&c.name).filter(|_| c.catalog) {
        Some(m) => json!({
            "publisher": m.publisher, "reviewed": true, "category": m.category, "abilities": m.abilities,
            "asks": m.asks, "can": m.can, "cannot": m.cannot, "letters": m.letters, "tint": m.tint,
        }),
        None => {
            let t = c.title.clone().unwrap_or_else(|| title(&c.name));
            json!({ "publisher": null, "reviewed": false, "category": "work", "abilities": [], "asks": [], "can": null, "cannot": null, "letters": t.chars().take(2).collect::<String>(), "tint": "#3A3F48" })
        }
    }
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
        "ready": k.provider_ready(c),
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
async fn detail(AxState(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(c) = k.connector(&name).filter(|c| (p.is_owner() && c.owner.is_none()) || crate::boundary::may_use(&p, c)) else { return err(StatusCode::NOT_FOUND, "no such app") };
    let mut v = card(&k, &c, &p.id).await;
    v["for_you"] = json!(crate::boundary::may_use(&p, &c));
    v["recent"] = json!(k.recent_uses(&c.name, &p.id, 10));
    Json(v).into_response()
}

/// Add an app by its MCP address.
async fn add(AxState(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = owner!(k, h);
    answer(k.add_connector(&format!("person:{}", p.id), &b).await, StatusCode::CREATED)
}

fn added(k: &Kernel, name: &str) -> Result<Connector, Response> {
    k.added_connector(name).map_err(|(st, m)| err(st, m))
}

/// Remove an app: its sign-in is revoked at the app and its credential deleted.
async fn remove(AxState(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = owner!(k, h);
    answer(k.remove_connector(&format!("person:{}", p.id), &name).await, StatusCode::NO_CONTENT)
}

/// Classify one tool: its risk class, or null to stop offering it.
async fn classify(AxState(k): K, h: HeaderMap, Path((name, tool)): Path<(String, String)>, Json(b): Json<Value>) -> Response {
    let p = owner!(k, h);
    if let Err(r) = added(&k, &name) {
        return r;
    }
    let risk = b["risk"].as_str().map(String::from);
    if risk.as_deref().is_some_and(|r| crate::tools::Risk::parse(r).is_none()) {
        return err(StatusCode::BAD_REQUEST, "risk is read, reversible_write, irreversible, financial or prohibited");
    }
    // The sandbox is the appliance's own microVM: an app's tool acts at the
    // app, so classed so it would skip the contract and the Rule of Two.
    if risk.as_deref() == Some("sandbox") {
        return err(StatusCode::BAD_REQUEST, "sandbox is the appliance's own microVM, never an app's tool: risk is read, reversible_write, irreversible, financial or prohibited");
    }
    if let Err(e) = k.emit(&format!("person:{}", p.id), Ev::ToolClassified { connector: name, tool, risk }).await {
        return internal(e);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// A person signs in to one of the organisation's apps with their own
/// account; the sign-in is theirs alone.
async fn sign_in(AxState(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(c) = k.connector(&name).filter(|c| c.auth.as_deref() == Some(OAUTH)) else { return err(StatusCode::NOT_FOUND, "no app by that name signs in") };
    if !crate::boundary::may_use(&p, &c) {
        return err(StatusCode::FORBIDDEN, "your organisation has not made this app available to you");
    }
    if k.read(|s| s.kill_switch) {
        return err(StatusCode::CONFLICT, "the kill switch is engaged");
    }
    match k.connector_sign_in(&c, &p.id, b["redirect_uri"].as_str().unwrap_or_default()).await {
        Ok((url, state)) => Json(json!({ "authorize_url": url, "state": state })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, &format!("{e:#}")),
    }
}

async fn sign_in_complete(AxState(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (Some(state), Some(code)) = (b["state"].as_str(), b["code"].as_str()) else { return err(StatusCode::BAD_REQUEST, "state and code are required") };
    match k.connector_sign_in_complete(&name, &p.id, state, code, b["iss"].as_str()).await {
        Ok(()) => {
            // Connected for the first time: Magnus opens a conversation about it.
            k.open_after_connect(&p.id, &name);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, &format!("{e:#}")),
    }
}

/// A person ends their own sign-in to an app.
async fn disconnect(AxState(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    // Turned off, an app keeps its sign-ins: each person can still end theirs.
    if !k.all_connectors().iter().any(|(c, _)| c.name == name) {
        return err(StatusCode::NOT_FOUND, "no such app");
    }
    match k.disconnect(&name, &p.id).await {
        // Where the app could not end the sign-in itself, the person is
        // told where they remove Magnus there (connectors spec, Lifecycle).
        Ok(Some((at_provider, detail))) => {
            let remove_at = (!at_provider).then(|| crate::catalog::manifests().iter().find(|m| m.id == name).and_then(|m| m.remove_access.clone())).flatten();
            Json(json!({ "at_provider": at_provider, "detail": detail, "remove_at": remove_at })).into_response()
        }
        Ok(None) => err(StatusCode::NOT_FOUND, "you are not signed in to this app"),
        Err(e) => internal(e),
    }
}

#[cfg(test)]
mod naming {
    /// An app added by its address is named by what it calls itself, else
    /// by the part of its host that says something, made unique.
    #[test]
    fn an_app_is_named_from_its_address() {
        let none = |_: &str| false;
        assert_eq!(super::name_from("https://mcp.notion.com/mcp", None, none), "notion");
        assert_eq!(super::name_from("https://mcp.linear.app/mcp", None, none), "linear");
        assert_eq!(super::name_from("https://drivemcp.googleapis.com/mcp/v1", None, none), "drivemcp");
        assert_eq!(super::name_from("https://mcp.example.com/mcp", Some("Team Calendar"), none), "team-calendar");
        assert_eq!(super::name_from("http://127.0.0.1:8080/mcp", None, none), "app");
        assert_eq!(super::name_from("https://mcp.notion.com/mcp", None, |n| n == "notion"), "notion-2");
    }

    /// A Slack answer that also carries a bot token keeps the person's
    /// user token, and a dead refresh is told apart from a limit.
    #[test]
    fn a_slack_answer_keeps_the_persons_token_not_the_bots() {
        let both = serde_json::json!({
            "ok": true, "access_token": "xoxb-bot", "token_type": "bot", "refresh_token": "xoxe-bot", "expires_in": 43200,
            "authed_user": { "id": "U1", "access_token": "xoxp-user", "refresh_token": "xoxe-user", "expires_in": 43200, "token_type": "user" },
            "team": { "id": "T1", "name": "Veyra" }
        });
        let g = super::granted(&both).unwrap();
        assert_eq!((g.access.as_str(), g.refresh.as_deref(), g.expires, g.account.as_deref()), ("xoxp-user", Some("xoxe-user"), Some(43200), Some("U1 in Veyra")));
        let user = serde_json::json!({ "ok": true, "access_token": "xoxp-only", "token_type": "user", "refresh_token": "xoxe-1", "expires_in": 43200, "authed_user": { "id": "U2" }, "team": { "name": "Veyra" } });
        assert_eq!(super::granted(&user).unwrap().access, "xoxp-only");
        let google = serde_json::json!({ "access_token": "ya29", "refresh_token": "1//", "expires_in": 3600, "token_type": "Bearer" });
        assert_eq!(super::granted(&google).unwrap().access, "ya29");
        assert!(super::granted(&serde_json::json!({ "ok": false, "error": "invalid_code" })).is_err());
        let bounced = super::slack_handoff_url("st", Some("the-code"), Some("https://mcp.slack.com"), None);
        assert!(bounced.starts_with("http://127.0.0.1:17321/callback?"));
        let back: std::collections::HashMap<String, String> = url::Url::parse(&bounced).unwrap().query_pairs().into_owned().collect();
        assert_eq!(back.get("code").map(String::as_str), Some("the-code"));
        assert_eq!(back.get("state").map(String::as_str), Some("st"));
        assert_eq!(back.get("iss").map(String::as_str), Some("https://mcp.slack.com"));
        assert!(!bounced.contains("client_secret"));
        assert!(super::dead_token(&serde_json::json!({ "ok": false, "error": "invalid_refresh_token" })));
        assert!(!super::dead_token(&serde_json::json!({ "ok": false, "error": "ratelimited" })));
        assert!(!super::dead_token(&google));
    }

    /// A refused registration waits out the server's `Retry-After`, capped,
    /// else a short backoff (#315).
    #[test]
    fn a_refused_registration_waits_out_retry_after_then_backs_off() {
        use std::time::Duration;
        let headers = |v: Option<&str>| {
            let mut h = reqwest::header::HeaderMap::new();
            if let Some(v) = v {
                h.insert("retry-after", v.parse().unwrap());
            }
            h
        };
        assert_eq!(super::registration_backoff(&headers(Some("3")), 0), Duration::from_secs(3));
        assert_eq!(super::registration_backoff(&headers(Some("120")), 0), Duration::from_secs(10));
        assert_eq!(super::registration_backoff(&headers(None), 0), Duration::from_secs(1));
        assert_eq!(super::registration_backoff(&headers(None), 1), Duration::from_secs(2));
        assert_eq!(super::registration_backoff(&headers(Some("tomorrow")), 1), Duration::from_secs(2));
    }
}
