//! Where a person stands with an app: their token, refreshed when it
//! expires, whether it works, what Magnus did through it, and its end.

use super::relay;
use super::sign_in::{dead_token, granted, Tokens};
use super::{connection_name, http, Standing, OAUTH};
use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::vault::Secret;
use anyhow::{Context, Result};
use serde_json::{json, Value};

impl Kernel {
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
        let _g = self.connecting.refresh.lock().await;
        let Some(raw) = self.vault.get(name)? else { return Ok(None) };
        let mut t: Tokens = serde_json::from_str(raw.expose()).context("stored sign-in")?;
        if t.lapsed {
            return Ok(None);
        }
        // A minute early, so a token does not expire on its way.
        let now = chrono::Utc::now().timestamp();
        if t.expires_at.is_some_and(|at| at - 60 <= now) {
            let Some(refresh) = t.refresh_token.clone() else { return Ok(None) };
            // Through the relay, which holds the client's secret: a refused refresh
            // ends the sign-in, an unreachable relay leaves it until it expires.
            let r = match t.client.relay.clone() {
                Some(provider) => match relay::refresh(self, &provider, &refresh).await {
                    Ok(relay::Refreshed::Tokens { tokens }) => Some(tokens.as_answer()),
                    Ok(relay::Refreshed::Refused { reason }) => {
                        tracing::warn!("refresh for {} through the relay refused: {reason}", c.name);
                        t.lapsed = true;
                        self.vault.put(name, &serde_json::to_string(&t)?)?;
                        return Ok(None);
                    }
                    Err(e) => {
                        tracing::warn!("refresh for {} through the relay: {e:#}", c.name);
                        return Ok(None);
                    }
                },
                None => None,
            };
            let mut form =
                vec![("grant_type", "refresh_token".to_string()), ("refresh_token", refresh), ("client_id", t.client.client_id.clone()), ("resource", t.client.resource.clone())];
            form.retain(|(k, v)| *k != "resource" || !v.is_empty());
            if let Some(s) = &t.client.client_secret {
                form.push(("client_secret", s.clone()));
            }
            let v: Value = match r {
                Some(v) => v,
                None => match crate::token_endpoint::post(&http(), &t.client.token_endpoint, &form).send().await {
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
                },
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

    /// Where `person` stands with an app, and when they signed in and as
    /// whom, where known.
    pub async fn standing(&self, c: &Connector, person: &str) -> (Standing, Option<i64>, Option<String>) {
        if c.credential.is_none() || c.auth.as_deref() != Some(OAUTH) {
            let connected = self.connected(c, person) && !crate::consent::awaits_connect(self, c);
            return (if connected { Standing::Connected } else { Standing::Available }, None, None);
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
        uses.sort_by_key(|a| std::cmp::Reverse(a.0));
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
    pub async fn disconnect(&self, app: &str, person: &str, actor: &str) -> Result<Option<(bool, String)>> {
        let name = connection_name(app, person);
        let Some(s) = self.vault.get(&name)? else { return Ok(None) };
        // Its push subscription is ended first, while the sign-in still works.
        self.unsubscribe_push(app, person).await;
        let (done, detail) = revoke(self, s.expose()).await;
        self.vault.remove(&name)?;
        self.live_tools.lock().await.remove(&name);
        self.brokered.stop(Some(app), Some(person)).await;
        tracing::info!("{person} disconnected {app}: revoked {done}: {detail}");
        self.emit(actor, Ev::AppDisconnected { app: app.to_string(), person: person.to_string(), at_provider: done, detail: detail.clone() }).await?;
        Ok(Some((done, detail)))
    }
}

/// Revoke a signed-in app's tokens at its authorization server (RFC 7009):
/// whether it was done there, and what happened.
pub async fn revoke(k: &Kernel, stored: &str) -> (bool, String) {
    let Ok(t) = serde_json::from_str::<Tokens>(stored) else { return (false, "the stored sign-in could not be read".into()) };
    if let Some(provider) = &t.client.relay {
        return relay::revoke(k, provider, &[(t.refresh_token.clone(), "refresh_token"), (Some(t.access_token.clone()), "access_token")]).await;
    }
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
