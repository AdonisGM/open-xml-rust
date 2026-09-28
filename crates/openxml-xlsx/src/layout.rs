//! Row and column layout: hiding, outline groups, default sizes
//! (ECMA-376 Part 1 §18.3.1.13, §18.3.1.73, §18.3.1.81).
//!
//! ```
//! use openxml_xlsx::Workbook;
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.group_rows(2, 5, true)?; // a collapsed group; row 6 holds the summary
//! sheet.group_columns(3, 4, false)?;
//! sheet.set_column_hidden(8, true)?;
//! let view = sheet.as_view();
//! assert_eq!(view.row_outline_level(3), 1);
//! assert!(view.is_row_hidden(3));
//! assert!(view.is_column_hidden(8));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{MAX_COL, MAX_ROW};
use crate::worksheet::{Worksheet, WorksheetMut};

/// Stored width of a default (64 px) column.
const DEFAULT_COLUMN_WIDTH: f64 = 9.140625;

/// Deepest outline level Excel supports.
const MAX_OUTLINE: u8 = 7;

fn col_at(ws: &sml::CT_Worksheet, col: u32) -> Option<&sml::CT_Col> {
    ws.cols
        .iter()
        .flat_map(|c| c.col.iter())
        .find(|c| c.min.unwrap_or(0) <= col && col <= c.max.unwrap_or(0))
}

fn row_at(ws: &sml::CT_Worksheet, row: u32) -> Option<&sml::CT_Row> {
    let rows = &ws.sheet_data.as_ref()?.row;
    rows.binary_search_by_key(&row, |r| r.r.unwrap_or(0))
        .ok()
        .map(|i| &rows[i])
}

impl Worksheet<'_> {
    /// Whether a row is hidden.
    pub fn is_row_hidden(&self, row: u32) -> bool {
        row_at(self.data, row).is_some_and(|r| r.hidden == Some(true))
    }

    /// Whether a column is hidden.
    pub fn is_column_hidden(&self, col: u32) -> bool {
        col_at(self.data, col).is_some_and(|c| c.hidden == Some(true))
    }

    /// Outline (group) level of a row, 0 when ungrouped.
    pub fn row_outline_level(&self, row: u32) -> u8 {
        row_at(self.data, row).and_then(|r| r.outline_level).unwrap_or(0)
    }

    /// Outline (group) level of a column, 0 when ungrouped.
    pub fn column_outline_level(&self, col: u32) -> u8 {
        col_at(self.data, col).and_then(|c| c.outline_level).unwrap_or(0)
    }

    /// Whether the summary rows of groups are below them and the summary
    /// columns right of them (`(below, right)`, Excel's default `(true, true)`).
    pub fn outline_summary(&self) -> (bool, bool) {
        let pr = self.data.sheet_pr.as_ref().and_then(|p| p.outline_pr.as_ref());
        (
            pr.and_then(|p| p.summary_below).unwrap_or(true),
            pr.and_then(|p| p.summary_right).unwrap_or(true),
        )
    }

    /// Default row height in points.
    pub fn default_row_height(&self) -> f64 {
        self.data
            .sheet_format_pr
            .as_ref()
            .and_then(|f| f.default_row_height)
            .unwrap_or(15.0)
    }

    /// Default column width (characters), if set.
    pub fn default_column_width(&self) -> Option<f64> {
        self.data
            .sheet_format_pr
            .as_ref()
            .and_then(|f| f.default_col_width)
    }
}

impl WorksheetMut<'_> {
    /// Applies `f` to the column definitions of `first..=last`, splitting
    /// existing definitions and creating new ones for uncovered columns.
    pub(crate) fn update_cols(
        &mut self,
        first: u32,
        last: u32,
        mut f: impl FnMut(&mut sml::CT_Col),
    ) -> Result<()> {
        if first == 0 || first > last || last > MAX_COL {
            return Err(Error::InvalidArgument(format!("invalid columns {first}..{last}")));
        }
        let default_width = self
            .data
            .sheet_format_pr
            .as_ref()
            .and_then(|f| f.default_col_width)
            .unwrap_or(DEFAULT_COLUMN_WIDTH);
        if self.data.cols.is_empty() {
            self.data.cols.push(sml::CT_Cols::default());
        }
        // Several <cols> groups are legal; they are merged into the first.
        let mut all: Vec<sml::CT_Col> = Vec::new();
        for group in &mut self.data.cols {
            all.append(&mut group.col);
        }
        self.data.cols.truncate(1);
        let mut out = Vec::with_capacity(all.len() + 2);
        let mut covered = Vec::new();
        for c in all {
            let (min, max) = (c.min.unwrap_or(0), c.max.unwrap_or(0));
            if max < first || min > last {
                out.push(c);
                continue;
            }
            if min < first {
                out.push(sml::CT_Col {
                    max: Some(first - 1),
                    ..c.clone()
                });
            }
            if max > last {
                out.push(sml::CT_Col {
                    min: Some(last + 1),
                    ..c.clone()
                });
            }
            let (a, b) = (min.max(first), max.min(last));
            let mut mid = sml::CT_Col {
                min: Some(a),
                max: Some(b),
                ..c
            };
            f(&mut mid);
            out.push(mid);
            covered.push((a, b));
        }
        covered.sort_unstable();
        let mut next = first;
        let mut gaps = Vec::new();
        for (a, b) in covered {
            if a > next {
                gaps.push((next, a - 1));
            }
            next = next.max(b + 1);
        }
        if next <= last {
            gaps.push((next, last));
        }
        for (a, b) in gaps {
            let mut c = sml::CT_Col {
                min: Some(a),
                max: Some(b),
                width: Some(default_width),
                ..Default::default()
            };
            f(&mut c);
            out.push(c);
        }
        out.sort_by_key(|c| c.min);
        self.data.cols[0].col = out;
        Ok(())
    }

    /// Sets the width of columns `first..=last` (characters of the default font).
    pub fn set_columns_width(&mut self, first: u32, last: u32, width: f64) -> Result<()> {
        if !(0.0..=255.0).contains(&width) {
            return Err(Error::InvalidArgument(format!(
                "column width out of range: {width}"
            )));
        }
        self.update_cols(first, last, |c| {
            c.width = Some(width);
            c.custom_width = Some(true);
        })
    }

    /// Hides or shows a column.
    pub fn set_column_hidden(&mut self, col: u32, hidden: bool) -> Result<()> {
        self.set_columns_hidden(col, col, hidden)
    }

    /// Hides or shows columns `first..=last`.
    pub fn set_columns_hidden(&mut self, first: u32, last: u32, hidden: bool) -> Result<()> {
        self.update_cols(first, last, |c| c.hidden = hidden.then_some(true))
    }

    /// Hides or shows a row.
    pub fn set_row_hidden(&mut self, row: u32, hidden: bool) -> Result<()> {
        if !(1..=MAX_ROW).contains(&row) {
            return Err(Error::InvalidArgument(format!("row out of range: {row}")));
        }
        self.set_row_hidden_flag(row, hidden);
        Ok(())
    }

    /// Sets the default row height in points.
    pub fn set_default_row_height(&mut self, points: f64) -> Result<()> {
        if !(0.0..=409.0).contains(&points) {
            return Err(Error::InvalidArgument(format!(
                "row height out of range: {points}"
            )));
        }
        let f = self.data.sheet_format_pr.get_or_insert_with(Box::default);
        f.default_row_height = Some(points);
        f.custom_height = (points != 15.0).then_some(true);
        Ok(())
    }

    /// Sets the width of columns without an explicit width.
    pub fn set_default_column_width(&mut self, width: f64) -> Result<()> {
        if !(0.0..=255.0).contains(&width) {
            return Err(Error::InvalidArgument(format!(
                "column width out of range: {width}"
            )));
        }
        let f = self.data.sheet_format_pr.get_or_insert_with(Box::default);
        f.default_col_width = Some(width);
        if f.default_row_height.is_none() {
            f.default_row_height = Some(15.0);
        }
        Ok(())
    }

    /// Where group summaries are: below the rows / right of the columns.
    pub fn set_outline_summary(&mut self, below: bool, right: bool) {
        let pr = self.data.sheet_pr.get_or_insert_with(Box::default);
        let o = pr.outline_pr.get_or_insert_with(Box::default);
        o.summary_below = (!below).then_some(false);
        o.summary_right = (!right).then_some(false);
    }

    fn refresh_outline_levels(&mut self) {
        let rows = self
            .data
            .sheet_data
            .iter()
            .flat_map(|d| d.row.iter())
            .filter_map(|r| r.outline_level)
            .max()
            .unwrap_or(0);
        let cols = self
            .data
            .cols
            .iter()
            .flat_map(|c| c.col.iter())
            .filter_map(|c| c.outline_level)
            .max()
            .unwrap_or(0);
        if rows == 0 && cols == 0 && self.data.sheet_format_pr.is_none() {
            return;
        }
        let f = self.data.sheet_format_pr.get_or_insert_with(Box::default);
        if f.default_row_height.is_none() {
            f.default_row_height = Some(15.0);
        }
        f.outline_level_row = (rows > 0).then_some(rows);
        f.outline_level_col = (cols > 0).then_some(cols);
    }

    /// Groups rows `first..=last` one outline level deeper. A collapsed
    /// group hides its rows and marks the summary row as collapsed.
    pub fn group_rows(&mut self, first: u32, last: u32, collapsed: bool) -> Result<()> {
        if first == 0 || first > last || last > MAX_ROW {
            return Err(Error::InvalidArgument(format!("invalid rows {first}..{last}")));
        }
        let view = self.as_view();
        if (first..=last).any(|r| view.row_outline_level(r) >= MAX_OUTLINE) {
            return Err(Error::InvalidArgument("outlines are limited to 7 levels".into()));
        }
        let (below, _) = view.outline_summary();
        for r in first..=last {
            let row = self.row_mut(r);
            row.outline_level = Some(row.outline_level.unwrap_or(0) + 1);
            if collapsed {
                row.hidden = Some(true);
            }
        }
        if collapsed {
            let summary = if below {
                last.checked_add(1)
            } else {
                first.checked_sub(1)
            };
            if let Some(s) = summary.filter(|s| (1..=MAX_ROW).contains(s)) {
                self.row_mut(s).collapsed = Some(true);
            }
        }
        self.refresh_outline_levels();
        Ok(())
    }

    /// Removes one outline level from rows `first..=last` and shows them.
    pub fn ungroup_rows(&mut self, first: u32, last: u32) -> Result<()> {
        if first == 0 || first > last || last > MAX_ROW {
            return Err(Error::InvalidArgument(format!("invalid rows {first}..{last}")));
        }
        let (below, _) = self.as_view().outline_summary();
        if let Some(data) = self.data.sheet_data.as_mut() {
            for row in data.row.iter_mut() {
                let r = row.r.unwrap_or(0);
                if (first..=last).contains(&r) {
                    row.outline_level = row
                        .outline_level
                        .and_then(|l| l.checked_sub(1))
                        .filter(|&l| l > 0);
                    row.hidden = None;
                }
                let summary = if below { last + 1 } else { first.saturating_sub(1) };
                if r == summary {
                    row.collapsed = None;
                }
            }
        }
        self.refresh_outline_levels();
        Ok(())
    }

    /// Groups columns `first..=last` one outline level deeper (see [`WorksheetMut::group_rows`]).
    pub fn group_columns(&mut self, first: u32, last: u32, collapsed: bool) -> Result<()> {
        let view = self.as_view();
        if (first..=last.min(MAX_COL)).any(|c| view.column_outline_level(c) >= MAX_OUTLINE) {
            return Err(Error::InvalidArgument("outlines are limited to 7 levels".into()));
        }
        let (_, right) = view.outline_summary();
        self.update_cols(first, last, |c| {
            c.outline_level = Some(c.outline_level.unwrap_or(0) + 1);
            if collapsed {
                c.hidden = Some(true);
            }
        })?;
        if collapsed {
            let summary = if right {
                last.checked_add(1)
            } else {
                first.checked_sub(1)
            };
            if let Some(s) = summary.filter(|s| (1..=MAX_COL).contains(s)) {
                self.update_cols(s, s, |c| c.collapsed = Some(true))?;
            }
        }
        self.refresh_outline_levels();
        Ok(())
    }

    /// Removes one outline level from columns `first..=last` and shows them.
    pub fn ungroup_columns(&mut self, first: u32, last: u32) -> Result<()> {
        self.update_cols(first, last, |c| {
            c.outline_level = c.outline_level.and_then(|l| l.checked_sub(1)).filter(|&l| l > 0);
            c.hidden = None;
        })?;
        let (_, right) = self.as_view().outline_summary();
        let summary = if right { last + 1 } else { first.saturating_sub(1) };
        if (1..=MAX_COL).contains(&summary) && col_at(self.data, summary).is_some() {
            self.update_cols(summary, summary, |c| c.collapsed = None)?;
        }
        self.refresh_outline_levels();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Workbook;

    #[test]
    fn column_definitions_are_split_and_created() {
        let mut wb = Workbook::new();
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_columns_width(2, 6, 20.0).unwrap();
        s.set_column_hidden(4, true).unwrap();
        s.set_columns_hidden(8, 9, true).unwrap();
        let cols: Vec<(u32, u32, Option<f64>, Option<bool>)> = s.raw_mut().cols[0]
            .col
            .iter()
            .map(|c| (c.min.unwrap(), c.max.unwrap(), c.width, c.hidden))
            .collect();
        assert_eq!(
            cols,
            [
                (2, 3, Some(20.0), None),
                (4, 4, Some(20.0), Some(true)),
                (5, 6, Some(20.0), None),
                (8, 9, Some(super::DEFAULT_COLUMN_WIDTH), Some(true)),
            ]
        );
        assert!(s.update_cols(3, 2, |_| {}).is_err());
    }

    #[test]
    fn outline_levels_are_tracked() {
        let mut wb = Workbook::new();
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.group_rows(2, 9, false).unwrap();
        s.group_rows(3, 4, true).unwrap();
        let v = s.as_view();
        assert_eq!(v.row_outline_level(3), 2);
        assert_eq!(v.row_outline_level(9), 1);
        assert!(v.is_row_hidden(4) && !v.is_row_hidden(5));
        assert_eq!(
            s.raw_mut().sheet_format_pr.as_ref().unwrap().outline_level_row,
            Some(2)
        );
        s.ungroup_rows(3, 4).unwrap();
        assert!(!s.as_view().is_row_hidden(4));
        assert_eq!(
            s.raw_mut().sheet_format_pr.as_ref().unwrap().outline_level_row,
            Some(1)
        );
        for _ in 0..6 {
            s.group_columns(2, 3, false).unwrap();
        }
        assert!(s.group_columns(2, 2, false).is_ok());
        assert!(s.group_columns(2, 2, false).is_err(), "level 8 is refused");
        assert_eq!(s.as_view().column_outline_level(2), 7);
        s.set_outline_summary(false, true);
        assert_eq!(s.as_view().outline_summary(), (false, true));
    }
}
