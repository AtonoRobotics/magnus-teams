//! A sheet (CSV, or tab-separated) merged by cell: a cell is named as a
//! spreadsheet names it (`B4`), by its row and column. Two changes to
//! different cells both land; the same cell conflicts unless both made it
//! the same. A cell that starts with `=` is a formula, kept as written; the
//! sheet's values are worked out again from the merged cells, so a total
//! counts both changes. Rows are known by their place, so two people
//! inserting rows above the same cells at once see those cells conflict.

use super::merge::{pick, Choices, Conflict, Edit, Keep, Merged};
use std::collections::HashMap;

/// How many references deep a formula is followed before it is an error.
const MAX_FORMULA_NESTED_REFS: usize = 256;
/// How many cells a range in a formula may cover.
const MAX_RANGE_CELLS: usize = 100_000;

fn separator(s: &str) -> char {
    let first = s.lines().next().unwrap_or_default();
    if first.contains('\t') && !first.contains(',') {
        '\t'
    } else {
        ','
    }
}

/// The rows of a CSV, quotes understood.
fn parse(s: &str, sep: char) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell) = (vec![], vec![], String::new());
    let (mut quoted, mut chars) = (false, s.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') if cell.is_empty() => quoted = true,
            (false, c) if c == sep => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn write(rows: &[Vec<String>], sep: char) -> String {
    let quote = |c: &String| {
        if c.contains(sep) || c.contains('"') || c.contains('\n') {
            format!("\"{}\"", c.replace('"', "\"\""))
        } else {
            c.clone()
        }
    };
    rows.iter().map(|r| r.iter().map(quote).collect::<Vec<_>>().join(&sep.to_string()) + "\n").collect()
}

/// A cell's name: its column's letters, then its row from 1.
pub(super) fn name(row: usize, col: usize) -> String {
    let mut letters = vec![];
    let mut n = col + 1;
    while n > 0 {
        letters.push((b'A' + ((n - 1) % 26) as u8) as char);
        n = (n - 1) / 26;
    }
    format!("{}{}", letters.iter().rev().collect::<String>(), row + 1)
}

fn cell(rows: &[Vec<String>], r: usize, c: usize) -> &str {
    rows.get(r).and_then(|row| row.get(c)).map_or("", String::as_str)
}

/// Every cell of the three, each changed one taken by the three-way rule.
pub(super) fn merge(base: &str, mine: &str, theirs: &str, choices: &Choices) -> Merged {
    let sep = separator(theirs);
    let (b, m, t) = (parse(base, sep), parse(mine, sep), parse(theirs, sep));
    let rows = b.len().max(m.len()).max(t.len());
    let cols = [&b, &m, &t].iter().flat_map(|g| g.iter().map(Vec::len)).max().unwrap_or(0);
    let (mut out, mut conflicts) = (vec![], vec![]);
    for r in 0..rows {
        let mut row = vec![];
        for c in 0..cols {
            let (bc, mc, tc) = (cell(&b, r, c), cell(&m, r, c), cell(&t, r, c));
            let at = name(r, c);
            let got = match (pick(Some(bc), Some(mc), Some(tc)), choices.get(&at)) {
                (Some(v), _) => v.unwrap_or_default(),
                (None, Some(Keep::Mine)) => mc,
                (None, Some(Keep::Theirs)) => tc,
                (None, None) => {
                    let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
                    conflicts.push(Conflict { at, base: some(bc), mine: some(mc), theirs: some(tc) });
                    tc
                }
            };
            row.push(got.to_string());
        }
        out.push(row);
    }
    if !conflicts.is_empty() {
        return Merged::Conflicts(conflicts);
    }
    while out.last().is_some_and(|r| r.iter().all(String::is_empty)) {
        out.pop();
    }
    let width = [&m, &t].iter().flat_map(|g| g.iter().map(Vec::len)).max().unwrap_or(0);
    for r in &mut out {
        r.truncate(width);
    }
    Merged::Clean(write(&out, sep).into_bytes())
}

/// The cells `after` changed against `before`.
pub(super) fn diff(before: &str, after: &str) -> Vec<Edit> {
    let sep = separator(after);
    let (b, a) = (parse(before, sep), parse(after, sep));
    let rows = b.len().max(a.len());
    let cols = b.iter().chain(&a).map(Vec::len).max().unwrap_or(0);
    let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
    let mut out = vec![];
    for r in 0..rows {
        for c in 0..cols {
            let (x, y) = (cell(&b, r, c), cell(&a, r, c));
            if x != y {
                out.push(Edit { at: name(r, c), before: some(x), after: some(y) });
            }
        }
    }
    out
}

/// The sheet as its values: each formula worked out from the cells now.
pub(super) fn values(s: &str) -> String {
    let sep = separator(s);
    let rows = parse(s, sep);
    let mut sheet = Sheet { rows: &rows, done: HashMap::new() };
    let out: Vec<Vec<String>> = rows.iter().enumerate().map(|(r, row)| (0..row.len()).map(|c| sheet.shown(r, c)).collect()).collect();
    write(&out, sep)
}

/// A value, or the error a spreadsheet would show.
type Value = Result<f64, &'static str>;

struct Sheet<'a> {
    rows: &'a [Vec<String>],
    /// Worked out already: `None` while being worked out (a cycle).
    done: HashMap<(usize, usize), Option<Value>>,
}

impl Sheet<'_> {
    fn shown(&mut self, r: usize, c: usize) -> String {
        let raw = cell(self.rows, r, c);
        if !raw.starts_with('=') {
            return raw.to_string();
        }
        match self.value(r, c, 0) {
            Ok(v) if v.fract() == 0.0 && v.abs() < 1e15 => format!("{}", v as i64),
            Ok(v) => format!("{v}"),
            Err(e) => e.to_string(),
        }
    }

    /// A cell's number: a formula worked out, a number as written, an empty cell 0.
    fn value(&mut self, r: usize, c: usize, depth: usize) -> Value {
        let raw = cell(self.rows, r, c).trim();
        let Some(formula) = raw.strip_prefix('=') else {
            return if raw.is_empty() { Ok(0.0) } else { raw.replace(',', "").parse().map_err(|_| "#VALUE!") };
        };
        if depth > MAX_FORMULA_NESTED_REFS {
            return Err("#DEPTH!");
        }
        match self.done.get(&(r, c)) {
            Some(Some(v)) => return *v,
            Some(None) => return Err("#CYCLE!"),
            None => {}
        }
        self.done.insert((r, c), None);
        let tokens: Vec<char> = formula.chars().filter(|c| !c.is_whitespace()).collect();
        let mut p = Parser { t: &tokens, at: 0, depth };
        let v = p.expr(self).and_then(|v| if p.at == tokens.len() { Ok(v) } else { Err("#ERROR!") });
        self.done.insert((r, c), Some(v));
        v
    }
}

struct Parser<'a> {
    t: &'a [char],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.t.get(self.at).copied()
    }

    fn expr(&mut self, s: &mut Sheet) -> Value {
        let mut v = self.term(s)?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            let w = self.term(s)?;
            v = if op == '+' { v + w } else { v - w };
        }
        Ok(v)
    }

    fn term(&mut self, s: &mut Sheet) -> Value {
        let mut v = self.factor(s)?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            let w = self.factor(s)?;
            v = if op == '*' {
                v * w
            } else if w == 0.0 {
                return Err("#DIV/0!");
            } else {
                v / w
            };
        }
        Ok(v)
    }

    fn factor(&mut self, s: &mut Sheet) -> Value {
        match self.peek() {
            Some('-') => {
                self.at += 1;
                Ok(-self.factor(s)?)
            }
            Some('(') => {
                self.at += 1;
                let v = self.expr(s)?;
                self.eat(')')?;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.at += 1;
                }
                self.t[start..self.at].iter().collect::<String>().parse().map_err(|_| "#ERROR!")
            }
            Some(c) if c.is_ascii_alphabetic() || c == '$' => {
                let word = self.word();
                if self.peek() == Some('(') {
                    self.at += 1;
                    return self.call(&word.to_ascii_uppercase(), s);
                }
                let (r, c) = at(&word).ok_or("#REF!")?;
                s.value(r, c, self.depth + 1)
            }
            _ => Err("#ERROR!"),
        }
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '$') {
            self.at += 1;
        }
        self.t[start..self.at].iter().collect()
    }

    fn eat(&mut self, c: char) -> Result<(), &'static str> {
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err("#ERROR!")
        }
    }

    /// `SUM`, `AVERAGE`, `MIN`, `MAX` or `COUNT` of numbers and ranges.
    fn call(&mut self, f: &str, s: &mut Sheet) -> Value {
        let mut nums = vec![];
        loop {
            let start = self.at;
            let first = self.word();
            if self.peek() == Some(':') {
                self.at += 1;
                let last = self.word();
                let ((r1, c1), (r2, c2)) = (at(&first).ok_or("#REF!")?, at(&last).ok_or("#REF!")?);
                let (rs, cs) = (r1.min(r2)..=r1.max(r2), c1.min(c2)..=c1.max(c2));
                if rs.clone().count().saturating_mul(cs.clone().count()) > MAX_RANGE_CELLS {
                    return Err("#REF!");
                }
                for r in rs {
                    for c in cs.clone() {
                        // A range counts the numbers in it; words are left out.
                        if let Ok(v) = s.value(r, c, self.depth + 1) {
                            if !cell(s.rows, r, c).trim().is_empty() {
                                nums.push(v);
                            }
                        }
                    }
                }
            } else {
                self.at = start;
                nums.push(self.expr(s)?);
            }
            match self.peek() {
                Some(',') => self.at += 1,
                Some(')') => {
                    self.at += 1;
                    break;
                }
                _ => return Err("#ERROR!"),
            }
        }
        let n = nums.len() as f64;
        match f {
            "SUM" => Ok(nums.iter().sum()),
            "AVERAGE" if n > 0.0 => Ok(nums.iter().sum::<f64>() / n),
            "AVERAGE" => Err("#DIV/0!"),
            "MIN" => Ok(nums.iter().copied().fold(f64::INFINITY, f64::min)).map(|v| if v.is_finite() { v } else { 0.0 }),
            "MAX" => Ok(nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)).map(|v| if v.is_finite() { v } else { 0.0 }),
            "COUNT" => Ok(n),
            _ => Err("#NAME?"),
        }
    }
}

/// The row and column a cell name (`B4`, `$B$4`) means.
fn at(name: &str) -> Option<(usize, usize)> {
    let s = name.replace('$', "").to_ascii_uppercase();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let col = letters.chars().try_fold(0usize, |n, c| n.checked_mul(26)?.checked_add((c as u8 - b'A' + 1) as usize))? - 1;
    let row = digits.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "Item,Cost\nSteel,100\nLabour,200\nTotal,=SUM(B2:B3)\n";

    fn clean(m: Merged) -> String {
        match m {
            Merged::Clean(b) => String::from_utf8(b).unwrap(),
            Merged::Conflicts(c) => panic!("conflicted: {c:?}"),
        }
    }

    #[test]
    fn different_cells_both_land_and_the_total_counts_both() {
        let mine = "Item,Cost\nSteel,120\nLabour,200\nTotal,=SUM(B2:B3)\n";
        let theirs = "Item,Cost\nSteel,100\nLabour,250\nTotal,=SUM(B2:B3)\n";
        let got = clean(merge(BASE, mine, theirs, &Choices::new()));
        assert_eq!(got, "Item,Cost\nSteel,120\nLabour,250\nTotal,=SUM(B2:B3)\n", "the formula is kept as written");
        assert_eq!(values(&got), "Item,Cost\nSteel,120\nLabour,250\nTotal,370\n", "the total was not worked out again");
    }

    #[test]
    fn the_same_cell_conflicts() {
        let mine = "Item,Cost\nSteel,120\nLabour,200\nTotal,=SUM(B2:B3)\n";
        let theirs = "Item,Cost\nSteel,90\nLabour,200\nTotal,=SUM(B2:B3)\n";
        let Merged::Conflicts(c) = merge(BASE, mine, theirs, &Choices::new()) else { panic!("the same cell merged") };
        assert_eq!(c, vec![Conflict { at: "B2".into(), base: Some("100".into()), mine: Some("120".into()), theirs: Some("90".into()) }]);
        assert_eq!(diff(BASE, mine), vec![Edit { at: "B2".into(), before: Some("100".into()), after: Some("120".into()) }]);
    }

    #[test]
    fn formulas_work_out_as_a_spreadsheet_would() {
        assert_eq!(values("1,2\n=A1+B1*2,=(A1+B1)/2\n=A2-$A$1,=AVERAGE(A1:B1)\n=C9,=A5\n"), "1,2\n5,1.5\n4,1.5\n0,0\n");
        assert_eq!(values("=A1\n=B1/0\n=FOO(1)\n=A9+\n"), "#CYCLE!\n#DIV/0!\n#NAME?\n#ERROR!\n");
        assert_eq!(values("\"a, b\",\"=MAX(1,3)\"\n"), "\"a, b\",3\n", "a quoted cell is one cell");
        assert_eq!((name(0, 0), name(9, 27), at("AB10")), ("A1".to_string(), "AB10".to_string(), Some((9, 27))));
    }
}
