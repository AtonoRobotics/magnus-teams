//! Splitting event records from erasable content.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// One event's content, apart from its record.
pub struct Content {
    /// Random, kept with the content and deleted with it, so the hash left in
    /// the log cannot be matched by guessing short content once it is gone.
    pub salt: Vec<u8>,
    pub body: Value,
}

/// Split an event's body into the record the log keeps and its content,
/// leaving in the record the content's size and salted SHA-256.
pub fn split(mut body: Value) -> (Value, Option<Content>) {
    let kind = body["kind"].as_str().unwrap_or_default().to_string();
    let mut taken = serde_json::Map::new();
    for f in crate::events::content_fields(&kind) {
        let (outer, inner) = match f.split_once('.') {
            Some((o, i)) => (body.get_mut(o), i),
            None => (Some(&mut body), *f),
        };
        let Some(o) = outer.and_then(Value::as_object_mut) else { continue };
        if let Some(v) = o.get_mut(inner).filter(|v| !v.is_null()).map(Value::take) {
            o.remove(inner);
            taken.insert(f.to_string(), v);
        }
    }
    if taken.is_empty() {
        return (body, None);
    }
    let content = Content { salt: rand::random::<[u8; 32]>().to_vec(), body: Value::Object(taken) };
    // A Value always serializes, and its Display is the same compact JSON as `serde_json::to_vec`.
    let bytes = content.body.to_string().into_bytes();
    body["content"] = json!({ "sha256": hash(&content.salt, &bytes), "bytes": bytes.len() });
    (body, Some(content))
}

pub(super) fn hash(salt: &[u8], bytes: &[u8]) -> String {
    hex::encode(Sha256::new().chain_update(salt).chain_update(bytes).finalize())
}

/// An event's body as it was written: its record with its content put back.
/// Without its content (deleted), the record alone.
pub fn join(mut body: Value, content: Option<Value>) -> Value {
    let Some(Value::Object(c)) = content else { return body };
    if let Some(o) = body.as_object_mut() {
        o.remove("content");
    }
    for (f, v) in c {
        match f.split_once('.') {
            Some((outer, inner)) => {
                if let Some(o) = body.get_mut(outer).and_then(Value::as_object_mut) {
                    o.insert(inner.into(), v);
                }
            }
            None => body[f] = v,
        }
    }
    body
}
