//! Cell values and the SpreadsheetML string escaping rules.

use std::borrow::Cow;
use std::fmt;

use crate::date::DateTime;

/// Error values a cell can hold (ECMA-376 Part 1 §18.17.3).
pub const ERROR_VALUES: [&str; 8] = [
    "#NULL!",
    "#DIV/0!",
    "#VALUE!",
    "#REF!",
    "#NAME?",
    "#NUM!",
    "#N/A",
    "#GETTING_DATA",
];

/// Maximum number of characters of a cell's text (Excel limit).
pub const MAX_TEXT_LEN: usize = 32_767;

/// The value of a cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CellValue {
    /// No value.
    #[default]
    Empty,
    /// A number.
    Number(f64),
    /// Text.
    Text(String),
    /// A Boolean.
    Bool(bool),
    /// An error value such as `#DIV/0!`.
    Error(String),
    /// A number displayed with a date or time format.
    DateTime(DateTime),
    /// A formula (without the leading `=`) and its last computed value, if known.
    Formula {
        /// Formula text in A1 notation, e.g. `SUM(A1:A3)`.
        formula: String,
        /// Cached result stored in the file.
        cached: Option<Box<CellValue>>,
    },
}

impl CellValue {
    /// A formula without a cached result. A leading `=` is removed.
    pub fn formula(formula: impl Into<String>) -> Self {
        let f: String = formula.into();
        let f = f.strip_prefix('=').map(str::to_owned).unwrap_or(f);
        CellValue::Formula {
            formula: f,
            cached: None,
        }
    }

    /// A formula with a cached result. A leading `=` is removed.
    pub fn formula_with_result(formula: impl Into<String>, result: impl Into<CellValue>) -> Self {
        match CellValue::formula(formula) {
            CellValue::Formula { formula, .. } => {
                let result = result.into();
                let cached = (!result.is_empty()).then(|| Box::new(result));
                CellValue::Formula { formula, cached }
            }
            _ => unreachable!("formula() builds a formula"),
        }
    }

    /// Whether the cell has no value.
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }

    /// The value itself, or the cached result of a formula (`Empty` if unknown).
    pub fn result(&self) -> &CellValue {
        match self {
            CellValue::Formula { cached: Some(v), .. } => v,
            CellValue::Formula { cached: None, .. } => &CellValue::Empty,
            other => other,
        }
    }

    /// The number, if the value (or formula result) is numeric.
    pub fn as_f64(&self) -> Option<f64> {
        match self.result() {
            CellValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The text, if the value (or formula result) is text.
    pub fn as_str(&self) -> Option<&str> {
        match self.result() {
            CellValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The Boolean, if the value (or formula result) is a Boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self.result() {
            CellValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The date, if the value (or formula result) is a date.
    pub fn as_datetime(&self) -> Option<DateTime> {
        match self.result() {
            CellValue::DateTime(d) => Some(*d),
            _ => None,
        }
    }

    /// The formula text, if the cell holds a formula.
    pub fn as_formula(&self) -> Option<&str> {
        match self {
            CellValue::Formula { formula, .. } => Some(formula),
            _ => None,
        }
    }
}

/// Formats a number the way it is stored in a cell (shortest exact form).
pub(crate) fn format_number(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    let s = format!("{n}");
    if s.len() > 17 && (n.abs() >= 1e16 || n.abs() < 1e-5) {
        format!("{n:E}")
    } else {
        s
    }
}

impl fmt::Display for CellValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CellValue::Empty => Ok(()),
            CellValue::Number(n) => f.write_str(&format_number(*n)),
            CellValue::Text(s) | CellValue::Error(s) => f.write_str(s),
            CellValue::Bool(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            CellValue::DateTime(d) => write!(f, "{d}"),
            CellValue::Formula { formula, .. } => write!(f, "={formula}"),
        }
    }
}

macro_rules! from_number {
    ($($t:ty),*) => {$(
        impl From<$t> for CellValue {
            fn from(v: $t) -> Self {
                CellValue::Number(v as f64)
            }
        }
    )*};
}
from_number!(f64, f32, i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

impl From<&str> for CellValue {
    fn from(v: &str) -> Self {
        CellValue::Text(v.to_owned())
    }
}

impl From<String> for CellValue {
    fn from(v: String) -> Self {
        CellValue::Text(v)
    }
}

impl From<&String> for CellValue {
    fn from(v: &String) -> Self {
        CellValue::Text(v.clone())
    }
}

impl From<bool> for CellValue {
    fn from(v: bool) -> Self {
        CellValue::Bool(v)
    }
}

impl From<DateTime> for CellValue {
    fn from(v: DateTime) -> Self {
        CellValue::DateTime(v)
    }
}

impl<T: Into<CellValue>> From<Option<T>> for CellValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(CellValue::Empty, Into::into)
    }
}

fn hex4(s: &[u8]) -> Option<u32> {
    if s.len() < 4 || !s[..4].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u32::from_str_radix(std::str::from_utf8(&s[..4]).ok()?, 16).ok()
}

/// Returns the code unit of an `_xHHHH_` escape starting at `i`, if any.
fn escape_at(b: &[u8], i: usize) -> Option<u32> {
    if b.get(i..i + 2) != Some(b"_x") || b.get(i + 6) != Some(&b'_') {
        return None;
    }
    hex4(&b[i + 2..])
}

/// Decodes `_xHHHH_` escapes of an `ST_Xstring` value (ECMA-376 Part 1 §22.9.2.19).
pub fn decode_xstring(s: &str) -> Cow<'_, str> {
    if !s.contains("_x") {
        return Cow::Borrowed(s);
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(unit) = escape_at(b, i) {
            // Surrogate pairs are written as two consecutive escapes.
            if (0xD800..0xDC00).contains(&unit)
                && let Some(low) = escape_at(b, i + 7).filter(|l| (0xDC00..0xE000).contains(l))
            {
                let c = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                if let Some(ch) = char::from_u32(c) {
                    out.push(ch);
                    i += 14;
                    continue;
                }
            }
            if let Some(ch) = char::from_u32(unit) {
                out.push(ch);
                i += 7;
                continue;
            }
        }
        let ch = s[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    Cow::Owned(out)
}

/// Encodes characters that XML cannot carry (and literal `_xHHHH_`
/// sequences) as `_xHHHH_` escapes.
pub fn encode_xstring(s: &str) -> Cow<'_, str> {
    let needs = |c: char| (c < ' ' && c != '\t' && c != '\n') || c == '\u{FFFE}' || c == '\u{FFFF}';
    if !s.chars().any(needs) && !s.contains("_x") {
        return Cow::Borrowed(s);
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, c) in s.char_indices() {
        if needs(c) {
            out.push_str(&format!("_x{:04X}_", c as u32));
        } else if c == '_' && escape_at(b, i).is_some() {
            out.push_str("_x005F_");
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_into_values() {
        assert_eq!(CellValue::from(3), CellValue::Number(3.0));
        assert_eq!(CellValue::from(2.5f32), CellValue::Number(2.5));
        assert_eq!(CellValue::from(7usize), CellValue::Number(7.0));
        assert_eq!(CellValue::from("x"), CellValue::Text("x".into()));
        assert_eq!(CellValue::from(String::from("y")), CellValue::Text("y".into()));
        assert_eq!(CellValue::from(&String::from("z")), CellValue::Text("z".into()));
        assert_eq!(CellValue::from(true), CellValue::Bool(true));
        assert_eq!(CellValue::from(None::<i32>), CellValue::Empty);
        assert_eq!(CellValue::from(Some(1)), CellValue::Number(1.0));
        let d = DateTime::from_ymd(2024, 1, 2).unwrap();
        assert_eq!(CellValue::from(d), CellValue::DateTime(d));
    }

    #[test]
    fn formulas() {
        let f = CellValue::formula("=SUM(A1:A2)");
        assert_eq!(f.as_formula(), Some("SUM(A1:A2)"));
        assert_eq!(f.result(), &CellValue::Empty);
        let g = CellValue::formula_with_result("A1*2", 6);
        assert_eq!(g.as_f64(), Some(6.0));
        assert_eq!(g.to_string(), "=A1*2");
        let h = CellValue::formula_with_result("A1", CellValue::Empty);
        assert_eq!(
            h,
            CellValue::Formula {
                formula: "A1".into(),
                cached: None
            }
        );
        assert_eq!(CellValue::formula_with_result("\"a\"", "a").as_str(), Some("a"));
        assert_eq!(CellValue::formula_with_result("TRUE", true).as_bool(), Some(true));
    }

    #[test]
    fn accessors() {
        assert_eq!(CellValue::Number(1.5).as_f64(), Some(1.5));
        assert_eq!(CellValue::Text("a".into()).as_f64(), None);
        assert_eq!(CellValue::Text("a".into()).as_str(), Some("a"));
        assert_eq!(CellValue::Bool(false).as_bool(), Some(false));
        let d = DateTime::from_ymd(2020, 5, 6).unwrap();
        assert_eq!(CellValue::DateTime(d).as_datetime(), Some(d));
        assert!(CellValue::Empty.is_empty());
        assert!(!CellValue::Number(0.0).is_empty());
        assert_eq!(CellValue::Number(1.0).as_formula(), None);
        assert_eq!(CellValue::default(), CellValue::Empty);
    }

    #[test]
    fn display() {
        assert_eq!(CellValue::Number(3.0).to_string(), "3");
        assert_eq!(CellValue::Number(-0.25).to_string(), "-0.25");
        assert_eq!(CellValue::Bool(true).to_string(), "TRUE");
        assert_eq!(CellValue::Error("#N/A".into()).to_string(), "#N/A");
        assert_eq!(CellValue::Empty.to_string(), "");
        assert_eq!(
            CellValue::DateTime(DateTime::from_ymd(2024, 3, 1).unwrap()).to_string(),
            "2024-03-01"
        );
    }

    #[test]
    fn number_formatting_is_exact() {
        for n in [
            0.0,
            1.0,
            -1.0,
            0.1,
            1.0 / 3.0,
            123_456_789.012_345,
            1e-7,
            1e21,
            6.02e23,
            f64::MAX,
            f64::MIN_POSITIVE,
        ] {
            let s = format_number(n);
            assert_eq!(s.parse::<f64>().unwrap(), n, "{s}");
            assert!(s.len() <= 24, "{s}");
        }
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(42.0), "42");
    }

    #[test]
    fn xstring_escapes() {
        assert_eq!(decode_xstring("plain"), "plain");
        assert!(matches!(decode_xstring("plain"), Cow::Borrowed(_)));
        assert_eq!(decode_xstring("a_x000D_b"), "a\rb");
        assert_eq!(decode_xstring("_x0001__x0002_"), "\u{1}\u{2}");
        assert_eq!(decode_xstring("_x005F_x0041_"), "_x0041_");
        assert_eq!(decode_xstring("_xD83D__xDE00_"), "😀");
        assert_eq!(
            decode_xstring("_x12_ and _xZZZZ_"),
            "_x12_ and _xZZZZ_",
            "malformed escapes stay"
        );
        assert_eq!(decode_xstring("_xD800_"), "_xD800_", "lone surrogates stay");

        assert_eq!(encode_xstring("plain"), "plain");
        assert_eq!(encode_xstring("a\rb\u{1}"), "a_x000D_b_x0001_");
        assert_eq!(encode_xstring("tab\tnew\nline"), "tab\tnew\nline");
        assert_eq!(encode_xstring("_x0041_"), "_x005F_x0041_");
        assert_eq!(encode_xstring("_x_y"), "_x_y");
        for s in ["a\rb", "_x0041_", "x\u{1}_x00FF_\u{FFFE}", "日本語"] {
            assert_eq!(decode_xstring(&encode_xstring(s)), s, "{s:?}");
        }
    }
}
