//! Where a person stands with an app: their token, refreshed when it
//! expires, whether it works, what Magnus did through it, and its end.

use super::relay;
use super::sign_in::{dead_token, granted, Tokens};
use super::{connection_name, SignInOwner, Standing, Superseded, OAUTH};
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
    /// sign in again). Each sign-in refreshes on its own turn, and a
    /// refresh writes back only onto the sign-in it read (`SignInStore`).
    pub async fn connector_secret(&self, c: &Connector, person: &str) -> Result<Option<Secret>> {
        let Some(org) = &c.credential else { return Ok(None) };
        if c.auth.as_deref() != Some(OAUTH) {
            return self.vault.get(org);
        }
        let name = &connection_name(&c.name, person);
        let store = &self.connecting.store;
        let _turn = store.refreshing(name).await;
        let (generation, raw) = store.read(&self.vault, name)?;
        let Some(raw) = raw else { return Ok(None) };
        let mut t: Tokens = serde_json::from_str(raw.expose()).context("stored sign-in")?;
        if t.lapsed {
            return Ok(None);
        }
        let now = chrono::Utc::now().timestamp();
        if !t.expires_at.is_some_and(|at| at - REFRESH_EARLY_SECONDS <= now) {
            return Ok(Some(Secret::new(t.access_token)));
        }
        let Some(refresh) = t.refresh_token.clone() else { return Ok(None) };
        let v = match refresh_at_app(self, c, &t, refresh).await {
            Answer::Tokens(v) if !dead_token(&v) => v,
            Answer::Unreachable => return Ok(None),
            // Refused (not a failure to reach it): the sign-in no longer
            // works until the person signs in again.
            Answer::Tokens(_) | Answer::Refused => {
                let _ = store.write_back(&self.vault, name, generation, lapse)?;
                return Ok(None);
            }
        };
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
        t.lifetime = g.expires.or(t.lifetime).or(Some(DEFAULT_LIFETIME_SECONDS));
        t.expires_at = t.lifetime.map(|s| now + s);
        let made = serde_json::to_string(&t)?;
        let onto = |stored: &str| -> Result<String> {
            let mut kept: Tokens = serde_json::from_str(stored).context("stored sign-in")?;
            (kept.access_token, kept.refresh_token, kept.account, kept.lifetime, kept.expires_at) =
                (t.access_token.clone(), t.refresh_token.clone(), t.account.clone(), t.lifetime, t.expires_at);
            Ok(serde_json::to_string(&kept)?)
        };
        match store.write_back(&self.vault, name, generation, onto)? {
            Ok(()) => Ok(Some(Secret::new(t.access_token))),
            // A new sign-in took its place while this refresh was on its
            // way: the new one is used, and what this refresh made is left.
            Err(Superseded::Replaced) => {
                Ok(store.get(&self.vault, name)?.and_then(|raw| serde_json::from_str::<Tokens>(raw.expose()).ok()).filter(|t| !t.lapsed).map(|t| Secret::new(t.access_token)))
            }
            // Ended while this refresh was on its way: it stays ended, and
            // the tokens the refresh was given are revoked too.
            Err(Superseded::Ended) => {
                let (done, detail) = revoke(self, &made).await;
                tracing::info!("{}: a refresh finished after the sign-in ended; its tokens revoked {done}: {detail}", c.name);
                Ok(None)
            }
        }
    }

    /// Where `person` stands with an app, and when they signed in and as
    /// whom, where known.
    pub async fn standing(&self, c: &Connector, person: &str) -> (Standing, Option<i64>, Option<String>) {
        if c.credential.is_none() || c.auth.as_deref() != Some(OAUTH) {
            let connected = self.connected(c, person) && !crate::consent::awaits_connect(self, c);
            return (if connected { Standing::Connected } else { Standing::Available }, None, None);
        }
        let Ok(Some(raw)) = self.connecting.store.get(&self.vault, &connection_name(&c.name, person)) else { return (Standing::Available, None, None) };
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
        self.connecting.store.amend(&self.vault, &connection_name(app, person), |now| {
            let mut t: Tokens = serde_json::from_str(now).context("stored sign-in")?;
            for s in scopes {
                if !t.wants.contains(s) {
                    t.wants.push(s.clone());
                }
            }
            Ok(serde_json::to_string(&t)?)
        })
    }

    /// The person's sign-in to an app as stored, when there is one.
    fn stored_sign_in(&self, app: &str, person: &str) -> Option<Tokens> {
        self.connecting.store.get(&self.vault, &connection_name(app, person)).ok().flatten().and_then(|raw| serde_json::from_str::<Tokens>(raw.expose()).ok())
    }

    /// The access the app asked of the person's sign-in that it does not hold.
    pub fn wanted_scopes(&self, c: &Connector, person: &str) -> Vec<String> {
        self.stored_sign_in(&c.name, person).map(|t| t.wants).unwrap_or_default()
    }

    /// Whether the person's sign-in to an app was refused by the app: they
    /// need to sign in again.
    pub fn lapsed(&self, c: &Connector, person: &str) -> bool {
        c.auth.as_deref() == Some(OAUTH) && self.stored_sign_in(&c.name, person).is_some_and(|t| t.lapsed)
    }

    /// Whether `person` can use an app now: it takes no sign-in, or they
    /// have signed in to it themselves.
    pub fn connected(&self, c: &Connector, person: &str) -> bool {
        match (&c.credential, c.auth.as_deref()) {
            (None, _) => true,
            (Some(_), Some(OAUTH)) => self.connecting.store.get(&self.vault, &connection_name(&c.name, person)).is_ok_and(|s| s.is_some()),
            (Some(org), _) => self.vault.names().is_ok_and(|n| n.contains(org)),
        }
    }

    /// End one person's (or project's) sign-in to an app (connectors spec,
    /// Lifecycle): first revoked at the app, then deleted here, whatever the
    /// app said, and recorded. From the moment it starts ending, no call is
    /// made through it and no refresh begun earlier writes it back. None:
    /// they were not signed in. Some: whether the app itself ended the
    /// sign-in, and why not.
    pub async fn disconnect(&self, app: &str, person: &str, actor: &str) -> Result<Option<(bool, String)>> {
        let name = connection_name(app, person);
        let store = &self.connecting.store;
        if store.get(&self.vault, &name)?.is_none() {
            return Ok(None);
        }
        // Its push subscription is ended first, while the sign-in still works.
        self.unsubscribe_push(app, person).await;
        let Some((generation, s)) = store.ending(&self.vault, &name)? else { return Ok(None) };
        let (done, detail) = revoke(self, s.expose()).await;
        store.ended(&self.vault, &name, generation)?;
        self.live_tools.lock().await.remove(&name);
        self.brokered.stop(Some(app), Some(person)).await;
        tracing::info!("{person} disconnected {app}: revoked {done}: {detail}");
        self.emit(actor, Ev::AppDisconnected { app: app.to_string(), person: person.to_string(), at_provider: done, detail: detail.clone() }).await?;
        Ok(Some((done, detail)))
    }

    /// End every sign-in `owner` holds, to any app, on or off or removed
    /// since: a shared project being deleted. The number ended.
    pub(crate) async fn end_sign_ins_of(&self, owner: &SignInOwner, actor: &str) -> Result<usize> {
        let mut n = 0;
        for name in self.vault.names()? {
            let Some((app, whose)) = SignInOwner::parse(&name) else { continue };
            if &whose == owner && self.disconnect(&app, &whose.key(), actor).await?.is_some() {
                n += 1;
            }
        }
        Ok(n)
    }
}

/// How early a token is refreshed, so it does not expire on its way.
const REFRESH_EARLY_SECONDS: i64 = 60;
/// A refreshed token's lifetime in seconds, when the app never said one.
const DEFAULT_LIFETIME_SECONDS: i64 = 3600;

/// What the app said to a refresh.
enum Answer {
    Tokens(Value),
    /// It refused: the sign-in no longer works.
    Refused,
    /// It could not be reached: the sign-in stands until it expires.
    Unreachable,
}

/// Ask for a fresh token: through the relay, which holds the client's
/// secret, or at the app's own token endpoint.
async fn refresh_at_app(k: &Kernel, c: &Connector, t: &Tokens, refresh: String) -> Answer {
    if let Some(provider) = &t.client.relay {
        return match relay::refresh(k, provider, &refresh).await {
            Ok(relay::Refreshed::Tokens { tokens }) => Answer::Tokens(tokens.as_answer()),
            Ok(relay::Refreshed::Refused { reason }) => {
                tracing::warn!("refresh for {} through the relay refused: {reason}", c.name);
                Answer::Refused
            }
            Err(e) => {
                tracing::warn!("refresh for {} through the relay: {e:#}", c.name);
                Answer::Unreachable
            }
        };
    }
    let mut form =
        vec![("grant_type", "refresh_token".to_string()), ("refresh_token", refresh), ("client_id", t.client.client_id.clone()), ("resource", t.client.resource.clone())];
    form.retain(|(k, v)| *k != "resource" || !v.is_empty());
    if let Some(s) = &t.client.client_secret {
        form.push(("client_secret", s.clone()));
    }
    match crate::token_endpoint::post(&k.app_egress(), &t.client.token_endpoint, &form).await {
        Ok(r) if r.status().is_success() => Answer::Tokens(r.json().await.unwrap_or_default()),
        Ok(r) => {
            tracing::warn!("refresh for {}: HTTP {}", c.name, r.status());
            if r.status().is_client_error() {
                Answer::Refused
            } else {
                Answer::Unreachable
            }
        }
        Err(e) => {
            tracing::warn!("refresh for {}: {e}", c.name);
            Answer::Unreachable
        }
    }
}

/// A stored sign-in marked as refused by the app.
fn lapse(stored: &str) -> Result<String> {
    let mut t: Tokens = serde_json::from_str(stored).context("stored sign-in")?;
    t.lapsed = true;
    Ok(serde_json::to_string(&t)?)
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
        match k.app_egress().send(reqwest::Method::POST, &endpoint, |r| r.form(&form)).await {
            Ok(r) if r.status().is_success() => done.push(hint),
            Ok(r) => return (false, format!("the app refused to revoke its {hint}: HTTP {}", r.status())),
            Err(e) => return (false, format!("could not reach the app's revocation endpoint: {e}")),
        }
    }
    (true, format!("revoked by the app ({})", done.join(", ")))
}

#[cfg(test)]
#[path = "standing_tests.rs"]
mod tests;
