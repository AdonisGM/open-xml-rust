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

#[cfg(test)]
mod tests {
    use super::*;

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
