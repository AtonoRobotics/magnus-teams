//! How concurrent changes to a co-edited file combine, by what can be
//! merged safely (the owner's design, Co-editing shared files): a text
//! document by paragraph, a sheet by cell and then its formulas
//! recalculated, structured records by record and field. A binary file
//! never merges. Each merge takes the version a change was made on (its
//! base), the change (`mine`) and the version at the head now (`theirs`),
//! and gives the merged file, or the parts both changed differently, each
//! with both versions for a person to decide.

use super::{records, sheet, text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which merge a file takes, told by its name.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Specs, proposals, statements of work: merged by paragraph.
    Text,
    /// Cost sheets, budgets (CSV): merged by cell, formulas recalculated.
    Sheet,
    /// Quote and invoice line items (JSON): merged by record and field.
    Records,
    /// CAD, drawings, firmware images, and anything else: never merged;
    /// one editor at a time under a check-out.
    #[default]
    Binary,
}

impl Format {
    pub fn of(name: &str) -> Format {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" | "txt" | "text" | "rst" | "adoc" | "org" | "tex" => Format::Text,
            "csv" | "tsv" => Format::Sheet,
            "json" => Format::Records,
            _ => Format::Binary,
        }
    }

    pub fn merges(self) -> bool {
        self != Format::Binary
    }

    /// Why `bytes` cannot be a file of this format, if so.
    pub fn refusal(self, name: &str, bytes: &[u8]) -> Option<String> {
        let text = || std::str::from_utf8(bytes).map_err(|_| format!("{name} is co-edited as text, and this is not UTF-8 text"));
        match self {
            Format::Binary => None,
            Format::Text | Format::Sheet => text().err(),
            Format::Records => match text() {
                Err(e) => Some(e),
                Ok(t) => serde_json::from_str::<serde_json::Value>(t).err().map(|e| format!("{name} is co-edited as records, and this is not JSON: {e}")),
            },
        }
    }
}

/// One part a change made: a paragraph, a cell or a field, as it was and
/// as it is after. A part added has no before; one removed has no after.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Edit {
    pub at: String,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
}

/// One part both sides changed differently: as the base had it, as the
/// change has it (`mine`) and as the file has it now (`theirs`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Conflict {
    pub at: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub mine: Option<String>,
    #[serde(default)]
    pub theirs: Option<String>,
}

/// A person's decision on one conflict.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Mine,
    Theirs,
}

/// Decisions, by the conflict's `at`.
pub type Choices = BTreeMap<String, Keep>;

/// What a merge came to.
#[derive(Debug, PartialEq)]
pub enum Merged {
    Clean(Vec<u8>),
    Conflicts(Vec<Conflict>),
}

/// What the whole of a binary file is called in an edit or a conflict.
pub const WHOLE_FILE: &str = "the whole file";

/// The three-way rule for one part: a side that left it as the base had
/// it takes the other side's; both made it the same, that; else `None`, a
/// conflict.
pub(super) fn pick<'a, T: PartialEq + ?Sized>(base: Option<&'a T>, mine: Option<&'a T>, theirs: Option<&'a T>) -> Option<Option<&'a T>> {
    if mine == theirs || theirs == base {
        Some(mine)
    } else if mine == base {
        Some(theirs)
    } else {
        None
    }
}

/// Merge `mine`, made on `base`, with `theirs`, the head now, applying
/// `choices` to what conflicts. Each must already be a file of `format`
/// (`Format::refusal`).
pub fn merge(format: Format, base: &[u8], mine: &[u8], theirs: &[u8], choices: &Choices) -> Merged {
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    match format {
        Format::Binary if theirs == base || mine == theirs => Merged::Clean(mine.to_vec()),
        Format::Binary => match choices.get(WHOLE_FILE) {
            Some(Keep::Mine) => Merged::Clean(mine.to_vec()),
            Some(Keep::Theirs) => Merged::Clean(theirs.to_vec()),
            None => Merged::Conflicts(vec![Conflict { at: WHOLE_FILE.into(), base: None, mine: Some(size(mine)), theirs: Some(size(theirs)) }]),
        },
        Format::Text => text::merge(&s(base), &s(mine), &s(theirs), choices),
        Format::Sheet => sheet::merge(&s(base), &s(mine), &s(theirs), choices),
        Format::Records => records::merge(&s(base), &s(mine), &s(theirs), choices),
    }
}

/// What `after` changed against `before`.
pub fn diff(format: Format, before: &[u8], after: &[u8]) -> Vec<Edit> {
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    match format {
        Format::Binary if before == after => vec![],
        Format::Binary => vec![Edit { at: WHOLE_FILE.into(), before: (!before.is_empty()).then(|| size(before)), after: Some(size(after)) }],
        Format::Text => text::diff(&s(before), &s(after)),
        Format::Sheet => sheet::diff(&s(before), &s(after)),
        Format::Records => records::diff(&s(before), &s(after)),
    }
}

/// A sheet's cells with every formula recalculated, as CSV; nothing for
/// any other format.
pub fn values(format: Format, bytes: &[u8]) -> Option<String> {
    (format == Format::Sheet).then(|| sheet::values(&String::from_utf8_lossy(bytes)))
}

fn size(bytes: &[u8]) -> String {
    format!("{} bytes", bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_merged_by_what_its_name_says_it_is() {
        assert_eq!(Format::of("Statement of work.md"), Format::Text);
        assert_eq!(Format::of("costs.CSV"), Format::Sheet);
        assert_eq!(Format::of("quote-114.json"), Format::Records);
        assert_eq!(Format::of("bracket.step"), Format::Binary);
        assert_eq!(Format::of("firmware"), Format::Binary);
        assert!(Format::Records.refusal("q.json", b"{nope").is_some());
        assert!(Format::Text.refusal("a.md", &[0xff, 0xfe]).is_some());
        assert!(Format::Binary.refusal("a.dwg", &[0xff]).is_none());
    }

    #[test]
    fn a_binary_file_never_merges() {
        let none = Choices::new();
        assert_eq!(merge(Format::Binary, b"v1", b"mine", b"v1", &none), Merged::Clean(b"mine".to_vec()), "on the head it simply lands");
        let Merged::Conflicts(c) = merge(Format::Binary, b"v1", b"mine", b"theirs", &none) else { panic!("a stale binary change merged") };
        assert_eq!(c[0].at, WHOLE_FILE);
        let kept = Choices::from([(WHOLE_FILE.to_string(), Keep::Theirs)]);
        assert_eq!(merge(Format::Binary, b"v1", b"mine", b"theirs", &kept), Merged::Clean(b"theirs".to_vec()));
    }
}
