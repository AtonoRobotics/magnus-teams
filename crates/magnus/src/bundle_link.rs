//! Which bundle document a link in a body names. A link may carry a
//! fragment (`#section`), a query (`?v=1`) or percent-escapes, and still
//! name the document at its path: each place that asks what a link names
//! asks here, so a link to a document its reader may not see is hidden
//! however it is written (K2).

use percent_encoding::percent_decode_str;

/// The bundle path a link target names: without its fragment and query,
/// its percent-escapes decoded, when that is a path `bundle::place`
/// accepts. None for a source's URI or anything that is no bundle path.
pub fn link_path(target: &str) -> Option<String> {
    let bare = target.split(['#', '?']).next().unwrap_or_default();
    let path = percent_decode_str(bare).decode_utf8_lossy().into_owned();
    crate::bundle::place(&path).ok().map(|_| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::strip_hidden;

    #[test]
    fn a_link_names_its_document_whatever_follows_or_escapes_it() {
        for t in ["/people/finance.md", "/people/finance.md#pay", "/people/finance.md?v=1", "/people/finance.md?v=1#pay", "/people/fin%61nce.md", "/people%2Ffinance.md"] {
            assert_eq!(link_path(t).as_deref(), Some("/people/finance.md"), "{t}");
        }
        for t in ["https://example.com/people/finance.md", "/people/finance.txt", "/people/../finance.md", "#pay"] {
            assert_eq!(link_path(t), None, "{t}");
        }
    }

    /// A link to a hidden document is hidden with a section, a query or an
    /// escape on it: neither its name nor its path is shown.
    #[test]
    fn a_hidden_document_stays_hidden_behind_a_section_or_query() {
        let hidden = |t: &str| t == "/people/finance.md";
        for link in ["/people/finance.md#pay", "/people/finance.md?v=1", "/people/fin%61nce.md"] {
            let out = strip_hidden(&format!("Pay: [Carol](</people/x.md>) and [the payroll]({link}).\n\n- [salaries]({link})\n"), hidden);
            assert!(!out.contains("finance") && !out.contains("payroll") && !out.contains("salaries"), "{link} showed: {out}");
            assert!(out.contains("Carol"), "a visible link went too: {out}");
        }
    }
}
