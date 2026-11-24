//! Agent card signatures (A2A 1.0, Agent Card Signing): a JWS (RFC 7515)
//! over the card with its default-valued properties and its `signatures`
//! removed, canonicalized as RFC 8785 says. Magnus signs its own card with
//! Ed25519 and verifies an outside agent's signed with EdDSA, ES256 or
//! RS256, against a key from its key set.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ring::signature::{self, UnparsedPublicKey};
use serde_json::{json, Map, Value};

/// What the card's signature covers: the card without its signatures and
/// with every default-valued property removed (null, false, 0, "", and an
/// empty list or object, as the protocol's JSON leaves them out),
/// canonicalized (RFC 8785: keys sorted, no white space).
pub fn payload(card: &Value) -> String {
    let mut c = card.clone();
    if let Value::Object(m) = &mut c {
        m.remove("signatures");
    }
    crate::boundary::canonical(&strip(c).unwrap_or(Value::Object(Map::new())))
}

fn strip(v: Value) -> Option<Value> {
    match v {
        Value::Null | Value::Bool(false) => None,
        Value::Number(n) if n.as_f64() == Some(0.0) => None,
        Value::String(s) if s.is_empty() => None,
        Value::Array(a) => {
            let a: Vec<Value> = a.into_iter().filter_map(strip).collect();
            (!a.is_empty()).then_some(Value::Array(a))
        }
        Value::Object(m) => {
            let m: Map<String, Value> = m.into_iter().filter_map(|(k, v)| strip(v).map(|v| (k, v))).collect();
            (!m.is_empty()).then_some(Value::Object(m))
        }
        other => Some(other),
    }
}

/// The JWS signing input for a protected header and a payload.
fn signing_input(protected: &str, payload: &str) -> String {
    format!("{protected}.{}", B64.encode(payload.as_bytes()))
}

/// Sign `card` with an Ed25519 key: one `AgentCardSignature`.
pub fn sign(card: &Value, key: &signature::Ed25519KeyPair, kid: &str, jku: &str) -> Value {
    let header = json!({ "alg": "EdDSA", "typ": "JOSE", "kid": kid, "jku": jku });
    let protected = B64.encode(header.to_string().as_bytes());
    let sig = key.sign(signing_input(&protected, &payload(card)).as_bytes());
    json!({ "protected": protected, "signature": B64.encode(sig.as_ref()) })
}

/// The protected header of one signature: its algorithm, key id and key-set URL.
pub struct Header {
    pub alg: String,
    pub kid: String,
    pub jku: Option<String>,
}

pub fn header(sig: &Value) -> Result<Header> {
    let raw = B64.decode(sig["protected"].as_str().ok_or_else(|| anyhow!("a signature has no protected header"))?).context("the protected header")?;
    let h: Value = serde_json::from_slice(&raw).context("the protected header")?;
    Ok(Header { alg: h["alg"].as_str().unwrap_or_default().to_string(), kid: h["kid"].as_str().unwrap_or_default().to_string(), jku: h["jku"].as_str().map(String::from) })
}

/// Whether `sig` is a valid signature of `card` by the key `jwk`.
pub fn verify(card: &Value, sig: &Value, jwk: &Value) -> Result<()> {
    let h = header(sig)?;
    let protected = sig["protected"].as_str().unwrap_or_default();
    let bytes = B64.decode(sig["signature"].as_str().ok_or_else(|| anyhow!("a signature has no value"))?).context("the signature")?;
    let input = signing_input(protected, &payload(card));
    let field = |k: &str| -> Result<Vec<u8>> { B64.decode(jwk[k].as_str().ok_or_else(|| anyhow!("the key has no {k}"))?).with_context(|| format!("the key's {k}")) };
    let ok = match (h.alg.as_str(), jwk["kty"].as_str()) {
        ("EdDSA", Some("OKP")) if jwk["crv"] == "Ed25519" => UnparsedPublicKey::new(&signature::ED25519, field("x")?).verify(input.as_bytes(), &bytes),
        ("ES256", Some("EC")) if jwk["crv"] == "P-256" => {
            let point: Vec<u8> = [vec![4u8], field("x")?, field("y")?].concat();
            UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point).verify(input.as_bytes(), &bytes)
        }
        ("RS256", Some("RSA")) => signature::RsaPublicKeyComponents { n: field("n")?, e: field("e")? }.verify(&signature::RSA_PKCS1_2048_8192_SHA256, input.as_bytes(), &bytes),
        (alg, kty) => bail!("a signature by {alg} with a {} key is not one Magnus checks (EdDSA, ES256, RS256)", kty.unwrap_or("typeless")),
    };
    ok.map_err(|_| anyhow!("the card's signature does not match its contents"))
}

/// The public half of an Ed25519 key, as a JWK.
pub fn ed25519_jwk(public: &[u8], kid: &str) -> Value {
    json!({ "kty": "OKP", "crv": "Ed25519", "x": B64.encode(public), "kid": kid, "alg": "EdDSA", "use": "sig" })
}

/// Sign `card` with the Ed25519 key made from `seed`: the signature and the
/// key's public half, for an agent outside the kernel (the invariant tests'
/// fake agents) that has no signing library of its own.
pub fn sign_with_seed(card: &Value, seed: &[u8], kid: &str, jku: &str) -> Result<(Value, Value)> {
    use ring::signature::KeyPair;
    let key = signature::Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| anyhow!("an Ed25519 seed is 32 bytes"))?;
    Ok((sign(card, &key, kid, jku), ed25519_jwk(key.public_key().as_ref(), kid)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    fn key(seed: u8) -> signature::Ed25519KeyPair {
        signature::Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
    }

    /// A signed card verifies; changed in any field, or signed by another
    /// key, it does not. Default values and key order do not matter.
    #[test]
    fn a_card_verifies_only_as_signed_by_its_key() {
        let k = key(7);
        let card = json!({ "name": "Supplier", "skills": [], "capabilities": { "streaming": false }, "version": "1" });
        let sig = sign(&card, &k, "k1", "https://agent.example/jwks.json");
        let jwk = ed25519_jwk(k.public_key().as_ref(), "k1");
        verify(&card, &sig, &jwk).unwrap();
        let reordered = json!({ "version": "1", "name": "Supplier", "signatures": [sig.clone()] });
        verify(&reordered, &sig, &jwk).unwrap();
        let changed = json!({ "name": "Supplier.", "version": "1" });
        assert!(verify(&changed, &sig, &jwk).is_err(), "a changed card verified");
        let other = ed25519_jwk(key(8).public_key().as_ref(), "k1");
        assert!(verify(&card, &sig, &other).is_err(), "another key verified it");
        assert_eq!(header(&sig).unwrap().kid, "k1");
    }
}
