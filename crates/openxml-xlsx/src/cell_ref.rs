//! A1-style cell references and ranges.

use std::fmt;
use std::str::FromStr;

use openxml_core::{Error, Result};

/// Number of rows of a worksheet (ECMA-376 Part 1 §18.3.1.73: 1 048 576).
pub const MAX_ROW: u32 = 1_048_576;
/// Number of columns of a worksheet (16 384, column `XFD`).
pub const MAX_COL: u32 = 16_384;

/// A cell position. Rows and columns are 1-based (`A1` is row 1, column 1).
///
/// References order by row, then column — the order in which cells appear
/// in a worksheet part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellRef {
    row: u32,
    col: u32,
}

fn invalid(what: &str, value: impl fmt::Display) -> Error {
    Error::InvalidArgument(format!("{what} {value}"))
}

impl CellRef {
    /// Creates a reference from 1-based row and column numbers.
    pub fn new(row: u32, col: u32) -> Result<Self> {
        if !(1..=MAX_ROW).contains(&row) {
            return Err(invalid("row out of range:", row));
        }
        if !(1..=MAX_COL).contains(&col) {
            return Err(invalid("column out of range:", col));
        }
        Ok(CellRef { row, col })
    }

    /// Parses an A1 reference such as `B12`, `$B$12` or `xfd1048576`.
    pub fn parse(s: &str) -> Result<Self> {
        let t = s.trim();
        let bytes = t.as_bytes();
        let mut i = 0;
        if bytes.get(i) == Some(&b'$') {
            i += 1;
        }
        let letters_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let letters = &t[letters_start..i];
        if bytes.get(i) == Some(&b'$') {
            i += 1;
        }
        let digits = &t[i..];
        if letters.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("invalid cell reference:", format!("{s:?}")));
        }
        if digits.starts_with('0') {
            return Err(invalid("invalid cell reference:", format!("{s:?}")));
        }
        let col = column_index(letters)?;
        let row = digits
            .parse::<u32>()
            .map_err(|_| invalid("row out of range in", format!("{s:?}")))?;
        CellRef::new(row, col)
    }

    /// Row number (1-based).
    pub fn row(self) -> u32 {
        self.row
    }

    /// Column number (1-based).
    pub fn col(self) -> u32 {
        self.col
    }

    /// Column letters, e.g. `"AB"`.
    pub fn column_name(self) -> String {
        column_name(self.col)
    }

    /// The reference moved by `rows` and `cols` (which may be negative).
    pub fn offset(self, rows: i64, cols: i64) -> Result<Self> {
        let row = i64::from(self.row) + rows;
        let col = i64::from(self.col) + cols;
        if row < 1 || col < 1 || row > i64::from(MAX_ROW) || col > i64::from(MAX_COL) {
            return Err(invalid(
                "reference moved out of the sheet:",
                format!("{self}{rows:+}/{cols:+}"),
            ));
        }
        CellRef::new(row as u32, col as u32)
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", column_name(self.col), self.row)
    }
}

impl FromStr for CellRef {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        CellRef::parse(s)
    }
}

impl TryFrom<&str> for CellRef {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        CellRef::parse(s)
    }
}

/// Converts a 1-based column number to letters (`1` → `A`, `28` → `AB`).
///
/// # Panics
///
/// Panics if `col` is zero.
pub fn column_name(col: u32) -> String {
    assert!(col > 0, "column numbers are 1-based");
    let mut n = col;
    let mut out = Vec::with_capacity(3);
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII letters")
}

/// Converts column letters (case-insensitive) to a 1-based column number.
pub fn column_index(letters: &str) -> Result<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return Err(invalid("invalid column name:", format!("{letters:?}")));
    }
    let mut n: u32 = 0;
    for b in letters.bytes() {
        if !b.is_ascii_alphabetic() {
            return Err(invalid("invalid column name:", format!("{letters:?}")));
        }
        n = n * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1);
    }
    if n > MAX_COL {
        return Err(invalid("column out of range:", letters));
    }
    Ok(n)
}

/// Anything that designates a cell: `"B3"`, `CellRef`, or `(row, col)` (1-based).
pub trait ToCellRef {
    /// Resolves the reference.
    fn to_cell_ref(&self) -> Result<CellRef>;
}

impl ToCellRef for CellRef {
    fn to_cell_ref(&self) -> Result<CellRef> {
        Ok(*self)
    }
}

impl ToCellRef for &str {
    fn to_cell_ref(&self) -> Result<CellRef> {
        CellRef::parse(self)
    }
}

impl ToCellRef for String {
    fn to_cell_ref(&self) -> Result<CellRef> {
        CellRef::parse(self)
    }
}

impl ToCellRef for (u32, u32) {
    fn to_cell_ref(&self) -> Result<CellRef> {
        CellRef::new(self.0, self.1)
    }
}

/// A rectangular block of cells, e.g. `A1:C3`. The corners are normalized so
/// that `start` is the top-left and `end` the bottom-right cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRange {
    start: CellRef,
    end: CellRef,
}

impl CellRange {
    /// Creates a range from two opposite corners (in any order).
    pub fn new(a: CellRef, b: CellRef) -> Self {
        CellRange {
            start: CellRef {
                row: a.row.min(b.row),
                col: a.col.min(b.col),
            },
            end: CellRef {
                row: a.row.max(b.row),
                col: a.col.max(b.col),
            },
        }
    }

    /// A range covering a single cell.
    pub fn single(cell: CellRef) -> Self {
        CellRange {
            start: cell,
            end: cell,
        }
    }

    /// Parses `A1:C3` (or a single reference such as `B2`).
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().split_once(':') {
            Some((a, b)) => Ok(CellRange::new(CellRef::parse(a)?, CellRef::parse(b)?)),
            None => Ok(CellRange::single(CellRef::parse(s)?)),
        }
    }

    /// Top-left cell.
    pub fn start(&self) -> CellRef {
        self.start
    }

    /// Bottom-right cell.
    pub fn end(&self) -> CellRef {
        self.end
    }

    /// Number of rows.
    pub fn height(&self) -> u32 {
        self.end.row - self.start.row + 1
    }

    /// Number of columns.
    pub fn width(&self) -> u32 {
        self.end.col - self.start.col + 1
    }

    /// Number of cells.
    pub fn len(&self) -> u64 {
        u64::from(self.height()) * u64::from(self.width())
    }

    /// Always `false`: a range contains at least one cell.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether the range contains `cell`.
    pub fn contains(&self, cell: CellRef) -> bool {
        (self.start.row..=self.end.row).contains(&cell.row)
            && (self.start.col..=self.end.col).contains(&cell.col)
    }

    /// Whether the two ranges share at least one cell.
    pub fn intersects(&self, other: &CellRange) -> bool {
        self.start.row <= other.end.row
            && other.start.row <= self.end.row
            && self.start.col <= other.end.col
            && other.start.col <= self.end.col
    }

    /// The smallest range containing both ranges.
    pub fn union(&self, other: &CellRange) -> CellRange {
        CellRange::new(
            CellRef {
                row: self.start.row.min(other.start.row),
                col: self.start.col.min(other.start.col),
            },
            CellRef {
                row: self.end.row.max(other.end.row),
                col: self.end.col.max(other.end.col),
            },
        )
    }

    /// Iterates over the cells row by row.
    pub fn cells(&self) -> impl Iterator<Item = CellRef> + '_ {
        let (s, e) = (self.start, self.end);
        (s.row..=e.row).flat_map(move |row| (s.col..=e.col).map(move |col| CellRef { row, col }))
    }
}

impl fmt::Display for CellRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}:{}", self.start, self.end)
        }
    }
}

impl FromStr for CellRange {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        CellRange::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_round_trip() {
        for (n, s) in [
            (1, "A"),
            (26, "Z"),
            (27, "AA"),
            (52, "AZ"),
            (53, "BA"),
            (702, "ZZ"),
            (703, "AAA"),
            (16_384, "XFD"),
        ] {
            assert_eq!(column_name(n), s);
            assert_eq!(column_index(s).unwrap(), n);
            assert_eq!(column_index(&s.to_lowercase()).unwrap(), n);
        }
        for n in 1..=MAX_COL {
            assert_eq!(column_index(&column_name(n)).unwrap(), n);
        }
    }

    #[test]
    fn invalid_column_names() {
        for bad in ["", "A1", "XFE", "ZZZZ", "Ä", "-"] {
            assert!(column_index(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    #[should_panic(expected = "1-based")]
    fn column_zero_panics() {
        column_name(0);
    }

    #[test]
    fn parses_references() {
        let r = CellRef::parse("B12").unwrap();
        assert_eq!((r.row(), r.col()), (12, 2));
        assert_eq!(r.to_string(), "B12");
        assert_eq!(r.column_name(), "B");
        assert_eq!(CellRef::parse("$b$12").unwrap(), r);
        assert_eq!(CellRef::parse(" A1 ").unwrap(), CellRef::new(1, 1).unwrap());
        assert_eq!(
            CellRef::parse("XFD1048576").unwrap(),
            CellRef::new(MAX_ROW, MAX_COL).unwrap()
        );
        assert_eq!("C3".parse::<CellRef>().unwrap(), CellRef::new(3, 3).unwrap());
        assert_eq!(CellRef::try_from("C3").unwrap(), CellRef::new(3, 3).unwrap());
    }

    #[test]
    fn rejects_invalid_references() {
        for bad in [
            "",
            "A",
            "1",
            "A0",
            "A01",
            "XFE1",
            "A1048577",
            "1A",
            "A-1",
            "A1B",
            "A 1",
            "$$A1",
            "A99999999999",
        ] {
            assert!(CellRef::parse(bad).is_err(), "{bad:?}");
        }
        assert!(CellRef::new(0, 1).is_err());
        assert!(CellRef::new(1, 0).is_err());
        assert!(CellRef::new(MAX_ROW + 1, 1).is_err());
        assert!(CellRef::new(1, MAX_COL + 1).is_err());
    }

    #[test]
    fn ordering_and_offsets() {
        let a1 = CellRef::parse("A1").unwrap();
        let b1 = CellRef::parse("B1").unwrap();
        let a2 = CellRef::parse("A2").unwrap();
        assert!(a1 < b1 && b1 < a2, "row-major order");
        assert_eq!(a1.offset(1, 1).unwrap(), CellRef::parse("B2").unwrap());
        assert_eq!(CellRef::parse("C5").unwrap().offset(-4, -2).unwrap(), a1);
        assert!(a1.offset(-1, 0).is_err());
        assert!(a1.offset(0, i64::from(MAX_COL)).is_err());
    }

    #[test]
    fn to_cell_ref_conversions() {
        let expected = CellRef::new(2, 3).unwrap();
        assert_eq!("C2".to_cell_ref().unwrap(), expected);
        assert_eq!(String::from("C2").to_cell_ref().unwrap(), expected);
        assert_eq!((2u32, 3u32).to_cell_ref().unwrap(), expected);
        assert_eq!(expected.to_cell_ref().unwrap(), expected);
        assert!((0u32, 1u32).to_cell_ref().is_err());
    }

    #[test]
    fn ranges() {
        let r = CellRange::parse("C3:A1").unwrap();
        assert_eq!(r.to_string(), "A1:C3", "corners are normalized");
        assert_eq!((r.width(), r.height(), r.len()), (3, 3, 9));
        assert!(!r.is_empty());
        assert!(r.contains(CellRef::parse("B2").unwrap()));
        assert!(!r.contains(CellRef::parse("D1").unwrap()));
        let single = CellRange::parse("B2").unwrap();
        assert_eq!(single.to_string(), "B2");
        assert_eq!(single.len(), 1);
        assert!(r.intersects(&single));
        assert!(!r.intersects(&CellRange::parse("D4:E5").unwrap()));
        assert!(r.intersects(&CellRange::parse("C3:E5").unwrap()), "shared corner");
        assert_eq!(r.union(&CellRange::parse("E5").unwrap()).to_string(), "A1:E5");
        let cells: Vec<String> = CellRange::parse("A1:B2")
            .unwrap()
            .cells()
            .map(|c| c.to_string())
            .collect();
        assert_eq!(cells, ["A1", "B1", "A2", "B2"]);
        assert!(CellRange::parse("A1:").is_err());
        assert!(CellRange::parse("A1:B").is_err());
        assert_eq!(
            "A1:B2".parse::<CellRange>().unwrap().end(),
            CellRef::parse("B2").unwrap()
        );
        assert_eq!(
            CellRange::parse("B2:B2").unwrap().start(),
            CellRef::parse("B2").unwrap()
        );
    }
}
