//! The server's card signing keys: Ed25519 seeds kept in the vault, one for
//! each rotation period. The card is signed with the current period's key;
//! the published key set holds it and the one before, so a card fetched
//! just before a rotation still verifies. A key two periods old is removed
//! from the vault the first time the next one is made.

use super::jws;
use crate::vault::Vault;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use chrono::{DateTime, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};

/// How long one card signing key is used before the next takes over.
pub const CARD_KEY_ROTATION_DAYS: i64 = 30;
const DAY_SECS: i64 = 86_400;
const VAULT_PREFIX: &str = "a2a_card_key/";

/// The rotation period `at` falls in.
pub fn period(at: DateTime<Utc>) -> i64 {
    at.timestamp().div_euclid(CARD_KEY_ROTATION_DAYS * DAY_SECS)
}

pub fn kid(period: i64) -> String {
    format!("magnus-card-{period}")
}

fn vault_name(period: i64) -> String {
    format!("{VAULT_PREFIX}{period}")
}

/// The key of `period`, made the first time it is needed; making it removes
/// the keys older than the one before it.
fn key(vault: &Vault, period: i64, make: bool) -> Result<Option<Ed25519KeyPair>> {
    let seed: [u8; 32] = match vault.get(&vault_name(period))? {
        Some(s) => B64.decode(s.expose()).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| anyhow!("a stored card signing key is not one"))?,
        None if make => {
            use rand::RngCore;
            let mut b = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut b);
            vault.put(&vault_name(period), &B64.encode(b))?;
            for old in vault.names()?.into_iter().filter(|n| n.strip_prefix(VAULT_PREFIX).and_then(|p| p.parse::<i64>().ok()).is_some_and(|p| p < period - 1)) {
                vault.remove(&old)?;
            }
            b
        }
        None => return Ok(None),
    };
    Ok(Some(Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|e| anyhow!("a card signing key: {e}"))?))
}

/// `card` with the current key's signature, its key set at `jku`.
pub fn signed(vault: &Vault, mut card: Value, jku: &str, now: DateTime<Utc>) -> Result<Value> {
    let p = period(now);
    let k = key(vault, p, true)?.ok_or_else(|| anyhow!("no card signing key"))?;
    let sig = jws::sign(&card, &k, &kid(p), jku);
    card["signatures"] = json!([sig]);
    Ok(card)
}

/// The published key set: the current key and the one before it.
pub fn key_set(vault: &Vault, now: DateTime<Utc>) -> Result<Value> {
    let p = period(now);
    let mut keys = vec![];
    for (q, make) in [(p, true), (p - 1, false)] {
        if let Some(k) = key(vault, q, make)? {
            keys.push(jws::ed25519_jwk(k.public_key().as_ref(), &kid(q)));
        }
    }
    Ok(json!({ "keys": keys }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card is signed with the current period's key, which the key set
    /// publishes with the one before; two periods on, the oldest is gone
    /// from the vault and the card's key id has moved on.
    #[test]
    fn the_card_signing_key_rotates_and_the_old_one_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let t0 = DateTime::from_timestamp(CARD_KEY_ROTATION_DAYS * DAY_SECS * 100, 0).unwrap();
        let card = signed(&v, json!({ "name": "Magnus" }), "https://m.example/jwks", t0).unwrap();
        let h = jws::header(&card["signatures"][0]).unwrap();
        assert_eq!(h.kid, kid(100));
        let set = key_set(&v, t0).unwrap();
        jws::verify(&card, &card["signatures"][0], &set["keys"][0]).unwrap();
        let t1 = t0 + chrono::Duration::days(CARD_KEY_ROTATION_DAYS);
        let next = signed(&v, json!({ "name": "Magnus" }), "https://m.example/jwks", t1).unwrap();
        assert_eq!(jws::header(&next["signatures"][0]).unwrap().kid, kid(101));
        let set = key_set(&v, t1).unwrap();
        let kids: Vec<&str> = set["keys"].as_array().unwrap().iter().map(|k| k["kid"].as_str().unwrap()).collect();
        assert_eq!(kids, vec![kid(101).as_str(), kid(100).as_str()], "the key before is still published");
        let t2 = t1 + chrono::Duration::days(CARD_KEY_ROTATION_DAYS);
        signed(&v, json!({ "name": "Magnus" }), "https://m.example/jwks", t2).unwrap();
        assert!(v.get(&vault_name(100)).unwrap().is_none(), "a key two periods old is still kept");
        assert!(v.get(&vault_name(101)).unwrap().is_some());
    }
}
