//! The worksheet auto filter: filter buttons on a header row, column
//! criteria and the recorded sort order (ECMA-376 Part 1 §18.3.1.2, §18.3.2).
//!
//! Excel does not re-run filters when it opens a file: rows that fail the
//! criteria must be hidden by the producer, which
//! [`WorksheetMut::apply_auto_filter`] does for value, custom and top-N
//! criteria.
//!
//! ```
//! use openxml_xlsx::{Workbook, ColumnFilter};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! for (i, (name, score)) in [("Name", "Score"), ("An", "7"), ("Binh", "3"), ("Chi", "9")].iter().enumerate() {
//!     let row = i as u32 + 1;
//!     sheet.set_value((row, 1), *name)?;
//!     match score.parse::<f64>() {
//!         Ok(n) => sheet.set_value((row, 2), n)?,
//!         Err(_) => sheet.set_value((row, 2), *score)?,
//!     }
//! }
//! sheet.set_auto_filter("A1:B4")?;
//! sheet.set_filter_column(1, ColumnFilter::greater_than(5.0))?;
//! assert_eq!(sheet.apply_auto_filter()?, 1, "Binh (3) is hidden");
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{CellRange, CellRef, ToCellRange};
use crate::util::{qualified_range, remove_local_name, set_local_name};
use crate::value::{CellValue, format_number};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use sml::ST_FilterOperator as FilterOperator;

/// Name of the hidden defined name Excel keeps for the auto filter range.
const FILTER_DATABASE: &str = "_xlnm._FilterDatabase";

/// Criteria of one filter column.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnFilter {
    /// Show rows whose value (as displayed) is one of `values`.
    Values {
        /// Accepted values.
        values: Vec<String>,
        /// Also show empty cells.
        blanks: bool,
    },
    /// One or two comparisons; text operands may use `*` and `?` wildcards.
    Custom {
        /// Both conditions must hold (otherwise either).
        and: bool,
        /// The conditions.
        conditions: Vec<(FilterOperator, String)>,
    },
    /// The top or bottom N items or percent.
    Top {
        /// N.
        count: f64,
        /// N is a percentage.
        percent: bool,
        /// Bottom instead of top.
        bottom: bool,
    },
}

impl ColumnFilter {
    /// Show rows with one of these values.
    pub fn values<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ColumnFilter::Values {
            values: values.into_iter().map(Into::into).collect(),
            blanks: false,
        }
    }

    /// Show rows where the value compares with `operand`.
    pub fn custom(operator: FilterOperator, operand: impl Into<String>) -> Self {
        ColumnFilter::Custom {
            and: false,
            conditions: vec![(operator, operand.into())],
        }
    }

    /// Show rows greater than `n`.
    pub fn greater_than(n: f64) -> Self {
        Self::custom(FilterOperator::GreaterThan, format_number(n))
    }

    /// Show rows less than `n`.
    pub fn less_than(n: f64) -> Self {
        Self::custom(FilterOperator::LessThan, format_number(n))
    }

    /// Show rows between `low` and `high` (inclusive).
    pub fn between(low: f64, high: f64) -> Self {
        ColumnFilter::Custom {
            and: true,
            conditions: vec![
                (FilterOperator::GreaterThanOrEqual, format_number(low)),
                (FilterOperator::LessThanOrEqual, format_number(high)),
            ],
        }
    }

    /// The `n` largest values.
    pub fn top(n: u32) -> Self {
        ColumnFilter::Top {
            count: f64::from(n),
            percent: false,
            bottom: false,
        }
    }

    fn to_ct(&self, col_id: u32) -> Result<sml::CT_FilterColumn> {
        use sml::CT_FilterColumn_Choice as C;
        let choice = match self {
            ColumnFilter::Values { values, blanks } => C::Filters(Box::new(sml::CT_Filters {
                blank: blanks.then_some(true),
                filter: values
                    .iter()
                    .map(|v| sml::CT_Filter {
                        val: Some(v.clone()),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            })),
            ColumnFilter::Custom { and, conditions } => {
                if conditions.is_empty() || conditions.len() > 2 {
                    return Err(Error::InvalidArgument(
                        "a custom filter has one or two conditions".into(),
                    ));
                }
                C::CustomFilters(Box::new(sml::CT_CustomFilters {
                    and: and.then_some(true),
                    custom_filter: conditions
                        .iter()
                        .map(|(op, v)| sml::CT_CustomFilter {
                            operator: (*op != FilterOperator::Equal).then_some(*op),
                            val: Some(v.clone()),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                }))
            }
            ColumnFilter::Top {
                count,
                percent,
                bottom,
            } => C::Top10(Box::new(sml::CT_Top10 {
                top: bottom.then_some(false),
                percent: percent.then_some(true),
                val: Some(*count),
                ..Default::default()
            })),
        };
        Ok(sml::CT_FilterColumn {
            col_id: Some(col_id),
            choice: Some(choice),
            ..Default::default()
        })
    }

    fn from_ct(c: &sml::CT_FilterColumn) -> Option<Self> {
        use sml::CT_FilterColumn_Choice as C;
        Some(match c.choice.as_ref()? {
            C::Filters(f) => ColumnFilter::Values {
                values: f.filter.iter().filter_map(|v| v.val.clone()).collect(),
                blanks: f.blank.unwrap_or(false),
            },
            C::CustomFilters(f) => ColumnFilter::Custom {
                and: f.and.unwrap_or(false),
                conditions: f
                    .custom_filter
                    .iter()
                    .map(|c| {
                        (
                            c.operator.unwrap_or(FilterOperator::Equal),
                            c.val.clone().unwrap_or_default(),
                        )
                    })
                    .collect(),
            },
            C::Top10(t) => ColumnFilter::Top {
                count: t.val.unwrap_or(10.0),
                percent: t.percent.unwrap_or(false),
                bottom: t.top == Some(false),
            },
            _ => return None,
        })
    }
}

/// A sort key of the recorded sort state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// Column offset within the filter range (0 = first column).
    pub column: u32,
    /// Descending order.
    pub descending: bool,
}

/// The auto filter of a worksheet.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoFilter {
    /// The range, header row included.
    pub range: CellRange,
    /// Criteria by column offset.
    pub columns: Vec<(u32, ColumnFilter)>,
    /// Recorded sort keys.
    pub sort: Vec<SortKey>,
}

/// Case-insensitive match with `*` and `?` wildcards.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '~' && pi + 1 < p.len() && p[pi + 1] == t[ti] {
            pi += 2;
            ti += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn display(v: &CellValue) -> String {
    match v {
        CellValue::Empty => String::new(),
        CellValue::Text(s) => s.clone(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        CellValue::Error(e) => e.clone(),
        CellValue::DateTime(d) => d.to_iso(),
        CellValue::Formula { cached, .. } => cached.as_deref().map(display).unwrap_or_default(),
    }
}

fn number(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Formula { cached, .. } => cached.as_deref().and_then(number),
        CellValue::DateTime(_) => None,
        other => other.as_f64(),
    }
}

fn condition_holds(op: FilterOperator, operand: &str, v: &CellValue) -> bool {
    use std::cmp::Ordering;
    let ord = match (number(v), operand.trim().parse::<f64>()) {
        (Some(a), Ok(b)) => a.partial_cmp(&b),
        (None, Ok(_)) if matches!(op, FilterOperator::Equal | FilterOperator::NotEqual) => None,
        (None, Ok(_)) => return false,
        _ => {
            let text = display(v);
            if matches!(op, FilterOperator::Equal | FilterOperator::NotEqual) {
                let hit = wildcard_match(operand, &text);
                return (op == FilterOperator::Equal) == hit;
            }
            Some(text.to_lowercase().cmp(&operand.to_lowercase()))
        }
    };
    match (op, ord) {
        (FilterOperator::Equal, o) => o == Some(Ordering::Equal),
        (FilterOperator::NotEqual, o) => o != Some(Ordering::Equal),
        (FilterOperator::LessThan, Some(o)) => o == Ordering::Less,
        (FilterOperator::LessThanOrEqual, Some(o)) => o != Ordering::Greater,
        (FilterOperator::GreaterThan, Some(o)) => o == Ordering::Greater,
        (FilterOperator::GreaterThanOrEqual, Some(o)) => o != Ordering::Less,
        _ => false,
    }
}

impl Worksheet<'_> {
    /// The auto filter of the sheet.
    pub fn auto_filter(&self) -> Option<AutoFilter> {
        let af = self.data.auto_filter.as_deref()?;
        let range = CellRange::parse(af.ref_.as_deref()?).ok()?;
        let columns = af
            .filter_column
            .iter()
            .filter_map(|c| Some((c.col_id?, ColumnFilter::from_ct(c)?)))
            .collect();
        let sort = af
            .sort_state
            .iter()
            .flat_map(|s| s.sort_condition.iter())
            .filter_map(|c| {
                let r = CellRange::parse(c.ref_.as_deref()?).ok()?;
                Some(SortKey {
                    column: r.start().col().checked_sub(range.start().col())?,
                    descending: c.descending.unwrap_or(false),
                })
            })
            .collect();
        Some(AutoFilter { range, columns, sort })
    }
}

impl WorksheetMut<'_> {
    /// Adds filter buttons to the header row of `range` (replacing an
    /// existing auto filter).
    pub fn set_auto_filter(&mut self, range: impl ToCellRange) -> Result<()> {
        let range = range.to_cell_range()?;
        for t in self.as_view().tables()? {
            if t.range.intersects(&range) {
                return Err(Error::InvalidArgument(format!(
                    "{range} overlaps table {}",
                    t.name
                )));
            }
        }
        self.data.auto_filter = Some(Box::new(sml::CT_AutoFilter {
            ref_: Some(range.to_string()),
            ..Default::default()
        }));
        set_local_name(
            self.workbook,
            FILTER_DATABASE,
            self.index,
            qualified_range(self.name, range),
            true,
        );
        *self.workbook_dirty = true;
        Ok(())
    }

    fn filter_range(&self) -> Result<CellRange> {
        self.data
            .auto_filter
            .as_ref()
            .and_then(|a| a.ref_.as_deref())
            .and_then(|r| CellRange::parse(r).ok())
            .ok_or_else(|| Error::NotFound("the sheet has no auto filter".into()))
    }

    /// Sets the criteria of the filter column at `offset` (0 = first column
    /// of the filter range).
    pub fn set_filter_column(&mut self, offset: u32, filter: ColumnFilter) -> Result<()> {
        let range = self.filter_range()?;
        if offset >= range.width() {
            return Err(Error::InvalidArgument(format!(
                "column offset {offset} is outside {range}"
            )));
        }
        let column = filter.to_ct(offset)?;
        let af = self.data.auto_filter.as_mut().expect("checked above");
        af.filter_column.retain(|c| c.col_id != Some(offset));
        let at = af
            .filter_column
            .iter()
            .position(|c| c.col_id.is_some_and(|id| id > offset))
            .unwrap_or(af.filter_column.len());
        af.filter_column.insert(at, column);
        self.data.sheet_pr.get_or_insert_with(Box::default).filter_mode = Some(true);
        Ok(())
    }

    /// Removes the criteria of a filter column. Returns whether it had criteria.
    pub fn clear_filter_column(&mut self, offset: u32) -> bool {
        let Some(af) = self.data.auto_filter.as_mut() else {
            return false;
        };
        let before = af.filter_column.len();
        af.filter_column.retain(|c| c.col_id != Some(offset));
        let removed = before != af.filter_column.len();
        if af.filter_column.is_empty()
            && let Some(pr) = self.data.sheet_pr.as_mut()
        {
            pr.filter_mode = None;
        }
        removed
    }

    /// Records the sort order of the filtered data (Excel shows it on the
    /// filter buttons; the rows are not reordered).
    pub fn set_sort(&mut self, keys: &[SortKey]) -> Result<()> {
        let range = self.filter_range()?;
        if range.height() < 2 {
            return Err(Error::InvalidArgument(
                "nothing to sort below the header row".into(),
            ));
        }
        let data = CellRange::new(
            CellRef::new(range.start().row() + 1, range.start().col())?,
            range.end(),
        );
        let mut conditions = Vec::new();
        for k in keys {
            if k.column >= range.width() {
                return Err(Error::InvalidArgument(format!(
                    "sort column offset {} is outside {range}",
                    k.column
                )));
            }
            let col = range.start().col() + k.column;
            let r = CellRange::new(
                CellRef::new(data.start().row(), col)?,
                CellRef::new(data.end().row(), col)?,
            );
            conditions.push(sml::CT_SortCondition {
                descending: k.descending.then_some(true),
                ref_: Some(r.to_string()),
                ..Default::default()
            });
        }
        let af = self.data.auto_filter.as_mut().expect("checked above");
        af.sort_state = (!conditions.is_empty()).then(|| {
            Box::new(sml::CT_SortState {
                ref_: Some(data.to_string()),
                sort_condition: conditions,
                ..Default::default()
            })
        });
        Ok(())
    }

    /// Removes the auto filter and shows the rows it hid.
    pub fn remove_auto_filter(&mut self) -> Result<bool> {
        let Ok(range) = self.filter_range() else {
            return Ok(false);
        };
        for r in range.start().row() + 1..=range.end().row() {
            self.set_row_hidden_flag(r, false);
        }
        self.data.auto_filter = None;
        if let Some(pr) = self.data.sheet_pr.as_mut() {
            pr.filter_mode = None;
        }
        if remove_local_name(self.workbook, FILTER_DATABASE, self.index) {
            *self.workbook_dirty = true;
        }
        Ok(true)
    }

    /// Hides the rows of the filter range that fail the criteria and shows
    /// the others. Returns the number of hidden rows.
    pub fn apply_auto_filter(&mut self) -> Result<usize> {
        let view = self.as_view();
        let Some(af) = view.auto_filter() else {
            return Err(Error::NotFound("the sheet has no auto filter".into()));
        };
        let first = af.range.start().row() + 1;
        let last = af.range.end().row();
        let mut visible: Vec<bool> = vec![true; (last + 1).saturating_sub(first) as usize];
        for (offset, filter) in &af.columns {
            let col = af.range.start().col() + offset;
            let values: Vec<CellValue> = (first..=last)
                .map(|r| view.cell((r, col)))
                .collect::<Result<_>>()?;
            let pass: Vec<bool> = match filter {
                ColumnFilter::Values {
                    values: accepted,
                    blanks,
                } => values
                    .iter()
                    .map(|v| {
                        let text = display(v);
                        if text.is_empty() {
                            *blanks
                        } else {
                            accepted.iter().any(|a| {
                                a.eq_ignore_ascii_case(&text) || a.to_lowercase() == text.to_lowercase()
                            })
                        }
                    })
                    .collect(),
                ColumnFilter::Custom { and, conditions } => values
                    .iter()
                    .map(|v| {
                        let mut results = conditions.iter().map(|(op, x)| condition_holds(*op, x, v));
                        if *and {
                            results.all(|b| b)
                        } else {
                            results.any(|b| b)
                        }
                    })
                    .collect(),
                ColumnFilter::Top {
                    count,
                    percent,
                    bottom,
                } => {
                    let mut nums: Vec<f64> = values.iter().filter_map(number).collect();
                    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    if !*bottom {
                        nums.reverse();
                    }
                    let n = if *percent {
                        ((nums.len() as f64) * count / 100.0).ceil() as usize
                    } else {
                        *count as usize
                    };
                    let cut = nums.get(n.min(nums.len()).saturating_sub(1)).copied();
                    values
                        .iter()
                        .map(|v| match (number(v), cut) {
                            (Some(x), Some(c)) if n > 0 => {
                                if *bottom {
                                    x <= c
                                } else {
                                    x >= c
                                }
                            }
                            _ => false,
                        })
                        .collect()
                }
            };
            for (v, p) in visible.iter_mut().zip(pass) {
                *v &= p;
            }
        }
        let mut hidden = 0;
        for (i, show) in visible.into_iter().enumerate() {
            let row = first + i as u32;
            self.set_row_hidden_flag(row, !show);
            hidden += usize::from(!show);
        }
        Ok(hidden)
    }

    /// Sets or clears the `hidden` flag of a row without creating empty rows needlessly.
    pub(crate) fn set_row_hidden_flag(&mut self, row: u32, hidden: bool) {
        if hidden {
            self.row_mut(row).hidden = Some(true);
            return;
        }
        let Some(data) = self.data.sheet_data.as_mut() else {
            return;
        };
        if let Ok(i) = data.row.binary_search_by_key(&row, |r| r.r.unwrap_or(0)) {
            data.row[i].hidden = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("a*", "Apple"));
        assert!(wildcard_match("*ple", "apple"));
        assert!(wildcard_match("a?ple", "ample"));
        assert!(!wildcard_match("a?ple", "aple"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("x~*", "x*"));
        assert!(!wildcard_match("x~*", "xy"));
    }

    #[test]
    fn conditions() {
        let n = CellValue::Number(5.0);
        let t = CellValue::Text("banana".into());
        assert!(condition_holds(FilterOperator::GreaterThan, "4", &n));
        assert!(!condition_holds(FilterOperator::GreaterThan, "5", &n));
        assert!(condition_holds(FilterOperator::GreaterThanOrEqual, "5", &n));
        assert!(condition_holds(FilterOperator::Equal, "b*", &t));
        assert!(condition_holds(FilterOperator::NotEqual, "a*", &t));
        assert!(
            !condition_holds(FilterOperator::LessThan, "3", &t),
            "text is not compared with numbers"
        );
        assert!(condition_holds(FilterOperator::LessThan, "cherry", &t));
    }

    #[test]
    fn filter_columns_round_trip() {
        let filters = [
            ColumnFilter::values(["a", "b"]),
            ColumnFilter::Values {
                values: vec![],
                blanks: true,
            },
            ColumnFilter::greater_than(3.0),
            ColumnFilter::between(1.0, 2.5),
            ColumnFilter::custom(FilterOperator::Equal, "x*"),
            ColumnFilter::top(5),
            ColumnFilter::Top {
                count: 10.0,
                percent: true,
                bottom: true,
            },
        ];
        for f in filters {
            let ct = f.to_ct(2).unwrap();
            assert_eq!(ColumnFilter::from_ct(&ct), Some(f));
        }
        assert!(
            ColumnFilter::Custom {
                and: true,
                conditions: vec![]
            }
            .to_ct(0)
            .is_err()
        );
    }
}
