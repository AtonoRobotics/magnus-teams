//! The fixed components an agent may compose an outcome from (U8). The kernel
//! checks shape only; the app draws each component with its own code inside a
//! sandbox, so a composition can choose what to show but never how it runs.

use serde_json::Value;

pub const KINDS: &[&str] = &[
    "heading",
    "text",
    "list",
    "table",
    "stat",
    "bar_chart",
    "line_chart",
    "map",
    "timeline",
    "comparison",
    "code",
    "image",
    "file",
    "action",
    "card",
    "gallery",
    "app",
    "form",
    "deck",
];

/// What a chat answer may show beside its text: only components that carry
/// what prose cannot. Headings, paragraphs and lists belong in the answer
/// itself, and a chat turn delivers no files to show.
pub const SHOWN: &[&str] = &["table", "stat", "bar_chart", "line_chart", "map", "timeline", "comparison", "code"];

const MAX_COMPONENTS: usize = 40;
pub const MAX_TEXT: usize = 4000;
pub const MAX_ROWS: usize = 200;
/// A delivered file shown in full on the page holds at most as much text as
/// a composition may, and a table from it at most as many rows; the rest is
/// in the file.
pub const PREVIEW_CHARS: usize = MAX_COMPONENTS * MAX_TEXT;
const MAX_COLUMNS: usize = 12;
const MAX_POINTS: usize = 60;
const MAX_ITEMS: usize = 100;
/// A comparison sets 2 to this many options side by side.
const MAX_OPTIONS: usize = 6;
const MAX_FORM_FIELDS: usize = 12;
const MAX_SLIDES: usize = 60;
const SLIDE_LAYOUTS: &[&str] = &["title", "bullets", "text", "quote", "image"];
const FIELD_TYPES: &[&str] = &["text", "email", "number", "date", "textarea", "select"];
pub const MAX_SPOKEN: usize = 6000;

fn text(v: &Value, what: &str) -> Result<(), String> {
    let s = v.as_str().ok_or(format!("{what} must be a string"))?;
    if s.trim().is_empty() {
        return Err(format!("{what} is empty"));
    }
    if s.chars().count() > MAX_TEXT {
        return Err(format!("{what} is longer than {MAX_TEXT} characters"));
    }
    Ok(())
}

fn strings(v: &Value, what: &str, max: usize) -> Result<usize, String> {
    let a = v.as_array().ok_or(format!("{what} must be a list"))?;
    if a.is_empty() || a.len() > max {
        return Err(format!("{what} needs 1 to {max} entries"));
    }
    for (i, x) in a.iter().enumerate() {
        text(x, &format!("{what}[{i}]"))?;
    }
    Ok(a.len())
}

/// A number within a range, for a coordinate.
fn number_in(v: &Value, what: &str, lo: f64, hi: f64) -> Result<(), String> {
    match v.as_f64() {
        Some(x) if (lo..=hi).contains(&x) => Ok(()),
        _ => Err(format!("{what} must be a number from {lo} to {hi}")),
    }
}

/// A list of 1 to `max` objects.
fn objects<'a>(v: &'a Value, what: &str, max: usize) -> Result<&'a Vec<Value>, String> {
    let a = v.as_array().filter(|a| !a.is_empty() && a.len() <= max).ok_or(format!("{what} needs 1 to {max} entries"))?;
    if a.iter().any(|x| !x.is_object()) {
        return Err(format!("each of {what} must be an object"));
    }
    Ok(a)
}

fn numbers(v: &Value, what: &str, len: usize) -> Result<(), String> {
    let a = v.as_array().ok_or(format!("{what} must be a list of numbers"))?;
    if a.len() != len || a.iter().any(|x| !x.is_number()) {
        return Err(format!("{what} must hold {len} numbers, one per label"));
    }
    Ok(())
}

/// Check a composition against the fixed components. `files` are the names of
/// files the run delivered; an image must show one of them.
pub fn validate(view: &Value, files: &[String]) -> Result<(), String> {
    let parts = view.as_array().ok_or("the deliverable must be a list of components")?;
    if parts.is_empty() || parts.len() > MAX_COMPONENTS {
        return Err(format!("the deliverable needs 1 to {MAX_COMPONENTS} components"));
    }
    for (n, c) in parts.iter().enumerate() {
        let at = format!("component {n}");
        let kind = c["type"].as_str().ok_or(format!("{at} has no type"))?;
        match kind {
            "heading" | "text" => text(&c["text"], &format!("{at}.text"))?,
            // A button: a request for more work, never an approval.
            "action" => {
                text(&c["label"], &format!("{at}.label"))?;
                text(&c["request"], &format!("{at}.request"))?;
            }
            "card" => {
                text(&c["title"], &format!("{at}.title"))?;
                text(&c["text"], &format!("{at}.text"))?;
            }
            "gallery" => {
                for (i, g) in objects(&c["items"], &format!("{at}.items"), MAX_ITEMS)?.iter().enumerate() {
                    let f = g["file"].as_str().ok_or(format!("{at}.items[{i}].file must name a delivered file"))?;
                    if !files.iter().any(|x| x == f) {
                        return Err(format!("{at}.items[{i}].file {f:?} is not a file you delivered; deliver it with deliver_file first"));
                    }
                }
            }
            "deck" => {
                text(&c["title"], &format!("{at}.title"))?;
                for (i, s) in objects(&c["slides"], &format!("{at}.slides"), MAX_SLIDES)?.iter().enumerate() {
                    let layout = s["layout"].as_str().unwrap_or_default();
                    if !SLIDE_LAYOUTS.contains(&layout) {
                        return Err(format!("{at}.slides[{i}].layout must be one of {}", SLIDE_LAYOUTS.join(", ")));
                    }
                    if layout == "bullets" {
                        strings(&s["items"], &format!("{at}.slides[{i}].items"), MAX_ITEMS)?;
                    }
                }
            }
            // A form: its answers are a request for more work, never an approval.
            "form" => {
                text(&c["request"], &format!("{at}.request"))?;
                text(&c["submit"], &format!("{at}.submit"))?;
                for (i, f) in objects(&c["fields"], &format!("{at}.fields"), MAX_FORM_FIELDS)?.iter().enumerate() {
                    text(&f["name"], &format!("{at}.fields[{i}].name"))?;
                    let kind = f["type"].as_str().unwrap_or("text");
                    if !FIELD_TYPES.contains(&kind) {
                        return Err(format!("{at}.fields[{i}].type must be one of {}", FIELD_TYPES.join(", ")));
                    }
                    if kind == "select" {
                        strings(&f["options"], &format!("{at}.fields[{i}].options"), MAX_ITEMS)?;
                    }
                }
            }
            "list" => {
                strings(&c["items"], &format!("{at}.items"), MAX_ITEMS)?;
            }
            "table" => {
                // A header may be empty, as over a column of row labels.
                let cols = c["columns"]
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= MAX_COLUMNS && a.iter().all(|x| x.as_str().is_some_and(|s| s.chars().count() <= MAX_TEXT)))
                    .map(Vec::len)
                    .ok_or(format!("{at}.columns needs 1 to {MAX_COLUMNS} strings"))?;
                let rows = c["rows"].as_array().ok_or(format!("{at}.rows must be a list of rows"))?;
                if rows.len() > MAX_ROWS {
                    return Err(format!("{at} has more than {MAX_ROWS} rows; deliver a file instead"));
                }
                for (i, r) in rows.iter().enumerate() {
                    let cells = r.as_array().ok_or(format!("{at}.rows[{i}] must be a list"))?;
                    if cells.len() != cols || cells.iter().any(|x| !(x.is_string() || x.is_number())) {
                        return Err(format!("{at}.rows[{i}] must hold {cols} strings or numbers"));
                    }
                }
            }
            "stat" => {
                text(&c["label"], &format!("{at}.label"))?;
                if !(c["value"].is_string() || c["value"].is_number()) {
                    return Err(format!("{at}.value must be a string or a number"));
                }
            }
            "bar_chart" => {
                let n = strings(&c["labels"], &format!("{at}.labels"), MAX_POINTS)?;
                numbers(&c["values"], &format!("{at}.values"), n)?;
            }
            "line_chart" => {
                let n = strings(&c["labels"], &format!("{at}.labels"), MAX_POINTS)?;
                let series = c["series"].as_array().filter(|s| !s.is_empty() && s.len() <= 6).ok_or(format!("{at}.series needs 1 to 6 series"))?;
                for (i, s) in series.iter().enumerate() {
                    text(&s["name"], &format!("{at}.series[{i}].name"))?;
                    numbers(&s["values"], &format!("{at}.series[{i}].values"), n)?;
                }
            }
            // Places on a plain projection: the frame has no network, so no
            // map tiles.
            "map" => {
                for (i, p) in objects(&c["places"], &format!("{at}.places"), MAX_ITEMS)?.iter().enumerate() {
                    text(&p["name"], &format!("{at}.places[{i}].name"))?;
                    number_in(&p["lat"], &format!("{at}.places[{i}].lat"), -90.0, 90.0)?;
                    number_in(&p["lon"], &format!("{at}.places[{i}].lon"), -180.0, 180.0)?;
                }
            }
            "timeline" => {
                for (i, e) in objects(&c["events"], &format!("{at}.events"), MAX_ITEMS)?.iter().enumerate() {
                    text(&e["when"], &format!("{at}.events[{i}].when"))?;
                    text(&e["label"], &format!("{at}.events[{i}].label"))?;
                }
            }
            "comparison" => {
                let n = strings(&c["options"], &format!("{at}.options"), MAX_OPTIONS)?;
                if n < 2 {
                    return Err(format!("{at}.options needs 2 to {MAX_OPTIONS} entries"));
                }
                for (i, a) in objects(&c["aspects"], &format!("{at}.aspects"), MAX_ROWS)?.iter().enumerate() {
                    text(&a["name"], &format!("{at}.aspects[{i}].name"))?;
                    let vals = a["values"].as_array().filter(|v| v.len() == n && v.iter().all(|x| x.is_string() || x.is_number()));
                    if vals.is_none() {
                        return Err(format!("{at}.aspects[{i}].values must hold {n} strings or numbers, one per option"));
                    }
                    // No best is null, or a negative index from a model that fills every field.
                    let none = a["best"].is_null() || a["best"].as_i64().is_some_and(|b| b < 0);
                    if !none && !a["best"].as_u64().is_some_and(|b| (b as usize) < n) {
                        return Err(format!("{at}.aspects[{i}].best must be the index of an option, 0 to {}", n - 1));
                    }
                }
            }
            "code" => text(&c["code"], &format!("{at}.code"))?,
            // An image is drawn; a file is shown in full, drawn from the
            // same components by the app.
            "app" => text(&c["view"], &format!("{at}.view"))?,
            "image" if c["placeholder"].is_string() => text(&c["placeholder"], &format!("{at}.placeholder"))?,
            "image" | "file" => {
                let f = c["file"].as_str().ok_or(format!("{at}.file must name a delivered file"))?;
                if !files.iter().any(|x| x == f) {
                    return Err(format!("{at}.file {f:?} is not a file you delivered; deliver it with deliver_file first"));
                }
            }
            other => return Err(format!("{at} has unknown type {other:?}; use one of {}", KINDS.join(", "))),
        }
        // Optional, and an empty one is not given: some models fill every
        // field of the schema, leaving the ones they do not use empty.
        for t in ["title", "unit", "caption", "language"] {
            if !c[t].is_null() && !c[t].as_str().is_some_and(|s| s.trim().is_empty()) {
                text(&c[t], &format!("{at}.{t}"))?;
            }
        }
    }
    Ok(())
}

/// Check what a chat answer shows beside its text: the same components, but
/// only those that carry what the text cannot.
pub fn validate_shown(view: &Value) -> Result<(), String> {
    for (n, c) in view.as_array().into_iter().flatten().enumerate() {
        let kind = c["type"].as_str().unwrap_or_default();
        if KINDS.contains(&kind) && !SHOWN.contains(&kind) {
            return Err(format!("component {n} is a {kind}: say that in your answer; show only {}", SHOWN.join(", ")));
        }
    }
    validate(view, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn why(c: Value) -> String {
        validate(&json!([c]), &[]).err().unwrap_or_default()
    }

    #[test]
    fn the_new_components_are_checked() {
        assert_eq!(why(json!({ "type": "map", "places": [{ "name": "A", "lat": 1, "lon": 2 }] })), "");
        assert!(why(json!({ "type": "map", "places": [{ "name": "A", "lat": 1, "lon": 181 }] })).contains("lon must be a number from -180 to 180"));
        assert!(why(json!({ "type": "map", "places": [] })).contains("places needs 1 to"));
        assert_eq!(why(json!({ "type": "timeline", "events": [{ "when": "2024", "label": "x" }] })), "");
        assert!(why(json!({ "type": "timeline", "events": [{ "when": "2024" }] })).contains("label must be a string"));
        let cmp =
            |options: Value, values: Value, best: Value| json!({ "type": "comparison", "options": options, "aspects": [{ "name": "Price", "values": values, "best": best }] });
        assert_eq!(why(cmp(json!(["a", "b"]), json!([1, "two"]), json!(1))), "");
        assert!(why(cmp(json!(["a"]), json!([1]), Value::Null)).contains("options needs 2 to 6"));
        assert!(why(cmp(json!(["a", "b"]), json!([1]), Value::Null)).contains("must hold 2 strings or numbers"));
        assert!(why(cmp(json!(["a", "b"]), json!([1, 2]), json!(2))).contains("best must be the index of an option"));
        assert_eq!(why(json!({ "type": "code", "code": "fn main() {}", "language": "rust" })), "");
        assert!(why(json!({ "type": "code", "code": "" })).contains("code is empty"));
    }

    #[test]
    fn chat_shows_only_what_text_cannot_carry() {
        assert!(validate_shown(&json!([{ "type": "list", "items": ["a"] }])).unwrap_err().contains("say that in your answer"));
        assert!(validate_shown(&json!([{ "type": "image", "file": "x.png" }])).unwrap_err().contains("show only"));
        assert!(validate_shown(&json!([{ "type": "stat", "label": "Revenue", "value": 41.7 }])).is_ok());
        assert!(validate_shown(&json!([{ "type": "script", "text": "<script>" }])).unwrap_err().contains("unknown type"));
    }

    /// Codex fills every field, sending the optional ones it does not use as
    /// "" (seen on strix-alpha, 2026-10-29): each was refused, one per retry.
    #[test]
    fn an_empty_optional_field_is_not_given() {
        let blank = json!({ "type": "bar_chart", "labels": ["Q1", "Q2"], "values": [1, 2], "title": "", "unit": " ", "caption": "", "language": "" });
        assert_eq!(why(blank), "");
        assert!(why(json!({ "type": "bar_chart", "labels": ["Q1"], "values": [1], "title": 5 })).contains("title must be a string"));
        assert!(why(json!({ "type": "code", "code": "  " })).contains("code is empty"), "a required field may still not be empty");
        let cmp = json!({ "type": "comparison", "options": ["a", "b"], "aspects": [{ "name": "Price", "values": [1, 2], "best": -1 }] });
        assert_eq!(why(cmp), "", "best -1 means no best");
        assert_eq!(why(json!({ "type": "table", "columns": ["", "Q1"], "rows": [["Revenue", 1]] })), "", "a header over row labels may be empty");
        assert!(why(json!({ "type": "table", "columns": [], "rows": [] })).contains("columns needs 1 to"));
    }
}
