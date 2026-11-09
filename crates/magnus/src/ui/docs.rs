//! Specs and HTML live as fenced blocks in what the agent writes. An edit is
//! a search and replace on one, and the current text of each is worked out
//! from the log.
use super::{catalog, spec};
use crate::state::{Run, State};
use serde_json::Value;
use std::collections::HashMap;

const UI_FENCE: &str = "magnus-ui";
const HTML_FENCE: &str = "magnus-html";
const MAX_HTML_BYTES: usize = 256 * 1024;
pub const EDIT_TOOL: &str = "ui_edit";
pub const SCREENSHOT_TOOL: &str = "html_screenshot";

/// Set first in every sandboxed document, so nothing later can loosen it.
pub const HTML_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:";

#[derive(Debug, Clone, PartialEq)]
pub struct Fence {
    pub html: bool,
    pub id: Option<String>,
    pub body: String,
    /// The byte range of the body in the text.
    pub range: std::ops::Range<usize>,
}

fn id_in(info: &str) -> Option<String> {
    info.split_whitespace().find_map(|w| w.strip_prefix("id=")).map(|i| i.trim_matches('"').to_string()).filter(|i| !i.is_empty())
}

/// Every closed spec or HTML block in a text, in order.
pub fn fences(text: &str) -> Vec<Fence> {
    let mut out = vec![];
    let mut at = 0;
    let mut open: Option<(bool, Option<String>, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let start = at;
        at += line.len();
        match &open {
            None => {
                let Some(info) = trimmed.strip_prefix("```") else { continue };
                let html = info.starts_with(HTML_FENCE);
                if html || info.starts_with(UI_FENCE) {
                    open = Some((html, id_in(info), at));
                }
            }
            Some((html, id, body)) if trimmed == "```" => {
                out.push(Fence { html: *html, id: id.clone(), body: text[*body..start].to_string(), range: *body..start });
                open = None;
            }
            Some(_) => {}
        }
    }
    out
}

/// Why a block cannot be drawn, if it cannot.
pub fn problem(f: &Fence, person: &str, files: &[String]) -> Option<String> {
    if f.html {
        return (f.body.len() > MAX_HTML_BYTES).then(|| format!("that HTML is over {MAX_HTML_BYTES} bytes"));
    }
    let compiled = spec::compile(&f.body, &catalog::allowed(person));
    compiled.and_then(|c| crate::compose::validate(&Value::Array(c), files)).err()
}

/// Why this reply's blocks cannot be drawn, if any cannot.
pub fn problems(text: &str, person: &str, files: &[String]) -> Option<String> {
    fences(text).iter().find_map(|f| problem(f, person, files))
}

fn written_in(run: &Run, out: &mut Vec<(i64, String, String)>) {
    for reply in &run.replies {
        for f in fences(&reply.text()) {
            if let Some(id) = f.id {
                out.push((reply.seq, id, f.body));
            }
        }
        for (call, name, _) in reply.calls() {
            if name != EDIT_TOOL {
                continue;
            }
            let Some(data) = run.results.get(&call).filter(|r| !r.is_error).and_then(|r| r.data.as_ref()) else { continue };
            if let (Some(id), Some(text)) = (data["id"].as_str(), data["text"].as_str()) {
                out.push((reply.seq, id.to_string(), text.to_string()));
            }
        }
    }
}

/// The latest text of each named block this person's work has written.
pub fn documents(s: &State, person: &str) -> HashMap<String, String> {
    let mut all = vec![];
    for run in s.runs.values().filter(|r| r.person == person) {
        written_in(run, &mut all);
    }
    all.sort_by_key(|(seq, _, _)| *seq);
    all.into_iter().map(|(_, id, text)| (id, text)).collect()
}

/// A text with each named block brought up to date.
pub fn current(text: &str, docs: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for f in fences(text) {
        let Some(latest) = f.id.as_ref().and_then(|id| docs.get(id)) else { continue };
        out.push_str(&text[at..f.range.start]);
        out.push_str(latest);
        at = f.range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// A summary with its named blocks brought up to date, for the thread.
pub fn current_text(s: &State, person: &str, text: &Option<String>) -> Option<String> {
    let t = text.as_ref()?;
    if !t.contains("```magnus-") {
        return Some(t.clone());
    }
    Some(current(t, &documents(s, person)))
}

/// One search and replace. `find` must be there exactly once.
pub fn edited(text: &str, find: &str, replace: &str) -> Result<String, String> {
    match text.matches(find).count() {
        _ if find.is_empty() => Err("find is empty".to_string()),
        1 => Ok(text.replacen(find, replace, 1)),
        0 => Err("find is not in that block".to_string()),
        n => Err(format!("find is there {n} times; include more around it")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_found_with_their_ids_and_only_when_closed() {
        let t = "before\n```magnus-ui id=q3\nstat label=a value=1\n```\nafter\n```rust\nno\n```\n```magnus-html\n<p>x</p>\n```\n```magnus-ui\nopen";
        let f = fences(t);
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!((f[0].id.as_deref(), f[0].html, f[0].body.as_str()), (Some("q3"), false, "stat label=a value=1\n"));
        assert!(f[1].html && f[1].id.is_none());
    }

    #[test]
    fn a_later_edit_replaces_the_block_where_it_was_written() {
        let t = "a\n```magnus-ui id=q\nold\n```\nb\n```magnus-ui\nkept\n```\n";
        let docs = HashMap::from([("q".to_string(), "new\n".to_string())]);
        assert_eq!(current(t, &docs), "a\n```magnus-ui id=q\nnew\n```\nb\n```magnus-ui\nkept\n```\n");
    }

    #[test]
    fn an_edit_must_name_one_place() {
        assert_eq!(edited("a b", "b", "c").unwrap(), "a c");
        assert!(edited("a a", "a", "c").unwrap_err().contains("2 times"));
        assert!(edited("a", "z", "c").is_err());
        assert!(edited("a", "", "c").is_err());
    }

    #[test]
    fn a_block_that_cannot_be_drawn_says_why() {
        assert!(problems("```magnus-ui\nmarquee hi\n```\n", "nobody", &[]).unwrap().contains("unknown element"));
        assert!(problems("```magnus-ui\nstat label=a value=1\n```\n", "nobody", &[]).is_none());
        assert!(problems("```magnus-html\n<p>\n```\n", "nobody", &[]).is_none());
    }
}
