//! A text document's three-way merge, by paragraph: paragraphs are told
//! apart by blank lines. Each side's change is found as hunks against the
//! base (a run of base paragraphs replaced, or new ones put between two);
//! hunks of the two sides that touch different paragraphs both land, and
//! two that touch the same paragraph conflict unless they made it the same.
//! New paragraphs both sides put in the same place both land, the head's
//! first.

use super::merge::{Choices, Conflict, Edit, Keep, Merged};

/// How many paragraph pairs a diff compares, beyond the ends both sides
/// share. Past it the changed middle is one hunk, so a change to a very
/// long document still merges where its edges are untouched.
const MAX_DIFF_CELLS: usize = 4_000_000;

/// A paragraph: its lines as written (`raw`, line endings and the blank
/// lines after it included, so a merge gives back every byte it did not
/// change), and what it says (`key`: its lines without their endings),
/// which is what two versions are compared by.
#[derive(Clone, Debug)]
struct Para {
    key: String,
    raw: String,
}

impl PartialEq for Para {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A document's paragraphs: what lies between blank lines. The blank lines
/// before the first are its own (the first value).
fn split(s: &str) -> (String, Vec<Para>) {
    let (mut lead, mut out) = (String::new(), Vec::<Para>::new());
    let mut open = false;
    for line in s.split_inclusive('\n') {
        let text = line.strip_suffix('\n').map_or(line, |l| l.strip_suffix('\r').unwrap_or(l));
        match (blank(line), out.last_mut()) {
            (true, None) => lead.push_str(line),
            (true, Some(p)) => {
                p.raw.push_str(line);
                open = false;
            }
            (false, Some(p)) if open => {
                p.key.push('\n');
                p.key.push_str(text);
                p.raw.push_str(line);
            }
            (false, _) => {
                out.push(Para { key: text.to_string(), raw: line.to_string() });
                open = true;
            }
        }
    }
    (lead, out)
}

fn paragraphs(s: &str) -> Vec<Para> {
    split(s).1
}

/// The paragraphs written one after another, each set apart from the next
/// by a blank line where its own raw text has none (`nl`, the document's
/// line ending).
fn write(lead: &str, ps: &[Para], nl: &str) -> String {
    let mut text = lead.to_string();
    for (i, p) in ps.iter().enumerate() {
        text.push_str(&p.raw);
        if i + 1 < ps.len() && !p.raw.split_inclusive('\n').last().is_some_and(|l| l.ends_with('\n') && blank(l) && p.raw.lines().count() > 1) {
            if !p.raw.ends_with('\n') {
                text.push_str(nl);
            }
            text.push_str(nl);
        }
    }
    text
}

/// Which index pairs of `a` and `b` hold the same paragraph, in order: a
/// longest common subsequence, after the ends both share.
fn matched(a: &[Para], b: &[Para]) -> Vec<(usize, usize)> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let mut pairs: Vec<(usize, usize)> = (0..head).map(|i| (i, i)).collect();
    if !am.is_empty() && !bm.is_empty() && am.len().saturating_mul(bm.len()) <= MAX_DIFF_CELLS {
        // lengths[i][j]: the longest common run of am[i..] and bm[j..].
        let w = bm.len() + 1;
        let mut lengths = vec![0u32; (am.len() + 1) * w];
        for i in (0..am.len()).rev() {
            for j in (0..bm.len()).rev() {
                lengths[i * w + j] = if am[i] == bm[j] { lengths[(i + 1) * w + j + 1] + 1 } else { lengths[(i + 1) * w + j].max(lengths[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < am.len() && j < bm.len() {
            if am[i] == bm[j] {
                pairs.push((head + i, head + j));
                i += 1;
                j += 1;
            } else if lengths[(i + 1) * w + j] >= lengths[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    pairs.extend((0..tail).map(|t| (a.len() - tail + t, b.len() - tail + t)));
    pairs
}

/// Base paragraphs `start..end` become `with` (an empty range puts `with`
/// in before `start`).
#[derive(Clone, Debug, PartialEq)]
struct Hunk {
    start: usize,
    end: usize,
    with: Vec<Para>,
    mine: bool,
}

/// One side's change against the base, as hunks. A run replaced by as
/// many paragraphs is one hunk per paragraph, so a change to the next
/// paragraph is a different paragraph.
fn hunks(base: &[Para], side: &[Para], mine: bool) -> Vec<Hunk> {
    let mut out = vec![];
    let (mut bi, mut si) = (0, 0);
    for (i, j) in matched(base, side).into_iter().chain(std::iter::once((base.len(), side.len()))) {
        let (b, s) = (i - bi, j - si);
        if b == s {
            out.extend((0..b).filter(|t| base[bi + t] != side[si + t]).map(|t| Hunk { start: bi + t, end: bi + t + 1, with: vec![side[si + t].clone()], mine }));
        } else {
            out.push(Hunk { start: bi, end: i, with: side[si..j].to_vec(), mine });
        }
        (bi, si) = (i + 1, j + 1);
    }
    out
}

/// Whether two hunks touch the same paragraph: overlapping ranges, an
/// insertion strictly inside a replaced range, or two insertions at one
/// place.
fn touch(a: &Hunk, b: &Hunk) -> bool {
    match (a.start == a.end, b.start == b.end) {
        (true, true) => a.start == b.start,
        (true, false) => b.start < a.start && a.start < b.end,
        (false, true) => a.start < b.start && b.start < a.end,
        (false, false) => a.start < b.end && b.start < a.end,
    }
}

/// The base's paragraphs `start..end` with one side's hunks in them applied.
fn side_text(base: &[Para], start: usize, end: usize, hs: &[&Hunk]) -> Vec<Para> {
    let mut out = vec![];
    let mut pos = start;
    for h in hs {
        out.extend(base[pos..h.start.max(pos)].iter().cloned());
        out.extend(h.with.iter().cloned());
        pos = pos.max(h.end);
    }
    out.extend(base[pos.min(end)..end].iter().cloned());
    out
}

fn joined(ps: &[Para]) -> Option<String> {
    (!ps.is_empty()).then(|| ps.iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join("\n\n"))
}

/// The three-way merge of `mine` and `theirs`, both made on `base`.
pub(super) fn merge(base: &str, mine: &str, theirs: &str, choices: &Choices) -> Merged {
    let (lead, t) = split(theirs);
    let (mut b, m) = (paragraphs(base), paragraphs(mine));
    // A paragraph neither side changed is written as the head has it.
    for (i, j) in matched(&b, &t) {
        b[i].raw.clone_from(&t[j].raw);
    }
    let mut all: Vec<Hunk> = hunks(&b, &m, true).into_iter().chain(hunks(&b, &t, false)).collect();
    // Insertions before replacements at a place; the head's first.
    all.sort_by_key(|h| (h.start, h.end, h.mine));
    let mut groups: Vec<Vec<Hunk>> = vec![];
    for h in all {
        match groups.last_mut() {
            Some(g) if g.iter().any(|x| touch(x, &h)) => g.push(h),
            _ => groups.push(vec![h]),
        }
    }
    let (mut replaced, mut conflicts) = (vec![], vec![]);
    for g in groups {
        let (start, end) = (g.iter().map(|h| h.start).min().unwrap_or(0), g.iter().map(|h| h.end).max().unwrap_or(0));
        let (ms, ts): (Vec<&Hunk>, Vec<&Hunk>) = g.iter().partition(|h| h.mine);
        let (mt, tt) = (side_text(&b, start, end, &ms), side_text(&b, start, end, &ts));
        let with = if ms.is_empty() || ts.is_empty() || mt == tt {
            if ms.is_empty() {
                tt
            } else {
                mt
            }
        } else if start == end {
            // New paragraphs both put in one place: both land.
            tt.into_iter().chain(mt).collect()
        } else {
            let at = format!("paragraph {}", start + 1);
            match choices.get(&at) {
                Some(Keep::Mine) => mt,
                Some(Keep::Theirs) => tt,
                None => {
                    conflicts.push(Conflict { at, base: joined(&b[start..end]), mine: joined(&mt), theirs: joined(&tt) });
                    continue;
                }
            }
        };
        replaced.push((start, end, with));
    }
    if !conflicts.is_empty() {
        return Merged::Conflicts(conflicts);
    }
    let mut out: Vec<Para> = vec![];
    let mut pos = 0;
    for (start, end, with) in replaced {
        out.extend(b[pos..start.max(pos)].iter().cloned());
        out.extend(with);
        pos = pos.max(end);
    }
    out.extend(b[pos..].iter().cloned());
    let nl = if theirs.contains("\r\n") { "\r\n" } else { "\n" };
    Merged::Clean(write(&lead, &out, nl).into_bytes())
}

/// The paragraphs `after` changed against `before`.
pub(super) fn diff(before: &str, after: &str) -> Vec<Edit> {
    let (b, a) = (paragraphs(before), paragraphs(after));
    hunks(&b, &a, true).into_iter().map(|h| Edit { at: format!("paragraph {}", h.start + 1), before: joined(&b[h.start..h.end]), after: joined(&h.with) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "Scope: the east wing.\n\nPrice: 40,000.\n\nStart: March.\n";

    fn clean(m: Merged) -> String {
        match m {
            Merged::Clean(b) => String::from_utf8(b).unwrap(),
            Merged::Conflicts(c) => panic!("conflicted: {c:?}"),
        }
    }

    #[test]
    fn changes_to_different_paragraphs_both_land() {
        let mine = "Scope: the east wing.\n\nPrice: 42,500.\n\nStart: March.\n";
        let theirs = "Scope: the east and west wings.\n\nPrice: 40,000.\n\nStart: March.\n";
        let got = clean(merge(BASE, mine, theirs, &Choices::new()));
        assert_eq!(got, "Scope: the east and west wings.\n\nPrice: 42,500.\n\nStart: March.\n");
        // A third, next to both, lands too.
        let third = "Scope: the east wing.\n\nPrice: 40,000.\n\nStart: April.\n";
        assert_eq!(clean(merge(BASE, third, &got, &Choices::new())), "Scope: the east and west wings.\n\nPrice: 42,500.\n\nStart: April.\n");
    }

    #[test]
    fn the_same_paragraph_conflicts_with_both_versions() {
        let mine = "Scope: the east wing.\n\nPrice: 42,500.\n\nStart: March.\n";
        let theirs = "Scope: the east wing.\n\nPrice: 39,000.\n\nStart: March.\n";
        let Merged::Conflicts(c) = merge(BASE, mine, theirs, &Choices::new()) else { panic!("the same paragraph merged") };
        assert_eq!(c, vec![Conflict { at: "paragraph 2".into(), base: Some("Price: 40,000.".into()), mine: Some("Price: 42,500.".into()), theirs: Some("Price: 39,000.".into()) }]);
        let keep = Choices::from([("paragraph 2".to_string(), Keep::Mine)]);
        assert!(clean(merge(BASE, mine, theirs, &keep)).contains("42,500"));
    }

    #[test]
    fn new_paragraphs_in_one_place_both_land() {
        let mine = format!("{BASE}\nWarranty: one year.\n");
        let theirs = format!("{BASE}\nPayment: 30 days.\n");
        let got = clean(merge(BASE, &mine, &theirs, &Choices::new()));
        assert!(got.ends_with("Payment: 30 days.\n\nWarranty: one year.\n"), "{got}");
        assert_eq!(diff(BASE, &mine), vec![Edit { at: "paragraph 4".into(), before: None, after: Some("Warranty: one year.".into()) }]);
    }

    #[test]
    fn a_merge_keeps_every_byte_it_did_not_change() {
        // Line endings, runs of blank lines, and a blank line in a code block.
        let base = "Intro.\r\n\r\n```\r\nlet a = 1;\r\n\r\n\r\nlet b = 2;\r\n```\r\n\r\n\r\nEnd.\r\n";
        let mine = base.replace("Intro.", "Intro, again.");
        let theirs = base.replace("End.", "The end.");
        assert_eq!(clean(merge(base, &mine, &theirs, &Choices::new())), base.replace("Intro.", "Intro, again.").replace("End.", "The end."));
        // A paragraph put after the last, which had no line ending.
        let base = "One.\n\nTwo.";
        assert_eq!(clean(merge(base, "One!\n\nTwo.", "One.\n\nTwo.\n\nThree.", &Choices::new())), "One!\n\nTwo.\n\nThree.");
        assert_eq!(clean(merge(base, "One.\n\nTwo.\n\nThree.", "One.\n\nTwo, said twice.", &Choices::new())), "One.\n\nTwo, said twice.\n\nThree.");
    }

    #[test]
    fn a_paragraph_removed_and_changed_conflicts() {
        let mine = "Scope: the east wing.\n\nStart: March.\n";
        let theirs = "Scope: the east wing.\n\nPrice: 41,000.\n\nStart: March.\n";
        assert!(matches!(merge(BASE, mine, theirs, &Choices::new()), Merged::Conflicts(_)));
    }
}
