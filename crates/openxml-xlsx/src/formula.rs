//! Formula text utilities.

use crate::cell_ref::{MAX_COL, MAX_ROW, column_index, column_name};

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'$' || b >= 0x80
}

/// A reference token found in formula text.
struct Token {
    len: usize,
    text: String,
}

fn scan_letters(b: &[u8], mut i: usize) -> (usize, usize) {
    let start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    (start, i)
}

fn scan_digits(b: &[u8], mut i: usize) -> (usize, usize) {
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    (start, i)
}

fn shift_col(abs: bool, letters: &str, cols: i64) -> Option<String> {
    let col = i64::from(column_index(letters).ok()?);
    let new = if abs { col } else { col + cols };
    (1..=i64::from(MAX_COL))
        .contains(&new)
        .then(|| column_name(new as u32))
}

fn shift_row(abs: bool, digits: &str, rows: i64) -> Option<String> {
    let row: i64 = digits.parse().ok()?;
    if row < 1 || row > i64::from(MAX_ROW) {
        return None;
    }
    let new = if abs { row } else { row + rows };
    (1..=i64::from(MAX_ROW)).contains(&new).then(|| new.to_string())
}

/// Tries to read a cell reference (`$A$1`), column range (`A:$C`) or row
/// range (`1:$3`) at `i` and returns its shifted text.
fn reference_at(b: &[u8], i: usize, rows: i64, cols: i64) -> Option<Token> {
    let s = std::str::from_utf8(b).ok()?;
    let dollar = |j: usize| b.get(j) == Some(&b'$');
    // Cell reference or column range.
    let col_abs = dollar(i);
    let (ls, le) = scan_letters(b, i + usize::from(col_abs));
    if le > ls && le - ls <= 3 {
        let row_abs = dollar(le);
        let (ds, de) = scan_digits(b, le + usize::from(row_abs));
        if de > ds {
            // A1-style cell reference.
            let end = de;
            if b.get(end)
                .is_some_and(|&c| is_word_byte(c) || c == b'(' || c == b'!')
            {
                return None;
            }
            let text = match (
                shift_col(col_abs, &s[ls..le], cols),
                shift_row(row_abs, &s[ds..de], rows),
            ) {
                (Some(c), Some(r)) => {
                    format!(
                        "{}{c}{}{r}",
                        if col_abs { "$" } else { "" },
                        if row_abs { "$" } else { "" }
                    )
                }
                _ => "#REF!".into(),
            };
            return Some(Token { len: end - i, text });
        }
        if !row_abs && b.get(le) == Some(&b':') {
            // Column range such as A:C.
            let abs2 = dollar(le + 1);
            let (ls2, le2) = scan_letters(b, le + 1 + usize::from(abs2));
            if le2 > ls2 && le2 - ls2 <= 3 && !b.get(le2).is_some_and(|&c| is_word_byte(c) || c == b'(') {
                let text = match (
                    shift_col(col_abs, &s[ls..le], cols),
                    shift_col(abs2, &s[ls2..le2], cols),
                ) {
                    (Some(a), Some(z)) => format!(
                        "{}{a}:{}{z}",
                        if col_abs { "$" } else { "" },
                        if abs2 { "$" } else { "" }
                    ),
                    _ => "#REF!".into(),
                };
                return Some(Token { len: le2 - i, text });
            }
        }
        return None;
    }
    // Row range such as 1:3.
    let row_abs = dollar(i);
    let (ds, de) = scan_digits(b, i + usize::from(row_abs));
    if de > ds && b.get(de) == Some(&b':') {
        let abs2 = dollar(de + 1);
        let (ds2, de2) = scan_digits(b, de + 1 + usize::from(abs2));
        if de2 > ds2 && !b.get(de2).is_some_and(|&c| is_word_byte(c)) {
            let text = match (
                shift_row(row_abs, &s[ds..de], rows),
                shift_row(abs2, &s[ds2..de2], rows),
            ) {
                (Some(a), Some(z)) => {
                    format!(
                        "{}{a}:{}{z}",
                        if row_abs { "$" } else { "" },
                        if abs2 { "$" } else { "" }
                    )
                }
                _ => "#REF!".into(),
            };
            return Some(Token { len: de2 - i, text });
        }
    }
    None
}

/// Moves the relative references of an A1 formula by `rows` and `cols`,
/// as happens when a formula is copied (used to expand shared formulas,
/// ECMA-376 Part 1 §18.3.1.40). Absolute parts (`$A$1`) do not move; string
/// literals, quoted sheet names and structured references are left
/// untouched; references moved off the sheet become `#REF!`.
pub fn shift_formula(formula: &str, rows: i64, cols: i64) -> String {
    let b = formula.as_bytes();
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'"' | b'\'' => {
                // String literal or quoted sheet name: copy up to the closing quote
                // (a doubled quote is an escaped quote).
                let start = i;
                i += 1;
                while i < b.len() {
                    if b[i] == c {
                        if b.get(i + 1) == Some(&c) {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                out.push_str(&formula[start..i]);
            }
            b'[' => {
                // Structured reference / external workbook index: copy verbatim.
                let start = i;
                let mut depth = 0;
                while i < b.len() {
                    match b[i] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                out.push_str(&formula[start..i]);
            }
            _ => {
                let at_boundary = i == 0 || !is_word_byte(b[i - 1]);
                if at_boundary && (c == b'$' || c.is_ascii_alphanumeric()) {
                    if let Some(tok) = reference_at(b, i, rows, cols) {
                        out.push_str(&tok.text);
                        i += tok.len;
                        continue;
                    }
                    // Not a reference: copy the whole word.
                    let start = i;
                    while i < b.len() && is_word_byte(b[i]) {
                        i += 1;
                    }
                    out.push_str(&formula[start..i]);
                    continue;
                }
                let ch = formula[i..].chars().next().expect("in bounds");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

// ----- structural edits ------------------------------------------------------

/// Rows or columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Rows,
    Columns,
}

/// Insertion or deletion of `count` rows or columns starting at `at` (1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub axis: Axis,
    pub at: u32,
    pub count: u32,
    pub insert: bool,
}

impl Edit {
    fn limit(&self) -> u32 {
        match self.axis {
            Axis::Rows => MAX_ROW,
            Axis::Columns => MAX_COL,
        }
    }

    /// New number of row/column `i`; `None` if it is deleted or pushed off the sheet.
    pub fn map(&self, i: u32) -> Option<u32> {
        if self.insert {
            if i < self.at {
                return Some(i);
            }
            let n = i.checked_add(self.count)?;
            (n <= self.limit()).then_some(n)
        } else {
            let end = self.at + self.count - 1;
            if i < self.at {
                Some(i)
            } else if i > end {
                Some(i - self.count)
            } else {
                None
            }
        }
    }

    /// New bounds of the span `a..=b`: spans grow when rows are inserted
    /// inside them and shrink when rows inside are deleted; `None` when the
    /// whole span is deleted.
    pub fn map_span(&self, a: u32, b: u32) -> Option<(u32, u32)> {
        if self.insert {
            let a2 = self.map(a)?;
            let b2 = if b >= self.at {
                b.saturating_add(self.count).min(self.limit())
            } else {
                b
            };
            Some((a2, b2))
        } else {
            let end = self.at + self.count - 1;
            let a2 = if a < self.at {
                a
            } else if a > end {
                a - self.count
            } else {
                self.at
            };
            let b2 = if b < self.at {
                b
            } else if b > end {
                b - self.count
            } else {
                self.at.checked_sub(1)?
            };
            (a2 <= b2).then_some((a2, b2))
        }
    }
}

/// A parsed A1 reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ref {
    /// `$A$1`: (col, col_abs, row, row_abs).
    Cell(u32, bool, u32, bool),
    /// `A1:B2`.
    Area((u32, bool, u32, bool), (u32, bool, u32, bool)),
    /// `A:C`: (col, abs, col, abs).
    Columns(u32, bool, u32, bool),
    /// `1:3`: (row, abs, row, abs).
    Rows(u32, bool, u32, bool),
}

fn dollar(abs: bool) -> &'static str {
    if abs { "$" } else { "" }
}

impl Ref {
    fn text(&self) -> String {
        let cell = |(c, ca, r, ra): (u32, bool, u32, bool)| {
            format!("{}{}{}{r}", dollar(ca), column_name(c), dollar(ra))
        };
        match *self {
            Ref::Cell(c, ca, r, ra) => cell((c, ca, r, ra)),
            Ref::Area(a, b) => format!("{}:{}", cell(a), cell(b)),
            Ref::Columns(a, aa, b, ba) => format!(
                "{}{}:{}{}",
                dollar(aa),
                column_name(a),
                dollar(ba),
                column_name(b)
            ),
            Ref::Rows(a, aa, b, ba) => format!("{}{a}:{}{b}", dollar(aa), dollar(ba)),
        }
    }

    /// The reference after a structural edit; `None` when it becomes `#REF!`.
    pub fn apply(&self, e: &Edit) -> Option<Ref> {
        let rows = e.axis == Axis::Rows;
        Some(match *self {
            Ref::Cell(c, ca, r, ra) => {
                if rows {
                    Ref::Cell(c, ca, e.map(r)?, ra)
                } else {
                    Ref::Cell(e.map(c)?, ca, r, ra)
                }
            }
            Ref::Area((c1, ca1, r1, ra1), (c2, ca2, r2, ra2)) => {
                if rows {
                    let (a, b) = e.map_span(r1.min(r2), r1.max(r2))?;
                    Ref::Area((c1, ca1, a, ra1), (c2, ca2, b, ra2))
                } else {
                    let (a, b) = e.map_span(c1.min(c2), c1.max(c2))?;
                    Ref::Area((a, ca1, r1, ra1), (b, ca2, r2, ra2))
                }
            }
            Ref::Columns(a, aa, b, ba) if !rows => {
                let (a2, b2) = e.map_span(a.min(b), a.max(b))?;
                Ref::Columns(a2, aa, b2, ba)
            }
            Ref::Rows(a, aa, b, ba) if rows => {
                let (a2, b2) = e.map_span(a.min(b), a.max(b))?;
                Ref::Rows(a2, aa, b2, ba)
            }
            other => other,
        })
    }
}

/// A piece of formula text.
enum Segment<'a> {
    Text(&'a str),
    /// A (possibly sheet-qualified) name or reference.
    Ref {
        /// The prefix as written, including `!`.
        prefix: Option<&'a str>,
        /// The unquoted sheet name of the prefix.
        sheet: Option<String>,
        /// The prefix belongs to an external or 3-D reference.
        foreign: bool,
        /// The text after the prefix.
        text: &'a str,
        /// The parsed reference (`None` for names such as `Sheet1!Total`).
        parsed: Option<Ref>,
    },
}

fn is_name_byte(b: u8) -> bool {
    is_word_byte(b) || b == b'\\'
}

/// Parses `$?COL$?ROW` at `i`: (col, col_abs, row, row_abs, end).
fn parse_cell(b: &[u8], i: usize) -> Option<(u32, bool, u32, bool, usize)> {
    let s = std::str::from_utf8(b).ok()?;
    let col_abs = b.get(i) == Some(&b'$');
    let (ls, le) = scan_letters(b, i + usize::from(col_abs));
    if le == ls || le - ls > 3 {
        return None;
    }
    let row_abs = b.get(le) == Some(&b'$');
    let (ds, de) = scan_digits(b, le + usize::from(row_abs));
    if de == ds {
        return None;
    }
    let col = column_index(&s[ls..le]).ok()?;
    let row: u32 = s[ds..de].parse().ok()?;
    if !(1..=MAX_ROW).contains(&row) {
        return None;
    }
    Some((col, col_abs, row, row_abs, de))
}

/// Parses a reference at `i`; returns it and its end. The reference must
/// end at a boundary.
fn parse_ref(b: &[u8], i: usize) -> Option<(Ref, usize)> {
    let s = std::str::from_utf8(b).ok()?;
    let ends_ok = |end: usize| {
        !b.get(end)
            .is_some_and(|&c| is_name_byte(c) || c == b'(' || c == b'!')
    };
    if let Some((c, ca, r, ra, end)) = parse_cell(b, i) {
        if b.get(end) == Some(&b':')
            && let Some((c2, ca2, r2, ra2, end2)) = parse_cell(b, end + 1)
            && ends_ok(end2)
        {
            return Some((Ref::Area((c, ca, r, ra), (c2, ca2, r2, ra2)), end2));
        }
        return ends_ok(end).then_some((Ref::Cell(c, ca, r, ra), end));
    }
    // Column range A:C.
    let abs1 = b.get(i) == Some(&b'$');
    let (ls, le) = scan_letters(b, i + usize::from(abs1));
    if le > ls && le - ls <= 3 && b.get(le) == Some(&b':') {
        let abs2 = b.get(le + 1) == Some(&b'$');
        let (ls2, le2) = scan_letters(b, le + 1 + usize::from(abs2));
        if le2 > ls2 && le2 - ls2 <= 3 && ends_ok(le2) {
            let a = column_index(&s[ls..le]).ok()?;
            let z = column_index(&s[ls2..le2]).ok()?;
            return Some((Ref::Columns(a, abs1, z, abs2), le2));
        }
    }
    // Row range 1:3.
    let (ds, de) = scan_digits(b, i + usize::from(abs1));
    if de > ds && b.get(de) == Some(&b':') {
        let abs2 = b.get(de + 1) == Some(&b'$');
        let (ds2, de2) = scan_digits(b, de + 1 + usize::from(abs2));
        if de2 > ds2 && ends_ok(de2) {
            let a: u32 = s[ds..de].parse().ok()?;
            let z: u32 = s[ds2..de2].parse().ok()?;
            if (1..=MAX_ROW).contains(&a) && (1..=MAX_ROW).contains(&z) {
                return Some((Ref::Rows(a, abs1, z, abs2), de2));
            }
        }
    }
    None
}

/// End of the quoted token starting at `i` (index after the closing quote).
fn quoted_end(b: &[u8], i: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        if b[j] == q {
            if b.get(j + 1) == Some(&q) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// Splits formula text into literal text and references.
fn segments<'a>(formula: &'a str) -> Vec<Segment<'a>> {
    let b = formula.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    let flush = |out: &mut Vec<Segment<'a>>, from: usize, to: usize| {
        if to > from {
            out.push(Segment::Text(&formula[from..to]));
        }
    };
    while i < b.len() {
        let c = b[i];
        let boundary = i == 0 || !is_name_byte(b[i - 1]);
        if c == b'"' {
            i = quoted_end(b, i);
            continue;
        }
        if c == b'[' {
            let mut depth = 0;
            while i < b.len() {
                match b[i] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    b'\'' if depth > 0 => {
                        // An escape inside a structured reference.
                        i += 1;
                    }
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        let foreign = i > 0 && (b[i - 1] == b']' || b[i - 1] == b':');
        // Sheet prefix: 'Quoted Name'! or Name!
        let prefix_end = if c == b'\'' {
            let end = quoted_end(b, i);
            (b.get(end) == Some(&b'!')).then_some(end + 1)
        } else if boundary && c != b'$' && is_name_byte(c) {
            let mut j = i;
            while j < b.len() && is_name_byte(b[j]) {
                j += 1;
            }
            (b.get(j) == Some(&b'!')).then_some(j + 1)
        } else {
            None
        };
        if let Some(pe) = prefix_end {
            let raw = &formula[i..pe];
            let name = &raw[..raw.len() - 1];
            let sheet = if let Some(q) = name.strip_prefix('\'') {
                q.strip_suffix('\'').unwrap_or(q).replace("''", "'")
            } else {
                name.to_owned()
            };
            let (parsed, end) = match parse_ref(b, pe) {
                Some((r, end)) => (Some(r), end),
                None => {
                    let mut j = pe;
                    if b.get(j) == Some(&b'#') {
                        // Sheet1!#REF!
                        j = formula[j..].find('!').map_or(b.len(), |k| j + k + 1);
                    } else {
                        while j < b.len() && is_name_byte(b[j]) {
                            j += 1;
                        }
                    }
                    (None, j)
                }
            };
            flush(&mut out, text_start, i);
            out.push(Segment::Ref {
                prefix: Some(raw),
                sheet: Some(sheet),
                foreign,
                text: &formula[pe..end],
                parsed,
            });
            i = end;
            text_start = i;
            continue;
        }
        if boundary && (c == b'$' || c.is_ascii_alphanumeric()) {
            if let Some((r, end)) = parse_ref(b, i) {
                flush(&mut out, text_start, i);
                out.push(Segment::Ref {
                    prefix: None,
                    sheet: None,
                    foreign,
                    text: &formula[i..end],
                    parsed: Some(r),
                });
                i = end;
                text_start = i;
                continue;
            }
            while i < b.len() && is_name_byte(b[i]) {
                i += 1;
            }
            continue;
        }
        i += formula[i..].chars().next().map_or(1, char::len_utf8);
    }
    flush(&mut out, text_start, b.len());
    out
}

/// Applies a row/column insertion or deletion on sheet `target` to the
/// references of a formula found on sheet `host` (`None` for defined
/// names, whose references are always qualified). References into deleted
/// cells become `#REF!`.
pub(crate) fn adjust_formula(formula: &str, host: Option<&str>, target: &str, edit: &Edit) -> String {
    let mut out = String::with_capacity(formula.len() + 8);
    for seg in segments(formula) {
        match seg {
            Segment::Text(t) => out.push_str(t),
            Segment::Ref {
                prefix,
                sheet,
                foreign,
                text,
                parsed,
            } => {
                out.push_str(prefix.unwrap_or(""));
                let on_target = !foreign
                    && sheet
                        .as_deref()
                        .or(host)
                        .is_some_and(|s| s.eq_ignore_ascii_case(target));
                match parsed {
                    Some(r) if on_target => match r.apply(edit) {
                        Some(new) => out.push_str(&new.text()),
                        None => out.push_str("#REF!"),
                    },
                    _ => out.push_str(text),
                }
            }
        }
    }
    out
}

/// Replaces the sheet name `old` in the references of a formula with
/// `new` (quoted as needed), or with `#REF!` references when `new` is `None`.
pub(crate) fn rename_sheet_in_formula(formula: &str, old: &str, new: Option<&str>) -> String {
    let mut out = String::with_capacity(formula.len() + 8);
    for seg in segments(formula) {
        match seg {
            Segment::Text(t) => out.push_str(t),
            Segment::Ref {
                prefix,
                sheet,
                foreign,
                text,
                ..
            } => {
                let hit = !foreign && sheet.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(old));
                match (hit, new) {
                    (true, Some(n)) => {
                        out.push_str(&crate::util::quote_sheet(n));
                        out.push('!');
                        out.push_str(text);
                    }
                    (true, None) => out.push_str("#REF!"),
                    _ => {
                        out.push_str(prefix.unwrap_or(""));
                        out.push_str(text);
                    }
                }
            }
        }
    }
    out
}

/// Whether a formula may refer to sheet `name` (a cheap pre-check).
pub(crate) fn mentions_sheet(formula: &str, name: &str) -> bool {
    formula.to_lowercase().contains(&name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(at: u32, count: u32, insert: bool) -> Edit {
        Edit {
            axis: Axis::Rows,
            at,
            count,
            insert,
        }
    }

    fn cols(at: u32, count: u32, insert: bool) -> Edit {
        Edit {
            axis: Axis::Columns,
            at,
            count,
            insert,
        }
    }

    #[test]
    fn edits_map_lines_and_spans() {
        let ins = rows(5, 2, true);
        assert_eq!(ins.map(4), Some(4));
        assert_eq!(ins.map(5), Some(7));
        assert_eq!(
            ins.map_span(1, 10),
            Some((1, 12)),
            "a span around the insertion grows"
        );
        assert_eq!(ins.map_span(1, 4), Some((1, 4)));
        assert_eq!(ins.map(MAX_ROW), None, "pushed off the sheet");
        let del = rows(5, 2, false);
        assert_eq!(del.map(5), None);
        assert_eq!(del.map(6), None);
        assert_eq!(del.map(7), Some(5));
        assert_eq!(del.map_span(1, 10), Some((1, 8)));
        assert_eq!(del.map_span(5, 6), None);
        assert_eq!(del.map_span(6, 9), Some((5, 7)));
        assert_eq!(del.map_span(2, 5), Some((2, 4)));
        assert_eq!(rows(1, 1, false).map_span(1, 1), None);
    }

    #[test]
    fn formulas_follow_inserted_and_deleted_rows() {
        let ins = rows(3, 2, true);
        assert_eq!(
            adjust_formula("A1+A3+$B$4*C10", Some("S"), "S", &ins),
            "A1+A5+$B$6*C12"
        );
        assert_eq!(adjust_formula("SUM(A1:A10)", Some("S"), "S", &ins), "SUM(A1:A12)");
        assert_eq!(
            adjust_formula("SUM(3:4)+SUM(A:B)", Some("S"), "S", &ins),
            "SUM(5:6)+SUM(A:B)"
        );
        assert_eq!(adjust_formula("Other!A5+A5", Some("S"), "S", &ins), "Other!A5+A7");
        assert_eq!(adjust_formula("S!A5+A5", Some("Other"), "S", &ins), "S!A7+A5");
        assert_eq!(
            adjust_formula("'S'!A5", None, "s", &ins),
            "'S'!A7",
            "sheet names are case-insensitive"
        );
        assert_eq!(
            adjust_formula("A5", None, "S", &ins),
            "A5",
            "unqualified names are not on any sheet"
        );
        let del = rows(3, 2, false);
        assert_eq!(adjust_formula("A1+A3+A5", Some("S"), "S", &del), "A1+#REF!+A3");
        assert_eq!(adjust_formula("SUM(A2:A6)", Some("S"), "S", &del), "SUM(A2:A4)");
        assert_eq!(adjust_formula("SUM(A3:A4)", Some("S"), "S", &del), "SUM(#REF!)");
        assert_eq!(adjust_formula("S!A4", None, "S", &del), "S!#REF!");
    }

    #[test]
    fn formulas_follow_columns() {
        let ins = cols(2, 1, true);
        assert_eq!(adjust_formula("A1+B1+$C$1", Some("S"), "S", &ins), "A1+C1+$D$1");
        assert_eq!(
            adjust_formula("SUM(A:C)+SUM(1:2)", Some("S"), "S", &ins),
            "SUM(A:D)+SUM(1:2)"
        );
        let del = cols(1, 1, false);
        assert_eq!(adjust_formula("A1+B1", Some("S"), "S", &del), "#REF!+A1");
        assert_eq!(adjust_formula("SUM(A1:C1)", Some("S"), "S", &del), "SUM(A1:B1)");
    }

    #[test]
    fn scanner_skips_non_references() {
        let e = rows(1, 1, true);
        assert_eq!(
            adjust_formula(
                "\"A1\"&LOG10(A1)&Table1[[#This Row],[A1]]&MyName&TRUE",
                Some("S"),
                "S",
                &e
            ),
            "\"A1\"&LOG10(A2)&Table1[[#This Row],[A1]]&MyName&TRUE"
        );
        assert_eq!(
            adjust_formula("[1]S!A1+Sheet1:S!A1", Some("X"), "S", &e),
            "[1]S!A1+Sheet1:S!A1",
            "external and 3-D references"
        );
        assert_eq!(adjust_formula("3.5E+3*A1", Some("S"), "S", &e), "3.5E+3*A2");
        assert_eq!(
            adjust_formula("S!Total+S!#REF!", None, "S", &e),
            "S!Total+S!#REF!"
        );
        assert_eq!(adjust_formula("'It''s'!B2", None, "It's", &e), "'It''s'!B3");
        assert_eq!(
            adjust_formula("_xlfn.CONCAT(A1,Données!B1)", Some("Données"), "Données", &e),
            "_xlfn.CONCAT(A2,Données!B2)"
        );
    }

    #[test]
    fn sheet_renames() {
        assert_eq!(
            rename_sheet_in_formula("Old!A1+old!B2+Other!C3", "Old", Some("New Name")),
            "'New Name'!A1+'New Name'!B2+Other!C3"
        );
        assert_eq!(
            rename_sheet_in_formula("'Old'!A1:B2+\"Old!A1\"", "Old", Some("X")),
            "X!A1:B2+\"Old!A1\""
        );
        assert_eq!(
            rename_sheet_in_formula("SUM(Old!A1:B2)", "Old", None),
            "SUM(#REF!)"
        );
        assert!(mentions_sheet("'my sheet'!A1", "My Sheet"));
    }

    #[test]
    fn relative_and_absolute_references() {
        assert_eq!(shift_formula("A1+B$2*$C3+$D$4", 1, 1), "B2+C$2*$C4+$D$4");
        assert_eq!(shift_formula("SUM(A1:A10)", 2, 0), "SUM(A3:A12)");
        assert_eq!(shift_formula("a1*2", 0, 1), "B1*2", "lower-case references");
        assert_eq!(shift_formula("A1", 0, 0), "A1");
    }

    #[test]
    fn sheet_qualified_references() {
        assert_eq!(shift_formula("Sheet2!A1", 1, 1), "Sheet2!B2");
        assert_eq!(
            shift_formula("'My Sheet'!A1+'It''s'!B2", 1, 0),
            "'My Sheet'!A2+'It''s'!B3"
        );
        assert_eq!(
            shift_formula("AB1!C3", 1, 0),
            "AB1!C4",
            "sheet named like a reference"
        );
    }

    #[test]
    fn non_references_are_untouched() {
        assert_eq!(shift_formula("\"A1\"&A1", 1, 0), "\"A1\"&A2");
        assert_eq!(
            shift_formula("\"say \"\"B2\"\"\"&B2", 1, 0),
            "\"say \"\"B2\"\"\"&B3"
        );
        assert_eq!(
            shift_formula("LOG10(A1)+ATAN2(1,2)", 1, 0),
            "LOG10(A2)+ATAN2(1,2)"
        );
        assert_eq!(shift_formula("3.14*A1+1E3", 1, 0), "3.14*A2+1E3");
        assert_eq!(
            shift_formula("Table1[Col1]+Table1[[#This Row],[B2]]", 1, 0),
            "Table1[Col1]+Table1[[#This Row],[B2]]"
        );
        assert_eq!(shift_formula("MyName+TRUE", 1, 1), "MyName+TRUE");
        assert_eq!(shift_formula("_xlfn.CONCAT(A1)", 1, 0), "_xlfn.CONCAT(A2)");
        assert_eq!(
            shift_formula("Données!A1+Ω1", 1, 0),
            "Données!A2+Ω1",
            "non-ASCII names"
        );
    }

    #[test]
    fn column_and_row_ranges() {
        assert_eq!(shift_formula("SUM(A:A)", 5, 1), "SUM(B:B)");
        assert_eq!(shift_formula("SUM($A:C)", 0, 1), "SUM($A:D)");
        assert_eq!(shift_formula("SUM(1:1)", 1, 3), "SUM(2:2)");
        assert_eq!(shift_formula("SUM($1:3)", 1, 0), "SUM($1:4)");
    }

    #[test]
    fn references_off_the_sheet_become_ref_errors() {
        assert_eq!(shift_formula("A1", -1, 0), "#REF!");
        assert_eq!(shift_formula("XFD1+1", 0, 1), "#REF!+1");
        assert_eq!(shift_formula("A:A", 0, -1), "#REF!");
        assert_eq!(shift_formula("1:1", -1, 0), "#REF!");
        assert_eq!(
            shift_formula("$A$1", -1, -1),
            "$A$1",
            "absolute references never move"
        );
    }
}
