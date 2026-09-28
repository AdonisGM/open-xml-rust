//! A small formula evaluator that fills in the cached results of formulas.
//!
//! Excel recalculates formulas when it opens a file; other readers often
//! show the cached result stored next to each formula instead.
//! [`Workbook::calculate`] computes those results for a clearly limited
//! subset of Excel's language and leaves every other formula untouched:
//!
//! * numbers, strings, `TRUE`/`FALSE`, error literals;
//! * cell and area references, optionally sheet-qualified (`Sheet2!A1:B3`);
//! * operators `+ - * / ^ & % = <> < <= > >=` and parentheses;
//! * functions `SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `COUNTA`, `IF`,
//!   `AND`, `OR`, `NOT`, `ABS`, `ROUND`, `CONCAT`, `CONCATENATE`.
//!
//! Formulas using anything else (names, whole-column references, structured
//! references, other functions, array formulas) and formulas that depend on
//! them are skipped, as are circular references.
//!
//! ```
//! use openxml_xlsx::{Workbook, CellValue};
//!
//! let mut wb = Workbook::new();
//! {
//!     let mut s = wb.worksheet_mut("Sheet1")?;
//!     s.set_value("A1", 2.0)?;
//!     s.set_value("A2", 3.0)?;
//!     s.set_formula("A3", "SUM(A1:A2)*10")?;
//!     s.set_formula("A4", "IF(A3>40,\"big\",\"small\")")?;
//! }
//! let report = wb.calculate()?;
//! assert_eq!(report.calculated, 2);
//! let s = wb.worksheet("Sheet1")?;
//! assert_eq!(s.cell("A3")?.result(), &CellValue::Number(50.0));
//! assert_eq!(s.cell("A4")?.result(), &CellValue::Text("big".into()));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use std::collections::{HashMap, HashSet};

use openxml_core::Result;
use openxml_schema::sml;

use crate::cell_ref::{CellRange, CellRef};
use crate::date::DateSystem;
use crate::value::{CellValue, encode_xstring, format_number};
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;

/// Outcome of [`Workbook::calculate`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CalcReport {
    /// Formulas whose result was computed.
    pub calculated: usize,
    /// Formulas outside the supported subset (left unchanged).
    pub skipped: usize,
    /// Formula cells whose stored result changed.
    pub updated: usize,
}

/// A computed value.
#[derive(Clone, Debug, PartialEq)]
enum V {
    Num(f64),
    Text(String),
    Bool(bool),
    Err(String),
    Empty,
    /// The values of an area (for function arguments).
    List(Vec<V>),
}

/// The formula is outside the supported subset.
#[derive(Debug)]
struct Unsupported;

type Eval<T> = std::result::Result<T, Unsupported>;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Err(String),
    Ref(Option<String>, CellRange),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

const ERRORS: [&str; 7] = ["#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"];

fn tokenize(f: &str) -> Eval<Vec<Tok>> {
    let b = f.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    let rest = f.get(i..).ok_or(Unsupported)?;
                    let q = rest.find('"').ok_or(Unsupported)?;
                    s.push_str(&rest[..q]);
                    i += q + 1;
                    if b.get(i) == Some(&b'"') {
                        s.push('"');
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Tok::Str(s));
            }
            b'#' => {
                let e = ERRORS
                    .iter()
                    .find(|e| f[i..].to_ascii_uppercase().starts_with(*e))
                    .ok_or(Unsupported)?;
                out.push(Tok::Err((*e).to_owned()));
                i += e.len();
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b'<' | b'>' => {
                let two = f.get(i..i + 2);
                let op = match two {
                    Some("<=") => "<=",
                    Some(">=") => ">=",
                    Some("<>") => "<>",
                    _ if c == b'<' => "<",
                    _ => ">",
                };
                out.push(Tok::Op(op));
                i += op.len();
            }
            b'+' | b'-' | b'*' | b'/' | b'^' | b'&' | b'=' | b'%' => {
                let op = match c {
                    b'+' => "+",
                    b'-' => "-",
                    b'*' => "*",
                    b'/' => "/",
                    b'^' => "^",
                    b'&' => "&",
                    b'=' => "=",
                    _ => "%",
                };
                out.push(Tok::Op(op));
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && b[j].is_ascii_digit() {
                        i = j;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                if b.get(i) == Some(&b':') {
                    return Err(Unsupported); // row ranges
                }
                out.push(Tok::Num(f[start..i].parse().map_err(|_| Unsupported)?));
            }
            b'\'' => {
                let end = f[i + 1..].find("'!").ok_or(Unsupported)? + i + 1;
                let sheet = f[i + 1..end].replace("''", "'");
                let (range, len) = parse_area(&f[end + 2..])?;
                out.push(Tok::Ref(Some(sheet), range));
                i = end + 2 + len;
            }
            _ if c == b'$' || c == b'_' || c.is_ascii_alphabetic() || c >= 0x80 => {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b'$') || b[i] >= 0x80)
                {
                    i += 1;
                }
                let word = &f[start..i];
                if b.get(i) == Some(&b'!') {
                    let (range, len) = parse_area(&f[i + 1..])?;
                    out.push(Tok::Ref(Some(word.to_owned()), range));
                    i += 1 + len;
                } else if b.get(i) == Some(&b'(') {
                    out.push(Tok::Ident(word.to_ascii_uppercase()));
                } else if let Ok((range, len)) = parse_area(&f[start..]) {
                    out.push(Tok::Ref(None, range));
                    i = start + len;
                } else {
                    match word.to_ascii_uppercase().as_str() {
                        "TRUE" => out.push(Tok::Ident("TRUE".into())),
                        "FALSE" => out.push(Tok::Ident("FALSE".into())),
                        _ => return Err(Unsupported), // defined names
                    }
                }
            }
            _ => return Err(Unsupported),
        }
    }
    Ok(out)
}

/// Parses `A1` or `A1:B2` at the start of `s`; returns the range and its length.
fn parse_area(s: &str) -> Eval<(CellRange, usize)> {
    let cell_len = |t: &str| -> Option<usize> {
        let b = t.as_bytes();
        let mut i = usize::from(b.first() == Some(&b'$'));
        let ls = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == ls || i - ls > 3 {
            return None;
        }
        if b.get(i) == Some(&b'$') {
            i += 1;
        }
        let ds = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        (i > ds).then_some(i)
    };
    let n1 = cell_len(s).ok_or(Unsupported)?;
    let a = CellRef::parse(&s[..n1]).map_err(|_| Unsupported)?;
    let (range, len) = if s[n1..].starts_with(':') {
        match cell_len(&s[n1 + 1..]) {
            Some(n2) => {
                let b = CellRef::parse(&s[n1 + 1..n1 + 1 + n2]).map_err(|_| Unsupported)?;
                (CellRange::new(a, b), n1 + 1 + n2)
            }
            None => return Err(Unsupported),
        }
    } else {
        (CellRange::single(a), n1)
    };
    let next = s.as_bytes().get(len);
    if next.is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'(' || c == b'.') {
        return Err(Unsupported);
    }
    Ok((range, len))
}

#[derive(Clone, Debug)]
enum Expr {
    Val(V),
    Ref(Option<String>, CellRange),
    Neg(Box<Expr>),
    Percent(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn binary(&mut self, level: usize) -> Eval<Expr> {
        const LEVELS: [&[&str]; 5] = [
            &["=", "<>", "<", "<=", ">", ">="],
            &["&"],
            &["+", "-"],
            &["*", "/"],
            &["^"],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.pos += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Eval<Expr> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.unary()
            }
            _ => {
                let mut e = self.primary()?;
                while self.peek() == Some(&Tok::Op("%")) {
                    self.pos += 1;
                    e = Expr::Percent(Box::new(e));
                }
                Ok(e)
            }
        }
    }

    fn primary(&mut self) -> Eval<Expr> {
        match self.next().ok_or(Unsupported)? {
            Tok::Num(n) => Ok(Expr::Val(V::Num(n))),
            Tok::Str(s) => Ok(Expr::Val(V::Text(s))),
            Tok::Err(e) => Ok(Expr::Val(V::Err(e))),
            Tok::Ref(sheet, r) => Ok(Expr::Ref(sheet, r)),
            Tok::LParen => {
                let e = self.binary(0)?;
                (self.next() == Some(Tok::RParen)).then_some(e).ok_or(Unsupported)
            }
            Tok::Ident(name) if name == "TRUE" || name == "FALSE" => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    (self.next() == Some(Tok::RParen))
                        .then_some(())
                        .ok_or(Unsupported)?;
                }
                Ok(Expr::Val(V::Bool(name == "TRUE")))
            }
            Tok::Ident(name) => {
                let name = name.strip_prefix("_XLFN.").unwrap_or(&name).to_owned();
                (self.next() == Some(Tok::LParen))
                    .then_some(())
                    .ok_or(Unsupported)?;
                let mut args = Vec::new();
                if self.peek() == Some(&Tok::RParen) {
                    self.pos += 1;
                } else {
                    loop {
                        args.push(self.binary(0)?);
                        match self.next() {
                            Some(Tok::Comma) => {}
                            Some(Tok::RParen) => break,
                            _ => return Err(Unsupported),
                        }
                    }
                }
                Ok(Expr::Call(name, args))
            }
            _ => Err(Unsupported),
        }
    }
}

fn parse(formula: &str) -> Eval<Expr> {
    let toks = tokenize(formula.trim_start_matches('='))?;
    let mut p = Parser { toks, pos: 0 };
    let e = p.binary(0)?;
    (p.pos == p.toks.len()).then_some(e).ok_or(Unsupported)
}

fn to_number(v: &V) -> std::result::Result<f64, String> {
    match v {
        V::Num(n) => Ok(*n),
        V::Bool(b) => Ok(f64::from(u8::from(*b))),
        V::Empty => Ok(0.0),
        V::Text(t) => t.trim().parse().map_err(|_| "#VALUE!".to_owned()),
        V::Err(e) => Err(e.clone()),
        V::List(l) if l.len() == 1 => to_number(&l[0]),
        V::List(_) => Err("#VALUE!".into()),
    }
}

fn to_text(v: &V) -> std::result::Result<String, String> {
    match v {
        V::Num(n) => Ok(format_number(*n)),
        V::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
        V::Empty => Ok(String::new()),
        V::Text(t) => Ok(t.clone()),
        V::Err(e) => Err(e.clone()),
        V::List(l) if l.len() == 1 => to_text(&l[0]),
        V::List(_) => Err("#VALUE!".into()),
    }
}

fn to_bool(v: &V) -> std::result::Result<bool, String> {
    match v {
        V::Bool(b) => Ok(*b),
        V::Num(n) => Ok(*n != 0.0),
        V::Empty => Ok(false),
        V::Text(t) if t.eq_ignore_ascii_case("true") => Ok(true),
        V::Text(t) if t.eq_ignore_ascii_case("false") => Ok(false),
        V::Text(_) => Err("#VALUE!".into()),
        V::Err(e) => Err(e.clone()),
        V::List(l) if l.len() == 1 => to_bool(&l[0]),
        V::List(_) => Err("#VALUE!".into()),
    }
}

fn scalar(v: V) -> V {
    match v {
        V::List(mut l) if l.len() == 1 => l.pop().expect("one value"),
        other => other,
    }
}

/// Excel's ordering of mixed values: numbers < text < booleans.
fn compare(a: &V, b: &V) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank = |v: &V| match v {
        V::Num(_) | V::Empty => 0,
        V::Text(_) => 1,
        _ => 2,
    };
    match (a, b) {
        (V::Empty, V::Text(t)) => "".cmp(t.to_lowercase().as_str()),
        (V::Text(t), V::Empty) => t.to_lowercase().as_str().cmp(""),
        (V::Empty, V::Bool(x)) => false.cmp(x),
        (V::Bool(x), V::Empty) => x.cmp(&false),
        _ => match rank(a).cmp(&rank(b)) {
            Ordering::Equal => match (a, b) {
                (V::Text(x), V::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                (V::Bool(x), V::Bool(y)) => x.cmp(y),
                _ => {
                    let (x, y) = (to_number(a).unwrap_or(0.0), to_number(b).unwrap_or(0.0));
                    x.partial_cmp(&y).unwrap_or(Ordering::Equal)
                }
            },
            o => o,
        },
    }
}

struct Ctx<'w> {
    views: HashMap<usize, Worksheet<'w>>,
    names: Vec<(String, usize)>,
    date_system: DateSystem,
    memo: HashMap<(usize, CellRef), Eval<V>>,
    visiting: HashSet<(usize, CellRef)>,
}

impl Ctx<'_> {
    fn sheet_index(&self, name: &str) -> Eval<usize> {
        self.names
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, i)| *i)
            .ok_or(Unsupported)
    }

    fn cell(&mut self, sheet: usize, at: CellRef) -> Eval<V> {
        if let Some(v) = self.memo.get(&(sheet, at)) {
            return match v {
                Ok(v) => Ok(v.clone()),
                Err(_) => Err(Unsupported),
            };
        }
        if !self.visiting.insert((sheet, at)) {
            return Err(Unsupported); // circular reference
        }
        let value = self.views[&sheet].cell(at).map_err(|_| Unsupported)?;
        let array = self.views[&sheet]
            .find_cell(at)
            .and_then(|c| c.f.as_deref())
            .is_some_and(|f| f.t == Some(sml::ST_CellFormulaType::Array));
        let result = match value {
            CellValue::Formula { .. } if array => Err(Unsupported),
            CellValue::Formula { formula, .. } => {
                parse(&formula)
                    .and_then(|e| self.eval(sheet, &e))
                    .map(|v| match scalar(v) {
                        V::Empty => V::Num(0.0),
                        V::List(_) => V::Err("#VALUE!".into()),
                        other => other,
                    })
            }
            CellValue::Empty => Ok(V::Empty),
            CellValue::Number(n) => Ok(V::Num(n)),
            CellValue::Text(t) => Ok(V::Text(t)),
            CellValue::Bool(b) => Ok(V::Bool(b)),
            CellValue::Error(e) => Ok(V::Err(e)),
            CellValue::DateTime(d) => Ok(d
                .to_serial(self.date_system)
                .map_or(V::Err("#NUM!".into()), V::Num)),
        };
        self.visiting.remove(&(sheet, at));
        let stored = match &result {
            Ok(v) => Ok(v.clone()),
            Err(_) => Err(Unsupported),
        };
        self.memo.insert((sheet, at), stored);
        result
    }

    fn eval(&mut self, sheet: usize, e: &Expr) -> Eval<V> {
        Ok(match e {
            Expr::Val(v) => v.clone(),
            Expr::Ref(name, range) => {
                let s = match name {
                    Some(n) => self.sheet_index(n)?,
                    None => sheet,
                };
                if range.len() > 100_000 {
                    return Err(Unsupported);
                }
                let mut values = Vec::with_capacity(range.len() as usize);
                for c in range.cells() {
                    values.push(self.cell(s, c)?);
                }
                if range.len() == 1 {
                    values.pop().expect("one cell")
                } else {
                    V::List(values)
                }
            }
            Expr::Neg(x) => match to_number(&scalar(self.eval(sheet, x)?)) {
                Ok(n) => V::Num(-n),
                Err(e) => V::Err(e),
            },
            Expr::Percent(x) => match to_number(&scalar(self.eval(sheet, x)?)) {
                Ok(n) => V::Num(n / 100.0),
                Err(e) => V::Err(e),
            },
            Expr::Bin(op, a, b) => {
                let (a, b) = (scalar(self.eval(sheet, a)?), scalar(self.eval(sheet, b)?));
                if matches!(a, V::List(_)) || matches!(b, V::List(_)) {
                    return Err(Unsupported); // implicit intersection
                }
                binary(op, &a, &b)
            }
            Expr::Call(name, args) => self.call(sheet, name, args)?,
        })
    }

    fn args(&mut self, sheet: usize, args: &[Expr]) -> Eval<Vec<V>> {
        args.iter().map(|a| self.eval(sheet, a)).collect()
    }

    fn call(&mut self, sheet: usize, name: &str, args: &[Expr]) -> Eval<V> {
        let need = |n: std::ops::RangeInclusive<usize>| {
            if n.contains(&args.len()) {
                Ok(())
            } else {
                Err(Unsupported)
            }
        };
        Ok(match name {
            "IF" => {
                need(2..=3)?;
                let cond = scalar(self.eval(sheet, &args[0])?);
                match to_bool(&cond) {
                    Err(e) => V::Err(e),
                    Ok(true) => scalar(self.eval(sheet, &args[1])?),
                    Ok(false) => match args.get(2) {
                        Some(a) => scalar(self.eval(sheet, a)?),
                        None => V::Bool(false),
                    },
                }
            }
            "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT" | "COUNTA" => {
                need(1..=255)?;
                let values = self.args(sheet, args)?;
                aggregate(name, &values)
            }
            "AND" | "OR" => {
                need(1..=255)?;
                let values = self.args(sheet, args)?;
                let mut bools = Vec::new();
                for v in &values {
                    match v {
                        V::List(l) => {
                            for x in l {
                                match x {
                                    V::Err(e) => return Ok(V::Err(e.clone())),
                                    V::Bool(b) => bools.push(*b),
                                    V::Num(n) => bools.push(*n != 0.0),
                                    _ => {}
                                }
                            }
                        }
                        other => match to_bool(other) {
                            Ok(b) => bools.push(b),
                            Err(e) => return Ok(V::Err(e)),
                        },
                    }
                }
                if bools.is_empty() {
                    V::Err("#VALUE!".into())
                } else if name == "AND" {
                    V::Bool(bools.iter().all(|b| *b))
                } else {
                    V::Bool(bools.iter().any(|b| *b))
                }
            }
            "NOT" => {
                need(1..=1)?;
                match to_bool(&scalar(self.eval(sheet, &args[0])?)) {
                    Ok(b) => V::Bool(!b),
                    Err(e) => V::Err(e),
                }
            }
            "ABS" => {
                need(1..=1)?;
                match to_number(&scalar(self.eval(sheet, &args[0])?)) {
                    Ok(n) => V::Num(n.abs()),
                    Err(e) => V::Err(e),
                }
            }
            "ROUND" => {
                need(2..=2)?;
                let x = to_number(&scalar(self.eval(sheet, &args[0])?));
                let d = to_number(&scalar(self.eval(sheet, &args[1])?));
                match (x, d) {
                    (Ok(x), Ok(d)) => {
                        let f = 10f64.powi(d.trunc() as i32);
                        // Excel rounds half away from zero.
                        V::Num(round_half_away((x * f).abs()) * x.signum() / f)
                    }
                    (Err(e), _) | (_, Err(e)) => V::Err(e),
                }
            }
            "CONCAT" | "CONCATENATE" => {
                need(1..=255)?;
                let values = self.args(sheet, args)?;
                let mut out = String::new();
                for v in &values {
                    let parts: Vec<&V> = match v {
                        V::List(l) if name == "CONCAT" => l.iter().collect(),
                        V::List(l) if l.len() == 1 => vec![&l[0]],
                        V::List(_) => return Ok(V::Err("#VALUE!".into())),
                        other => vec![other],
                    };
                    for p in parts {
                        match to_text(p) {
                            Ok(t) => out.push_str(&t),
                            Err(e) => return Ok(V::Err(e)),
                        }
                    }
                }
                V::Text(out)
            }
            _ => return Err(Unsupported),
        })
    }
}

/// Rounds a non-negative value half up, tolerating representation error
/// (2.675 × 100 = 267.49999…).
fn round_half_away(v: f64) -> f64 {
    (v + 1e-9).round()
}

fn binary(op: &str, a: &V, b: &V) -> V {
    match op {
        "&" => match (to_text(a), to_text(b)) {
            (Ok(x), Ok(y)) => V::Text(x + &y),
            (Err(e), _) | (_, Err(e)) => V::Err(e),
        },
        "=" | "<>" | "<" | "<=" | ">" | ">=" => {
            if let V::Err(e) = a {
                return V::Err(e.clone());
            }
            if let V::Err(e) = b {
                return V::Err(e.clone());
            }
            let o = compare(a, b);
            use std::cmp::Ordering::*;
            V::Bool(match op {
                "=" => o == Equal,
                "<>" => o != Equal,
                "<" => o == Less,
                "<=" => o != Greater,
                ">" => o == Greater,
                _ => o != Less,
            })
        }
        _ => {
            let (x, y) = match (to_number(a), to_number(b)) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(e), _) | (_, Err(e)) => return V::Err(e),
            };
            let r = match op {
                "+" => x + y,
                "-" => x - y,
                "*" => x * y,
                "/" if y == 0.0 => return V::Err("#DIV/0!".into()),
                "/" => x / y,
                _ => x.powf(y),
            };
            if r.is_finite() {
                V::Num(r)
            } else {
                V::Err("#NUM!".into())
            }
        }
    }
}

fn aggregate(name: &str, values: &[V]) -> V {
    let mut nums = Vec::new();
    let mut non_empty = 0usize;
    for v in values {
        match v {
            V::List(l) => {
                for x in l {
                    match x {
                        V::Num(n) => nums.push(*n),
                        V::Err(e) if name != "COUNT" && name != "COUNTA" => return V::Err(e.clone()),
                        _ => {}
                    }
                    if !matches!(x, V::Empty) {
                        non_empty += 1;
                    }
                }
            }
            V::Empty => {}
            V::Err(e) if name != "COUNT" && name != "COUNTA" => return V::Err(e.clone()),
            other => {
                non_empty += 1;
                match to_number(other) {
                    Ok(n) => nums.push(n),
                    Err(e) if name != "COUNT" && name != "COUNTA" => return V::Err(e),
                    Err(_) => {}
                }
            }
        }
    }
    match name {
        "SUM" => V::Num(nums.iter().sum()),
        "AVERAGE" if nums.is_empty() => V::Err("#DIV/0!".into()),
        "AVERAGE" => V::Num(nums.iter().sum::<f64>() / nums.len() as f64),
        "MIN" | "MAX" if nums.is_empty() => V::Num(0.0),
        "MIN" => V::Num(nums.iter().copied().fold(f64::INFINITY, f64::min)),
        "MAX" => V::Num(nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "COUNT" => V::Num(nums.len() as f64),
        _ => V::Num(non_empty as f64),
    }
}

impl Workbook {
    /// Computes the results of the formulas in the supported subset (see
    /// the [module documentation](crate::calc)) and stores them as the
    /// formulas' cached values. Formulas outside the subset keep their
    /// stored results; Excel still recalculates everything on open when
    /// asked to (`fullCalcOnLoad`).
    pub fn calculate(&mut self) -> Result<CalcReport> {
        let positions = self.worksheet_positions();
        let mut results: Vec<(usize, CellRef, V)> = Vec::new();
        let mut report = CalcReport::default();
        {
            let mut ctx = Ctx {
                views: HashMap::new(),
                names: Vec::new(),
                date_system: self.date_system,
                memo: HashMap::new(),
                visiting: HashSet::new(),
            };
            for &i in &positions {
                let view = self.view(i)?;
                ctx.names.push((view.name().to_owned(), i));
                ctx.views.insert(i, view);
            }
            for &i in &positions {
                let cells: Vec<CellRef> = ctx.views[&i]
                    .raw()
                    .sheet_data
                    .iter()
                    .flat_map(|d| d.row.iter())
                    .flat_map(|r| r.c.iter())
                    .filter(|c| c.f.is_some())
                    .filter_map(|c| CellRef::parse(c.r.as_deref()?).ok())
                    .collect();
                for at in cells {
                    match ctx.cell(i, at) {
                        Ok(v) => {
                            report.calculated += 1;
                            results.push((i, at, v));
                        }
                        Err(_) => report.skipped += 1,
                    }
                }
            }
        }
        let mut by_sheet: HashMap<usize, Vec<(CellRef, V)>> = HashMap::new();
        for (i, at, v) in results {
            by_sheet.entry(i).or_default().push((at, v));
        }
        for (i, cells) in by_sheet {
            let was_dirty = self.sheets[i].dirty;
            let mut changed = false;
            let mut ws = self.view_mut(i)?;
            for (at, v) in cells {
                let (t, text) = match v {
                    V::Num(n) => (None, format_number(n)),
                    V::Text(s) => (Some(sml::ST_CellType::Str), encode_xstring(&s).into_owned()),
                    V::Bool(b) => (Some(sml::ST_CellType::B), u8::from(b).to_string()),
                    V::Err(e) => (Some(sml::ST_CellType::E), e),
                    V::Empty | V::List(_) => (None, "0".to_owned()),
                };
                let cell = ws.cell_mut(at);
                if cell.t != t || cell.v.as_deref() != Some(text.as_str()) {
                    cell.t = t;
                    cell.v = Some(text);
                    changed = true;
                    report.updated += 1;
                }
            }
            if !changed {
                self.sheets[i].dirty = was_dirty;
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(f: &str) -> V {
        let mut ctx = Ctx {
            views: HashMap::new(),
            names: Vec::new(),
            date_system: DateSystem::V1900,
            memo: HashMap::new(),
            visiting: HashSet::new(),
        };
        ctx.eval(0, &parse(f).expect("supported")).expect("evaluated")
    }

    #[test]
    fn operators_and_precedence() {
        assert_eq!(eval("1+2*3"), V::Num(7.0));
        assert_eq!(eval("(1+2)*3"), V::Num(9.0));
        assert_eq!(eval("2^3^2"), V::Num(64.0), "Excel's ^ is left-associative");
        assert_eq!(eval("-2^2"), V::Num(4.0), "negation binds tighter than ^");
        assert_eq!(eval("50%"), V::Num(0.5));
        assert_eq!(eval("1/0"), V::Err("#DIV/0!".into()));
        assert_eq!(eval("\"a\"&1&TRUE"), V::Text("a1TRUE".into()));
        assert_eq!(eval("\"say \"\"hi\"\"\""), V::Text("say \"hi\"".into()));
        assert_eq!(eval("1<2"), V::Bool(true));
        assert_eq!(eval("\"b\">\"A\""), V::Bool(true), "text comparison ignores case");
        assert_eq!(eval("\"a\"=\"A\""), V::Bool(true));
        assert_eq!(eval("1<\"a\""), V::Bool(true), "numbers sort before text");
        assert_eq!(eval("\"x\"+1"), V::Err("#VALUE!".into()));
        assert_eq!(eval("\"2\"+1"), V::Num(3.0));
        assert_eq!(eval("#N/A+1"), V::Err("#N/A".into()));
        assert_eq!(eval("1.5E+2"), V::Num(150.0));
    }

    #[test]
    fn functions() {
        assert_eq!(eval("SUM(1,2,3)"), V::Num(6.0));
        assert_eq!(eval("AVERAGE(1,2,3,4)"), V::Num(2.5));
        assert_eq!(eval("MIN(4,2,8)"), V::Num(2.0));
        assert_eq!(eval("MAX(4,2,8)"), V::Num(8.0));
        assert_eq!(eval("COUNT(1,\"x\",TRUE)"), V::Num(2.0));
        assert_eq!(eval("COUNTA(1,\"x\",TRUE)"), V::Num(3.0));
        assert_eq!(eval("IF(1>2,\"yes\",\"no\")"), V::Text("no".into()));
        assert_eq!(eval("IF(FALSE,1)"), V::Bool(false));
        assert_eq!(
            eval("IF(TRUE,1,1/0)"),
            V::Num(1.0),
            "the other branch is not evaluated"
        );
        assert_eq!(eval("_xlfn.CONCAT(\"a\",1,\"b\")"), V::Text("a1b".into()));
        assert_eq!(eval("CONCATENATE(\"x\",2)"), V::Text("x2".into()));
        assert_eq!(eval("AND(TRUE,1)"), V::Bool(true));
        assert_eq!(eval("OR(FALSE,0)"), V::Bool(false));
        assert_eq!(eval("NOT(0)"), V::Bool(true));
        assert_eq!(eval("ABS(-3)"), V::Num(3.0));
        assert_eq!(eval("ROUND(2.675,2)"), V::Num(2.68));
        assert_eq!(eval("ROUND(-1.5,0)"), V::Num(-2.0));
        assert_eq!(eval("ROUND(1234,-2)"), V::Num(1200.0));
        assert_eq!(eval("AVERAGE(\"x\")"), V::Err("#VALUE!".into()));
    }

    #[test]
    fn unsupported_formulas_are_detected() {
        for f in [
            "VLOOKUP(1,A1:B2,2)",
            "MyName+1",
            "SUM(A:A)",
            "Table1[Col]",
            "{1,2}",
            "SUM(1:2)",
            "1+",
        ] {
            assert!(parse(f).is_err() || matches!(parse(f), Ok(Expr::Call(..))), "{f}");
        }
        assert!(parse("SUM(A1:B2)").is_ok());
        assert!(parse("'My Sheet'!A1*2").is_ok());
    }
}
