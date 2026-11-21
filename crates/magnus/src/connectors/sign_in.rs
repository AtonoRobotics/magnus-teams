//! A person's own sign-in to an app, made here: registration (RFC 7591),
//! the authorization code with PKCE, the answer's checks, and the tokens
//! kept in the vault.

use super::sign_in_target::{sign_in_target, SignInTarget};
use super::{connection_name, http, BACKOFF_FIRST_SECS, KNOWN_POLL, KNOWN_WAIT, PENDING_FOR, PROBE_WAIT, RETRY_AFTER_MOST, SLACK_OAUTH_CALLBACK};
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::oauth::{challenge, loopback, random};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A sign-in waiting for its code.
pub(crate) struct Pending {
    pub(crate) app: String,
    /// Who is signing in: the sign-in becomes theirs alone.
    pub(crate) person: String,
    pub(crate) verifier: String,
    pub(crate) redirect_uri: String,
    pub(crate) client: Client,
    /// The authorization server this sign-in went to, and whether it says
    /// so in its answer (RFC 9207): an answer from another one is refused.
    pub(crate) issuer: String,
    pub(crate) iss_required: bool,
    /// Whether this sign-in sent a PKCE challenge. The verifier is sent
    /// back only then.
    pub(crate) pkce: bool,
    /// The vault name of the registered client this sign-in used again,
    /// forgotten if the token endpoint no longer knows it.
    pub(crate) kept: Option<String>,
    pub(crate) at: Instant,
}

/// Where tokens come from, and who Magnus is there. Kept with the tokens.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Client {
    pub(crate) token_endpoint: String,
    #[serde(default)]
    pub(crate) revocation_endpoint: Option<String>,
    pub(crate) client_id: String,
    #[serde(default)]
    pub(crate) client_secret: Option<String>,
    pub(crate) resource: String,
    /// The catalog provider whose client the relay holds, for a sign-in made through it: refreshed and revoked there.
    #[serde(default)]
    pub(crate) relay: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Tokens {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub(crate) expires_at: Option<i64>,
    /// Seconds an access token lasts, as last said: kept for a refresh
    /// answer that does not say (expires_in is only recommended).
    #[serde(default)]
    pub(crate) lifetime: Option<i64>,
    pub(crate) client: Client,
    /// When the person signed in (Unix seconds).
    #[serde(default)]
    pub(crate) connected_at: Option<i64>,
    /// The account they signed in as, where the app's answer names it.
    #[serde(default)]
    pub(crate) account: Option<String>,
    /// The app refused to refresh it: the person signed Magnus out there, or
    /// the sign-in expired. It stays until they sign in again or disconnect,
    /// and is not tried again meanwhile (connectors spec, Lifecycle).
    #[serde(default)]
    pub(crate) lapsed: bool,
    /// Access the app asked for that this sign-in does not hold (its
    /// step-up challenge): the next sign-in asks for it too.
    #[serde(default)]
    pub(crate) wants: Vec<String>,
}

/// The account a sign-in's answer names, from its ID token's claims, for
/// the person to see which account they connected (not for any decision:
/// the token came straight from the app's token endpoint).
pub(crate) fn account_of(answer: &Value) -> Option<String> {
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
pub(crate) struct Grant {
    pub(crate) access: String,
    pub(crate) refresh: Option<String>,
    pub(crate) expires: Option<i64>,
    pub(crate) account: Option<String>,
}

pub(crate) fn granted(v: &Value) -> Result<Grant, String> {
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

/// A new sign-in's tokens, from what its token answer granted.
pub(crate) fn fresh_tokens(g: Grant, client: Client) -> Tokens {
    let now = chrono::Utc::now().timestamp();
    Tokens {
        access_token: g.access,
        refresh_token: g.refresh,
        expires_at: g.expires.map(|s| now + s),
        lifetime: g.expires,
        client,
        connected_at: Some(now),
        account: g.account,
        lapsed: false,
        wants: vec![],
    }
}

/// Slack says a refresh token is dead with HTTP 200 and `ok: false`. Those
/// end the sign-in; a limit or a blip does not.
pub(crate) fn dead_token(v: &Value) -> bool {
    v["ok"] == false && matches!(v["error"].as_str(), Some("invalid_refresh_token" | "invalid_grant" | "token_revoked" | "invalid_auth" | "token_expired" | "account_inactive"))
}

/// The sign-ins waiting for their code, by their state: the server's own,
/// kept on the kernel.
#[derive(Default)]
pub(crate) struct SignIns(Mutex<HashMap<String, Pending>>);

impl SignIns {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keep a sign-in until its code comes back, dropping any left
    /// unfinished too long.
    fn wait(&self, state: String, p: Pending) {
        let mut m = self.lock();
        m.retain(|_, x| x.at.elapsed() < PENDING_FOR);
        m.insert(state, p);
    }

    /// The sign-in `state` names, taken only by the person who started it,
    /// for the app they started it for: anyone else's attempt leaves it
    /// waiting for them. None, too, once it is older than `PENDING_FOR`.
    fn take(&self, state: &str, app: &str, person: &str) -> Option<Pending> {
        let mut m = self.lock();
        let p = match m.get(state) {
            Some(p) if p.app == app && p.person == person => m.remove(state),
            _ => None,
        };
        p.filter(|p| p.at.elapsed() < PENDING_FOR)
    }

    /// Whether `state` is a Slack sign-in still waiting, started with the
    /// registered https redirect. It stays: the desktop finishes it.
    pub(crate) fn slack_open(&self, state: &str) -> bool {
        self.lock().get(state).is_some_and(|p| p.app == "slack" && p.redirect_uri == SLACK_OAUTH_CALLBACK && p.at.elapsed() < PENDING_FOR)
    }
}

/// What a sign-in's address asks of the authorization server.
pub(crate) struct Authorize<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) redirect_uri: &'a str,
    pub(crate) state: &'a str,
    /// The PKCE verifier, when the sign-in sends a challenge (RFC 7636).
    pub(crate) verifier: Option<&'a str>,
    /// The resource indicator (RFC 8707); empty for none.
    pub(crate) resource: &'a str,
    pub(crate) scopes: &'a [String],
}

/// The scopes a sign-in asks for: exactly what the app's manifest names,
/// where it names any, else what its metadata lists; and the more access
/// the app asked of this person's sign-in.
pub(crate) fn scopes_asked(manifest: &[String], listed: &[String], wanted: &[String]) -> Vec<String> {
    let mut scopes = if manifest.is_empty() { listed.to_vec() } else { manifest.to_vec() };
    for w in wanted {
        if !scopes.contains(w) {
            scopes.push(w.clone());
        }
    }
    scopes
}

/// The address the person's browser opens to sign in, for `client_id`.
pub(crate) fn authorize_url(a: &Authorize, client_id: &str) -> Result<url::Url> {
    let mut u = url::Url::parse(a.endpoint)?;
    let mut q = u.query_pairs_mut();
    q.append_pair("response_type", "code").append_pair("client_id", client_id).append_pair("redirect_uri", a.redirect_uri).append_pair("state", a.state);
    if let Some(v) = a.verifier {
        q.append_pair("code_challenge", &challenge(v)).append_pair("code_challenge_method", "S256");
    }
    if !a.resource.is_empty() {
        q.append_pair("resource", a.resource);
    }
    if !a.scopes.is_empty() {
        q.append_pair("scope", &a.scopes.join(" "));
    }
    // A refresh token that lasts (Google gives one only when asked for
    // offline access).
    if a.endpoint.starts_with("https://accounts.google.com/") {
        q.append_pair("access_type", "offline").append_pair("prompt", "consent");
    }
    drop(q);
    Ok(u)
}

/// The answer must come from the server the sign-in went to (RFC 9207):
/// otherwise the code and verifier would go to this one's token endpoint
/// for another server's code (a mix-up). A server that says it names
/// itself must have.
pub(crate) fn check_answer_issuer(iss: Option<&str>, issuer: &str, required: bool) -> Result<()> {
    match iss {
        Some(i) if !crate::issuers::same(i, issuer) => bail!("the sign-in was answered by {i}, not {issuer}; start again"),
        None if required => bail!("{issuer} says it names itself in its answer, and this answer names no one; start again"),
        _ => Ok(()),
    }
}

/// The token request for a sign-in's code: the verifier only when the
/// sign-in sent a challenge, the resource only when there is one, and the
/// client's secret only when it has one.
pub(crate) fn token_form(code: &str, p: &Pending) -> Vec<(&'static str, String)> {
    let c = &p.client;
    let mut form = vec![("grant_type", "authorization_code".to_string()), ("code", code.to_string()), ("redirect_uri", p.redirect_uri.clone()), ("client_id", c.client_id.clone())];
    if p.pkce {
        form.push(("code_verifier", p.verifier.clone()));
    }
    if !c.resource.is_empty() {
        form.push(("resource", c.resource.clone()));
    }
    if let Some(s) = &c.client_secret {
        form.push(("client_secret", s.clone()));
    }
    form
}

/// How long a refused registration waits before trying again: what the server's `Retry-After` asks,
/// capped (the seconds form; a date falls back to the backoff), else 1 s, 2 s, 4 s. The sign-in is interactive, so the waits stay short.
pub(crate) fn registration_backoff(headers: &reqwest::header::HeaderMap, attempt: u32) -> Duration {
    if let Some(s) = headers.get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()) {
        return Duration::from_secs(s).min(RETRY_AFTER_MOST);
    }
    Duration::from_secs(BACKOFF_FIRST_SECS << attempt.min(2))
}

/// A provider may not know a client it registered a moment ago: GitLab.com answered the sign-in address built from one with
/// 401 invalid_client, "The requested OAuth 2.0 Client does not exist" (master run 42105845733, #170), and the person would land
/// on that error. After registering, the address is handed out once the provider's sign-in answers it with anything else, asked
/// every quarter second for ten seconds at most in all, however slowly it answers (how long the person may wait for the sign-in
/// page); then it is handed out as it is. Redirects are not followed: the answer is the provider's own.
pub(crate) async fn until_client_known(authorize: &str) {
    let known = async {
        while unknown(authorize).await {
            tokio::time::sleep(KNOWN_POLL).await;
        }
    };
    let _ = tokio::time::timeout(KNOWN_WAIT, known).await;
}

pub(crate) fn no_redirects() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(PROBE_WAIT).build()
}

/// The provider's sign-in says it does not know the client (401
/// invalid_client, as GitLab.com answers).
pub(crate) async fn unknown(authorize: &str) -> bool {
    match async { no_redirects()?.get(authorize).send().await }.await {
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
pub(crate) async fn refused(authorize: &str) -> bool {
    match async { no_redirects()?.get(authorize).send().await }.await {
        Ok(r) if matches!(r.status().as_u16(), 400 | 401) => r.text().await.unwrap_or_default().to_lowercase().contains("client"),
        _ => false,
    }
}

/// Where the client Magnus registered at `registration` for a redirect is
/// kept. Any port on a loopback IP address is the same redirect (RFC 8252
/// §7.3; GitLab.com's Doorkeeper and Cloudflare's provider library match it
/// so), so one client serves every sign-in from the person's computer.
/// `localhost` is a name, matched exactly.
pub(crate) fn registered_client_name(registration: &str, redirect_uri: &str) -> String {
    let redirect = match url::Url::parse(redirect_uri) {
        Ok(mut u) if u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "[::1]")) => {
            let _ = u.set_port(None);
            u.to_string()
        }
        _ => redirect_uri.to_string(),
    };
    format!("registered_client {registration} {redirect}")
}

impl Kernel {
    /// Register Magnus at an authorization server for a redirect address
    /// (RFC 7591), and keep the client under `name` for later sign-ins. A
    /// throttled or failing server (429, 5xx) is tried again, briefly: a
    /// provider under load refused a sign-in that a wait would save (#315).
    pub(crate) async fn register_client(&self, name: &str, registration: &str, redirect_uri: &str) -> Result<(String, Option<String>)> {
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
        // A confidential client the relay holds: the redirect is the relay's callback, whatever the app sent.
        if let Some(s) = self.via_relay(c) {
            return self.relay_sign_in(c, person, s).await;
        }
        if let Some(fixed) = &c.redirect_uri {
            if redirect_uri != fixed {
                bail!("sign in to {} from the Magnus app on this computer", c.title.as_deref().unwrap_or(&c.name));
            }
        }
        // A loopback address, Slack's one registered https address, or, from
        // a browser, this server's own return address. The token exchange
        // repeats whichever was sent here.
        if loopback(redirect_uri).is_err() && redirect_uri != SLACK_OAUTH_CALLBACK && !self.is_web_return(c, redirect_uri) {
            loopback(redirect_uri)?;
        }
        let SignInTarget { asm, scopes, issuer, resource } = sign_in_target(c).await?;
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
            Some(provider) => Some(crate::registrations::client(&self.vault, provider).map_err(|e| {
                tracing::warn!("{provider}'s client: {e:#}");
                anyhow!("{} cannot be connected on this server yet: Alpha Vector has not finished setting it up here", shown)
            })?),
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
        let scopes = scopes_asked(&c.scopes, &scopes, &wanted);
        let ask = Authorize { endpoint: &authorize, redirect_uri, state: &state, verifier: c.pkce.then_some(verifier.as_str()), resource: &resource, scopes: &scopes };
        let address = |client_id: &str| authorize_url(&ask, client_id);
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
        let client = Client { token_endpoint: token, revocation_endpoint: s("revocation_endpoint"), client_id, client_secret, resource, relay: None };
        let iss_required = asm["authorization_response_iss_parameter_supported"] == true;
        self.connecting.sign_ins.wait(
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
        let p = self.connecting.sign_ins.take(state, app, person).ok_or_else(|| anyhow!("this sign-in expired or is unknown; start again"))?;
        check_answer_issuer(iss, &p.issuer, p.iss_required)?;
        let c = &p.client;
        let form = token_form(code, &p);
        let r = crate::token_endpoint::post(&http(), &c.token_endpoint, &form).send().await?;
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
        self.keep_sign_in(app, person, fresh_tokens(granted(&v).map_err(|e| anyhow!(e))?, p.client)).await
    }

    /// Keep a person's new sign-in to an app, made here or through the
    /// relay: its tokens in the vault, its tools asked for again, recorded.
    pub(crate) async fn keep_sign_in(&self, app: &str, person: &str, t: Tokens) -> Result<()> {
        let account = t.account.clone();
        self.vault.put(&connection_name(app, person), &serde_json::to_string(&t)?)?;
        self.live_tools.lock().await.remove(&connection_name(app, person));
        self.emit(&format!("person:{person}"), Ev::AppConnected { app: app.to_string(), person: person.to_string(), account }).await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "sign_in_tests.rs"]
mod tests;
