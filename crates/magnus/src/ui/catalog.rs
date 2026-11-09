//! The catalog: which elements exist and how each is written. This table is
//! the one definition; the client compiles specs from it as served.
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

/// Goes up whenever the elements or the way they are written change.
pub const CATALOG_VERSION: u32 = 1;

#[derive(Serialize, Clone, Copy)]
pub struct Shape {
    /// The attribute that takes the words before any `key=value`.
    pub lead: Option<&'static str>,
    pub lists: &'static [&'static str],
    pub numbers: &'static [&'static str],
    pub number_lists: &'static [&'static str],
    /// Bare words that mean true.
    pub flags: &'static [&'static str],
    /// A number when it looks like one, otherwise text.
    pub loose: &'static [&'static str],
}

const PLAIN: Shape = Shape { lead: None, lists: &[], numbers: &[], number_lists: &[], flags: &[], loose: &[] };

/// How an element's indented lines are read.
#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Takes {
    /// `key=value` pairs become an object.
    Object,
    /// The whole line is one string.
    Text,
    /// The line is a comma-separated row of cells.
    Row,
}

#[derive(Serialize, Clone, Copy)]
pub struct Child {
    pub line: &'static str,
    pub into: &'static str,
    pub takes: Takes,
    pub shape: Shape,
}

#[derive(Serialize, Clone, Copy)]
pub struct Element {
    pub name: &'static str,
    pub doc: &'static str,
    pub shape: Shape,
    pub child: Option<Child>,
}

const fn element(name: &'static str, doc: &'static str, shape: Shape, child: Option<Child>) -> Element {
    Element { name, doc, shape, child }
}

pub const ELEMENTS: &[Element] = &[
    element("heading", "heading Text level=2", Shape { lead: Some("text"), numbers: &["level"], ..PLAIN }, None),
    element("text", "text A paragraph. Add muted to quiet it.", Shape { lead: Some("text"), flags: &["muted"], ..PLAIN }, None),
    element(
        "list",
        "list title=\"...\" [ordered], then one indented `item Words` per line.",
        Shape { flags: &["ordered"], ..PLAIN },
        Some(Child { line: "item", into: "items", takes: Takes::Text, shape: PLAIN }),
    ),
    element(
        "table",
        "table columns=Name,Score, then one indented `row Ada,3` per line.",
        Shape { lists: &["columns"], ..PLAIN },
        Some(Child { line: "row", into: "rows", takes: Takes::Row, shape: PLAIN }),
    ),
    element("stat", "stat label=Revenue value=41.7 unit=M", Shape { loose: &["value"], ..PLAIN }, None),
    element("bar_chart", "bar_chart title=\"...\" labels=North,South values=10,20 unit=M", Shape { lists: &["labels"], number_lists: &["values"], ..PLAIN }, None),
    element(
        "line_chart",
        "line_chart labels=Jan,Feb, then one indented `series Name values=1,2` per line.",
        Shape { lists: &["labels"], ..PLAIN },
        Some(Child { line: "series", into: "series", takes: Takes::Object, shape: Shape { lead: Some("name"), number_lists: &["values"], ..PLAIN } }),
    ),
    element(
        "map",
        "map, then one indented `place Paris lat=48.85 lon=2.35` per line.",
        PLAIN,
        Some(Child { line: "place", into: "places", takes: Takes::Object, shape: Shape { lead: Some("name"), numbers: &["lat", "lon"], ..PLAIN } }),
    ),
    element(
        "timeline",
        "timeline, then one indented `event when=2024 label=\"Launched\"` per line.",
        PLAIN,
        Some(Child { line: "event", into: "events", takes: Takes::Object, shape: PLAIN }),
    ),
    element(
        "comparison",
        "comparison options=A,B, then one indented `aspect Speed values=1,2 best=0` per line.",
        Shape { lists: &["options"], ..PLAIN },
        Some(Child { line: "aspect", into: "aspects", takes: Takes::Object, shape: Shape { lead: Some("name"), lists: &["values"], numbers: &["best"], ..PLAIN } }),
    ),
    element("code", "code language=rust code=\"fn main() {}\"", PLAIN, None),
    element("image", "image file=chart.png caption=\"...\", or image placeholder=ID for one still being made", PLAIN, None),
    element("file", "file file=report.md", PLAIN, None),
    element("action", "action label=\"Do it\" request=\"What to ask for\". A request for more work, never an approval.", PLAIN, None),
    element("card", "card title=\"...\" text=\"...\"", PLAIN, None),
    element(
        "gallery",
        "gallery, then one indented `item file=a.png caption=\"...\"` per line.",
        PLAIN,
        Some(Child { line: "item", into: "items", takes: Takes::Object, shape: PLAIN }),
    ),
    element(
        "form",
        "form title=\"...\" submit=Send request=\"What to do with the answers\", then one indented `field name=email label=Email type=email required` per line. Types: text, email, number, date, textarea, select (with options=a,b). A request for more work, never an approval.",
        PLAIN,
        Some(Child { line: "field", into: "fields", takes: Takes::Object, shape: Shape { lists: &["options"], flags: &["required"], ..PLAIN } }),
    ),
    element(
        "deck",
        "deck title=\"...\", then one indented `slide layout=bullets title=\"Wins\" items=One,Two` per line. Layouts: title (title, subtitle), bullets (title, items), text (title, text), quote (text, subtitle), image (title, file). Fill the slots; the template does the rest. Exports as PPTX and PDF.",
        PLAIN,
        Some(Child { line: "slide", into: "slides", takes: Takes::Object, shape: Shape { lists: &["items"], ..PLAIN } }),
    ),
    element("app", "app view=ID, for the view a connector's tool told you it has.", PLAIN, None),
];

pub fn element_named(name: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|e| e.name == name)
}

/// By person: the elements their client says it can draw.
static ADVERTISED: Mutex<Option<HashMap<String, BTreeSet<String>>>> = Mutex::new(None);

pub fn advertise(person: &str, elements: BTreeSet<String>) {
    ADVERTISED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(person.to_string(), elements);
}

/// Which elements a spec for this person may use: every one until their
/// client has said otherwise.
pub fn allowed(person: &str) -> Vec<&'static str> {
    let guard = ADVERTISED.lock().unwrap_or_else(|e| e.into_inner());
    let theirs = guard.as_ref().and_then(|m| m.get(person));
    ELEMENTS.iter().map(|e| e.name).filter(|n| theirs.is_none_or(|t| t.contains(*n))).collect()
}

pub fn as_json() -> Value {
    json!({ "version": CATALOG_VERSION, "elements": ELEMENTS, "html_csp": super::docs::HTML_CSP })
}

/// What the agent reads on demand: one element's doc, or all their names.
pub fn described(person: &str, element: Option<&str>) -> String {
    let names = allowed(person);
    match element {
        Some(name) => match element_named(name).filter(|e| names.contains(&e.name)) {
            Some(e) => e.doc.to_string(),
            None => format!("There is no element {name:?}. Elements: {}.", names.join(", ")),
        },
        None => format!("Catalog version {CATALOG_VERSION}. Elements: {}.", names.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client's compiler test runs from this copy of the table.
    #[test]
    fn the_clients_copy_of_the_catalog_is_current() {
        let copy: Value = serde_json::from_str(include_str!("../../../../desktop/tests/fixtures/ui-catalog.json")).unwrap();
        assert_eq!(copy["version"], json!(CATALOG_VERSION));
        assert_eq!(copy["elements"], json!(ELEMENTS), "regenerate desktop/tests/fixtures/ui-catalog.json");
    }
}
