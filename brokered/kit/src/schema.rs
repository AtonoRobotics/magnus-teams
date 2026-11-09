//! The schema check a brokered connector's tools run on both sides
//! (connectors spec, Brokered API connectors: "input and output schemas
//! validated on both sides"): the server on what it is given and what it
//! answers, the appliance on what it sends and what comes back. The part of
//! JSON Schema the servers' own schemas use: `type`, `properties`,
//! `required`, `additionalProperties: false`, `items` and `enum`.

use serde_json::Value;

/// Whether `v` matches `schema`: the first place it does not, when not.
pub fn check(schema: &Value, v: &Value) -> Result<(), String> {
    at(schema, v, "")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn at(schema: &Value, v: &Value, path: &str) -> Result<(), String> {
    let here = if path.is_empty() { "the value" } else { path };
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let k = kind(v);
        if !types.iter().any(|t| *t == k || (*t == "number" && k == "integer")) {
            return Err(format!("{here} is {k}, not {}", types.join(" or ")));
        }
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            return Err(format!("{here} is not one of {}", Value::Array(e.clone())));
        }
    }
    if let Value::Object(o) = v {
        for r in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if !o.contains_key(r) {
                return Err(format!("{path}/{r} is missing"));
            }
        }
        let props = schema.get("properties").and_then(Value::as_object);
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(s) => at(s, x, &format!("{path}/{k}"))?,
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => return Err(format!("{path}/{k} is not allowed")),
                None => {}
            }
        }
    }
    if let (Value::Array(a), Some(items)) = (v, schema.get("items")) {
        for (i, x) in a.iter().enumerate() {
            at(items, x, &format!("{path}/{i}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;
    use serde_json::json;

    #[test]
    fn a_value_is_checked_against_its_schema() {
        let s = json!({ "type": "object", "required": ["q"], "additionalProperties": false,
            "properties": { "q": { "type": "string" }, "n": { "type": "integer" }, "ids": { "type": "array", "items": { "type": "string" } }, "how": { "enum": ["a", "b"] } } });
        assert!(check(&s, &json!({ "q": "x", "n": 3, "ids": ["a"], "how": "a" })).is_ok());
        assert_eq!(check(&s, &json!({})).unwrap_err(), "/q is missing");
        assert_eq!(check(&s, &json!({ "q": 1 })).unwrap_err(), "/q is integer, not string");
        assert_eq!(check(&s, &json!({ "q": "x", "other": 1 })).unwrap_err(), "/other is not allowed");
        assert_eq!(check(&s, &json!({ "q": "x", "ids": ["a", 2] })).unwrap_err(), "/ids/1 is integer, not string");
        assert!(check(&s, &json!({ "q": "x", "how": "c" })).is_err());
        assert!(check(&json!({ "type": "number" }), &json!(2)).is_ok());
        assert!(check(&json!({ "type": ["string", "null"] }), &json!(null)).is_ok());
    }
}
