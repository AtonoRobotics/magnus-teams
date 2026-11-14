//! Asking the relay to refresh or revoke a relayed sign-in's tokens: the
//! request signed with this server's key, the answer with the relay's.

use super::super::{http, random};
use super::{now, open, timely, Message, Refreshed, Settings, Signed};
use crate::kernel::Kernel;
use anyhow::{bail, Context, Result};
use std::future::Future;

/// What a server asks of the relay after a sign-in.
pub(crate) trait RelayClient {
    /// New tokens for `refresh_token`, from `provider` through the relay's client.
    fn refresh(&self, provider: &str, refresh_token: &str) -> impl Future<Output = Result<Refreshed>> + Send;
    /// Revoke `token` at `provider` (RFC 7009): whether it was, and what happened.
    fn revoke(&self, provider: &str, token: &str, token_type_hint: &str) -> impl Future<Output = Result<(bool, String)>> + Send;
}

/// The relay at `[relay] url`: `POST v1/refresh` and `POST v1/revoke`, each
/// a `Signed` message answered by one.
pub(crate) struct HttpRelay<'a> {
    k: &'a Kernel,
    s: &'a Settings,
}

impl<'a> HttpRelay<'a> {
    pub(crate) fn of(k: &'a Kernel) -> Result<Self> {
        let Some(s) = k.cfg.relay.as_ref() else { bail!("no relay is configured on this server any more") };
        Ok(HttpRelay { k, s })
    }

    async fn ask(&self, path: &str, m: &Message) -> Result<Message> {
        let env = self.k.relay_keys.seal(&self.k.vault, m)?;
        let r = http().post(format!("{}/{path}", self.s.url.trim_end_matches('/'))).json(&env).send().await.context("could not reach the relay")?;
        anyhow::ensure!(r.status().is_success(), "the relay answered HTTP {}", r.status());
        let back: Signed = r.json().await.context("the relay's answer is not a signed message")?;
        open(&back, &self.s.key()?).context("the relay's answer")
    }
}

impl RelayClient for HttpRelay<'_> {
    async fn refresh(&self, provider: &str, refresh_token: &str) -> Result<Refreshed> {
        let request = random();
        let m =
            Message::Refresh { server_id: self.s.server_id.clone(), request: request.clone(), provider: provider.into(), refresh_token: refresh_token.into(), issued_at: now() };
        match self.ask("v1/refresh", &m).await? {
            Message::Refreshed { server_id, request: r, outcome, issued_at } if server_id == self.s.server_id && r == request && timely(issued_at) => Ok(outcome),
            _ => bail!("the relay's answer is not for this refresh"),
        }
    }

    async fn revoke(&self, provider: &str, token: &str, token_type_hint: &str) -> Result<(bool, String)> {
        let request = random();
        let m = Message::Revoke {
            server_id: self.s.server_id.clone(),
            request: request.clone(),
            provider: provider.into(),
            token: token.into(),
            token_type_hint: token_type_hint.into(),
            issued_at: now(),
        };
        match self.ask("v1/revoke", &m).await? {
            Message::Revoked { server_id, request: r, done, detail, issued_at } if server_id == self.s.server_id && r == request && timely(issued_at) => Ok((done, detail)),
            _ => bail!("the relay's answer is not for this revocation"),
        }
    }
}

/// Refresh a relayed sign-in through the relay.
pub(crate) async fn refresh(k: &Kernel, provider: &str, refresh_token: &str) -> Result<Refreshed> {
    HttpRelay::of(k)?.refresh(provider, refresh_token).await
}

/// Revoke a relayed sign-in's tokens through the relay, each with its hint:
/// whether the provider revoked them all, and what happened.
pub(crate) async fn revoke(k: &Kernel, provider: &str, tokens: &[(Option<String>, &str)]) -> (bool, String) {
    let relay = match HttpRelay::of(k) {
        Ok(r) => r,
        Err(e) => return (false, format!("{e:#}")),
    };
    let mut done = vec![];
    for (token, hint) in tokens {
        let Some(token) = token else { continue };
        match relay.revoke(provider, token, hint).await {
            Ok((true, _)) => done.push(*hint),
            Ok((false, detail)) => return (false, format!("the relay could not revoke the {hint}: {detail}")),
            Err(e) => return (false, format!("could not revoke the {hint} through the relay: {e:#}")),
        }
    }
    (true, format!("revoked through the relay ({})", done.join(", ")))
}
