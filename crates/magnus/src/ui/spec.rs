//! Compiles the line-based format into the components the client draws.
//! The client has its own compiler, which works as text streams; the two
//! are held to one set of cases in `desktop/tests/fixtures/ui-spec.json`.
use super::catalog::{element_named, Child, Element, Shape, Takes};
use serde_json::{json, Map, Value};

/// One word of a line: a key and value, or a bare word. Values are the
/// comma-separated parts, with `listed` saying a comma was written.
#[derive(Debug, PartialEq)]
struct Word {
    key: Option<String>,
    parts: Vec<String>,
    /// For each part, whether it was quoted, which keeps it text.
    quoted: Vec<bool>,
}

fn words_of(line: &str) -> Result<Vec<Word>, String> {
    let mut out = vec![];
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
        if chars.peek().is_none() {
            return Ok(out);
        }
        let (mut key, mut parts, mut current, mut quoted, mut flags) = (None, vec![], String::new(), false, vec![]);
        while let Some(&c) = chars.peek() {
            match c {
                ' ' | '\t' => break,
                '"' => {
                    chars.next();
                    quoted = true;
                    loop {
                        match chars.next() {
                            None => return Err(format!("a quote is not closed in: {line}")),
                            Some('"') => break,
                            Some('\\') => current.push(match chars.next() {
                                Some('n') => '\n',
                                Some(other) => other,
                                None => return Err(format!("a quote is not closed in: {line}")),
                            }),
                            Some(other) => current.push(other),
                        }
                    }
                }
                '=' if key.is_none() && !quoted && !current.is_empty() && parts.is_empty() => {
                    chars.next();
                    key = Some(std::mem::take(&mut current));
                }
                ',' if key.is_some() || !parts.is_empty() || !current.is_empty() => {
                    chars.next();
                    parts.push(std::mem::take(&mut current));
                    flags.push(std::mem::take(&mut quoted));
                }
                other => {
                    chars.next();
                    current.push(other);
                }
            }
        }
        parts.push(current);
        flags.push(quoted);
        out.push(Word { key, parts, quoted: flags });
    }
}

fn number(raw: &str, what: &str) -> Result<Value, String> {
    let n: f64 = raw.trim().parse().map_err(|_| format!("{what} must be a number, not {raw:?}"))?;
    serde_json::Number::from_f64(n).map(Value::Number).ok_or(format!("{what} must be a finite number"))
}

fn loose(raw: &str, quoted: bool) -> Value {
    match raw.trim().parse::<f64>().ok().filter(|_| !quoted).and_then(serde_json::Number::from_f64) {
        Some(n) => Value::Number(n),
        None => Value::String(raw.to_string()),
    }
}

/// Reads a line's words into `into`, by the shape they are written in.
fn fill(shape: &Shape, words: &[Word], into: &mut Map<String, Value>) -> Result<(), String> {
    let mut lead: Vec<String> = vec![];
    for w in words {
        let Some(key) = &w.key else {
            let bare = w.parts.join(",");
            if !w.quoted.iter().any(|q| *q) && shape.flags.contains(&bare.as_str()) {
                into.insert(bare, Value::Bool(true));
            } else if shape.lead.is_some() {
                lead.push(bare);
            } else {
                return Err(format!("{bare:?} needs a name, as key=value"));
            }
            continue;
        };
        let k = key.as_str();
        let value = if shape.number_lists.contains(&k) {
            Value::Array(w.parts.iter().map(|p| number(p, k)).collect::<Result<_, _>>()?)
        } else if shape.lists.contains(&k) {
            Value::Array(w.parts.iter().zip(&w.quoted).map(|(p, q)| loose(p, *q)).collect())
        } else if shape.numbers.contains(&k) {
            number(&w.parts.join(","), k)?
        } else if shape.loose.contains(&k) {
            loose(&w.parts.join(","), w.quoted.iter().any(|q| *q))
        } else {
            Value::String(w.parts.join(","))
        };
        into.insert(k.to_string(), value);
    }
    if let (Some(name), false) = (shape.lead, lead.is_empty()) {
        into.insert(name.to_string(), Value::String(lead.join(" ")));
    }
    Ok(())
}

fn child_value(child: &Child, rest: &str, words: &[Word]) -> Result<Value, String> {
    match child.takes {
        Takes::Text => {
            let t = rest.trim();
            Ok(Value::String(t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t).to_string()))
        }
        Takes::Row => {
            let row = words_of(rest)?;
            let cells: Vec<Value> = row.iter().flat_map(|w| w.parts.iter().zip(&w.quoted).map(|(p, q)| loose(p, *q))).collect();
            Ok(Value::Array(cells))
        }
        Takes::Object => {
            let mut o = Map::new();
            fill(&child.shape, words, &mut o)?;
            Ok(Value::Object(o))
        }
    }
}

/// The components a spec describes. Only `allowed` elements are accepted.
pub fn compile(text: &str, allowed: &[&str]) -> Result<Vec<Value>, String> {
    let mut out: Vec<Value> = vec![];
    let mut open: Option<&'static Element> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let trimmed = line.trim_start();
        let (first, rest) = trimmed.split_once([' ', '\t']).unwrap_or((trimmed, ""));
        if line.len() != trimmed.len() {
            let (Some(element), Some(component)) = (open, out.last_mut()) else { return Err(format!("nothing to indent under: {trimmed}")) };
            let Some(child) = element.child.as_ref() else { return Err(format!("{} takes no indented lines", element.name)) };
            if first != child.line {
                return Err(format!("{} takes `{}` lines, not {first:?}", element.name, child.line));
            }
            let value = child_value(child, rest, &words_of(rest)?)?;
            component[child.into].as_array_mut().ok_or("a spec's list went wrong")?.push(value);
            continue;
        }
        let element = element_named(first).filter(|e| allowed.contains(&e.name)).ok_or(format!("unknown element {first:?}: use one of {}", allowed.join(", ")))?;
        let mut o = Map::new();
        o.insert("type".to_string(), json!(element.name));
        fill(&element.shape, &words_of(rest)?, &mut o)?;
        if let Some(c) = &element.child {
            o.insert(c.into.to_string(), json!([]));
        }
        out.push(Value::Object(o));
        open = Some(element);
    }
    if out.is_empty() {
        return Err("the spec is empty".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::catalog::ELEMENTS;
    use super::*;

    #[test]
    fn the_shared_cases_compile_as_written_down() {
        let every: Vec<&str> = ELEMENTS.iter().map(|e| e.name).collect();
        let cases: Vec<Value> = serde_json::from_str(include_str!("../../../../desktop/tests/fixtures/ui-spec.json")).unwrap();
        assert!(cases.len() >= 10);
        for case in cases {
            let spec = case["spec"].as_str().unwrap();
            match (compile(spec, &every), case["error"].as_str()) {
                (Ok(got), None) => assert_eq!(Value::Array(got), case["components"], "{spec}"),
                (Err(why), Some(wanted)) => assert!(why.contains(wanted), "{spec}: {why}"),
                (got, wanted) => panic!("{spec}: got {got:?}, wanted error {wanted:?}"),
            }
        }
    }

    #[test]
    fn an_element_the_client_cannot_draw_is_refused() {
        assert!(compile("stat label=a value=1", &["heading"]).unwrap_err().contains("unknown element"));
    }
}
