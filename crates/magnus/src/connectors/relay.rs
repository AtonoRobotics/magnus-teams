//! Sign-in through the relay, the server's side (GAP6, step 1;
//! `decisions/2026-11-13-relay-handoff-server-side.md`).
//!
//! A confidential client's secret stays with the relay. The server starts
//! the sign-in: the provider's address names the relay's callback, and
//! `state` is `<server id>.<nonce>`, the nonce kept here while it waits. The
//! relay, given the code, claims the nonce from the server (a signed
//! `claim`; the server answers with whose sign-in it is and its PKCE
//! verifier, signed with its own key), exchanges the code with its secret,
//! and hands the tokens to the server (a signed `handoff`). It keeps no
//! tokens. Refresh and revocation go through the relay the same way, the
//! server signing the request and the relay the answer.
//!
//! Every message is one `Signed` envelope: the message's JSON, and an
//! ed25519 signature over `CONTEXT` followed by those bytes. The relay's
//! public key is configuration (`[relay]`); the server's key is its own, made
//! on first start and kept in the vault, and its public half is what the
//! relay registers (`GET /v1/relay`).

use crate::vault::Vault;
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

mod client;
mod sign_in;
#[cfg(test)]
mod tests;

pub(crate) use client::{refresh, revoke};
pub(crate) use sign_in::router;

/// The relay's callback, the one redirect providers are given (remote
/// access and relay, section 1).
pub const CALLBACK: &str = "https://pyrallon.ai/v1/connections/callback";
/// How far a message's `issued_at` may be from this server's clock, either way.
pub const ISSUED_SKEW_SECS: i64 = 120;
/// What every signature covers before the message itself: a signature made
/// for anything else never verifies here.
pub const CONTEXT: &[u8] = b"magnus-relay-v1\n";
/// The longest server id taken.
pub const SERVER_ID_MAX_BYTES: usize = 64;
/// The vault entry holding the seed of this server's own signing key.
const KEY_NAME: &str = "relay_server_key";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn callback() -> String {
    CALLBACK.into()
}

/// `[relay]`: this server's registration with the relay. Absent, nothing
/// signs in through it, and each provider's secret is the one Alpha Vector
/// provisioned to the server.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The id the relay registered this server under; the first part of
    /// every relayed sign-in's `state`.
    pub server_id: String,
    /// The relay's ed25519 public key, 32 bytes, base64 (either alphabet).
    pub public_key: String,
    /// The relay's API, where refresh and revocation are asked.
    pub url: String,
    /// The redirect providers are given.
    #[serde(default = "callback")]
    pub callback: String,
    /// The catalog providers whose clients the relay holds: their
    /// connectors sign in, refresh and revoke through it.
    #[serde(default)]
    pub via_relay: Vec<String>,
}

/// An https address, or plain http on this machine (a relay under test).
fn web_address(what: &str, s: &str) -> Result<()> {
    let u = url::Url::parse(s).with_context(|| format!("[relay] {what} {s:?} is not an address"))?;
    let local = matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
    if u.scheme() != "https" && !(u.scheme() == "http" && local) {
        bail!("[relay] {what} {s:?} must be https");
    }
    Ok(())
}

impl Settings {
    /// Checked at startup: a server id that fits `state`, a key that is
    /// one, addresses that are https, and providers whose clients take a
    /// secret (a public client has nothing for the relay to hold).
    pub fn check(&self) -> Result<()> {
        let id_ok = !self.server_id.is_empty() && self.server_id.len() <= SERVER_ID_MAX_BYTES && self.server_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        anyhow::ensure!(id_ok, "[relay] server_id must be 1 to {SERVER_ID_MAX_BYTES} letters, digits, '-' or '_'");
        self.key()?;
        web_address("url", &self.url)?;
        web_address("callback", &self.callback)?;
        for (i, p) in self.via_relay.iter().enumerate() {
            let c = crate::catalog::client(p).ok_or_else(|| anyhow!("[relay] via_relay: {p:?} is not a catalog provider"))?;
            anyhow::ensure!(c.needs_secret(), "[relay] via_relay: {p}'s client takes no secret, so there is nothing for the relay to hold");
            anyhow::ensure!(!self.via_relay[..i].contains(p), "[relay] via_relay names {p} twice");
        }
        Ok(())
    }

    /// The relay's public key.
    pub fn key(&self) -> Result<[u8; 32]> {
        let t = self.public_key.trim().trim_end_matches('=');
        let raw = B64.decode(t).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(t)).map_err(|_| anyhow!("[relay] public_key is not base64"))?;
        raw.try_into().map_err(|_| anyhow!("[relay] public_key is not an ed25519 public key (32 bytes)"))
    }

    pub fn relays(&self, provider: &str) -> bool {
        self.via_relay.iter().any(|p| p == provider)
    }
}

/// This server's own signing key, read from the vault (made there on first
/// need) and held in memory, so the kill switch, which seals the vault,
/// can still ask the relay to revoke.
#[derive(Default)]
pub struct Keys {
    seed: Mutex<Option<[u8; 32]>>,
}

impl Keys {
    /// Loaded at boot when a relay is configured.
    pub fn at_boot(s: Option<&Settings>, vault: &Vault) -> Keys {
        let k = Keys::default();
        if s.is_some() {
            if let Err(e) = k.seed(vault) {
                tracing::warn!("the relay signing key could not be read: {e:#}");
            }
        }
        k
    }

    fn seed(&self, vault: &Vault) -> Result<[u8; 32]> {
        let mut held = self.seed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = *held {
            return Ok(s);
        }
        let seed: [u8; 32] = match vault.get(KEY_NAME)? {
            Some(s) => B64.decode(s.expose()).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| anyhow!("the stored relay signing key is not one"))?,
            None => {
                use rand::RngCore;
                let mut b = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut b);
                vault.put(KEY_NAME, &B64.encode(b))?;
                b
            }
        };
        *held = Some(seed);
        Ok(seed)
    }

    fn pair(&self, vault: &Vault) -> Result<ring::signature::Ed25519KeyPair> {
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&self.seed(vault)?).map_err(|e| anyhow!("the relay signing key: {e}"))
    }

    /// The public half, for the relay to register.
    pub fn public(&self, vault: &Vault) -> Result<[u8; 32]> {
        use ring::signature::KeyPair;
        let p = self.pair(vault)?;
        p.public_key().as_ref().try_into().map_err(|_| anyhow!("the relay signing key's public half is not 32 bytes"))
    }

    pub fn seal(&self, vault: &Vault, m: &Message) -> Result<Signed> {
        let pair = self.pair(vault)?;
        seal_with(m, |b| pair.sign(b).as_ref().to_vec())
    }
}

/// A signed message, as it travels.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    /// The message's JSON, base64url.
    pub payload: String,
    /// ed25519 over `CONTEXT` and the payload's bytes, base64url.
    pub signature: String,
}

/// Tokens as the relay hands them over: the person's own (for Slack, the
/// user token, not the bot's), from the provider's token answer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelayTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    /// The account signed in as, where the provider's answer names it: shown, never decided on.
    #[serde(default)]
    pub account: Option<String>,
}

impl RelayTokens {
    /// As a token endpoint's answer, read by the same code as one.
    pub fn as_answer(&self) -> serde_json::Value {
        serde_json::json!({ "access_token": self.access_token, "refresh_token": self.refresh_token, "expires_in": self.expires_in, "scope": self.scope })
    }
}

/// What the relay answers a refresh.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Refreshed {
    Tokens {
        tokens: RelayTokens,
    },
    /// The provider refused the refresh token: the person signs in again.
    Refused {
        reason: String,
    },
}

/// Every message between a server and the relay. `issued_at` is Unix
/// seconds. The relay signs `Claim`, `Handoff`, `Refreshed` and `Revoked`;
/// the server signs `Claimed`, `Refresh` and `Revoke`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    /// The relay has a code for this nonce and asks whose sign-in it is;
    /// `iss` is what the provider's answer named itself (RFC 9207), if anything.
    Claim {
        server_id: String,
        nonce: String,
        iss: Option<String>,
        issued_at: i64,
    },
    /// Whose it is, and the PKCE verifier to exchange the code with.
    Claimed {
        server_id: String,
        nonce: String,
        provider: String,
        connector: String,
        person: String,
        code_verifier: Option<String>,
        issued_at: i64,
    },
    /// The tokens the relay's exchange returned.
    Handoff {
        server_id: String,
        nonce: String,
        connector: String,
        person: String,
        tokens: RelayTokens,
        issued_at: i64,
    },
    Refresh {
        server_id: String,
        request: String,
        provider: String,
        refresh_token: String,
        issued_at: i64,
    },
    Refreshed {
        server_id: String,
        request: String,
        outcome: Refreshed,
        issued_at: i64,
    },
    Revoke {
        server_id: String,
        request: String,
        provider: String,
        token: String,
        token_type_hint: String,
        issued_at: i64,
    },
    Revoked {
        server_id: String,
        request: String,
        done: bool,
        detail: String,
        issued_at: i64,
    },
}

pub fn seal_with(m: &Message, sign: impl FnOnce(&[u8]) -> Vec<u8>) -> Result<Signed> {
    let payload = serde_json::to_vec(m)?;
    let signature = sign(&[CONTEXT, payload.as_slice()].concat());
    Ok(Signed { payload: B64.encode(&payload), signature: B64.encode(signature) })
}

/// The message in `s`, when `key` signed it.
pub fn open(s: &Signed, key: &[u8; 32]) -> Result<Message> {
    let payload = B64.decode(&s.payload).map_err(|_| anyhow!("the payload is not base64url"))?;
    let signature = B64.decode(&s.signature).map_err(|_| anyhow!("the signature is not base64url"))?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&[CONTEXT, payload.as_slice()].concat(), &signature)
        .map_err(|_| anyhow!("the signature does not verify"))?;
    serde_json::from_slice(&payload).context("the message is not one")
}

/// Whether `issued_at` is close enough to now.
pub fn timely(issued_at: i64) -> bool {
    (chrono::Utc::now().timestamp() - issued_at).abs() <= ISSUED_SKEW_SECS
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
