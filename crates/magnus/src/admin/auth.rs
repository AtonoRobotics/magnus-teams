//! Signing in to the admin console (admin console spec, Authentication and
//! session; owner's rulings 1 and 2: no passkeys, so an admin signs in and
//! steps up with their password, and their code when they use one, on the
//! console's own form, against the local store).
//!
//! The session is the console's, server-side: the browser holds only a
//! cookie (`__Host-magnus_admin`), every change carries the session's CSRF
//! token, and the kernel is asked again at least every few minutes, so an
//! admin turned off there loses the console within 5.
//!
//! The console's own key below proves nothing anymore (#508): it only
//! re-prints its public half for the frozen live realm's console client
//! (`magnus-admin jwk`, for `release/admin-on-appliance.sh`), until the
//! removal slice deletes the realm registration with the realm.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use ring::signature::{self, KeyPair};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

pub const COOKIE: &str = "__Host-magnus_admin";
/// A sign-in or step-up form's binding must come back within this.
const PENDING_MINUTES: i64 = 10;

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// The console's own key, made on its first start and kept on the
/// appliance (readable by its owner only). Keycloak holds its public half.
pub struct Key {
    pair: signature::EcdsaKeyPair,
    kid: String,
}

impl Key {
    pub fn load_or_make(dir: &Path) -> Result<Key> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("client-key.p8");
        let rng = ring::rand::SystemRandom::new();
        if !path.exists() {
            let doc = signature::EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).map_err(|_| anyhow!("make the console's key"))?;
            crate::vault::write_private(&path, doc.as_ref())?;
        }
        let der = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let pair =
            signature::EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &der, &rng).map_err(|_| anyhow!("{} is not the console's key", path.display()))?;
        use sha2::Digest;
        let kid = b64(&sha2::Sha256::digest(pair.public_key().as_ref())[..16]);
        Ok(Key { pair, kid })
    }

    /// Its public half as a JWK set, for the frozen live realm's
    /// console client, until the removal slice deletes that registration.
    pub fn jwks(&self) -> Value {
        let p = self.pair.public_key().as_ref();
        json!({ "keys": [{ "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": self.kid, "x": b64(&p[1..33]), "y": b64(&p[33..65]) }] })
    }
}

/// Check a JWT's signature against a JWK set: ES256 or RS256 only.
/// An admin's console session. The provider's refresh token stays here,
/// never in the browser.
#[derive(Clone, Debug)]
pub struct Session {
    pub sub: String,
    pub name: String,
    pub csrf: String,
    pub started: DateTime<Utc>,
    pub seen: DateTime<Utc>,
    pub refreshed: DateTime<Utc>,
    pub stepped_up: Option<DateTime<Utc>>,
}

impl Session {
    /// Whether a password was typed in the last `window` seconds.
    pub fn stepped_up(&self, now: DateTime<Utc>, window: i64) -> bool {
        self.stepped_up.is_some_and(|t| now - t < Duration::seconds(window))
    }
}

#[derive(Default)]
pub struct Store {
    pub sessions: Mutex<HashMap<String, Session>>,
    /// Login-CSRF tokens for the local forms: token to minted. In-memory
    /// only; a restart just asks the browser to reload its form (#456).
    pub login_tokens: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl Store {
    pub fn add_login_token(&self, token: String) -> Result<()> {
        let mut m = self.login_tokens.lock().unwrap();
        let now = Utc::now();
        m.retain(|_, minted| now - *minted < Duration::minutes(PENDING_MINUTES));
        if m.len() >= 200 {
            bail!("too many sign-ins are waiting; try again in a few minutes");
        }
        m.insert(token, now);
        Ok(())
    }

    /// Take a login token: true once, while fresh.
    pub fn take_login_token(&self, token: &str) -> bool {
        let minted = self.login_tokens.lock().unwrap().remove(token);
        minted.is_some_and(|t| Utc::now() - t < Duration::minutes(PENDING_MINUTES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_consoles_own_key_is_kept_and_prints_its_public_half() {
        let dir = tempfile::tempdir().unwrap();
        let key = Key::load_or_make(dir.path()).unwrap();
        let again = Key::load_or_make(dir.path()).unwrap();
        assert_eq!(key.jwks(), again.jwks(), "the key is not kept");
        assert_eq!(key.jwks()["keys"][0]["kty"], "EC", "the public half is not a JWK set: {}", key.jwks());
    }

    /// A login token takes once, while fresh: unknown and used ones refuse (#456).
    #[test]
    fn login_tokens_take_once() {
        let s = Store::default();
        s.add_login_token("t1".to_string()).unwrap();
        assert!(s.take_login_token("t1"), "a fresh token refused");
        assert!(!s.take_login_token("t1"), "a used token took again");
        assert!(!s.take_login_token("nope"), "an unknown token took");
        s.login_tokens.lock().unwrap().insert("old".to_string(), Utc::now() - Duration::minutes(PENDING_MINUTES + 1));
        assert!(!s.take_login_token("old"), "an expired token took");
    }
}
