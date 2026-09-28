//! Inserting and deleting rows and columns, and keeping references valid
//! when sheets are renamed or removed.
//!
//! Inserting or deleting rows moves the cells below (or columns to the
//! right) and updates what refers to them: formulas on every sheet, merged
//! ranges, hyperlinks, data validations, conditional formats, the auto
//! filter, tables, comments, pictures, page breaks, column definitions and
//! defined names (print area and titles included). References into deleted
//! cells become `#REF!`, as in Excel.
//!
//! Limitations: inserted rows and columns are unformatted (Excel copies
//! the formatting of the neighbouring line); columns cannot be inserted or
//! deleted inside a table; formulas in chart parts are not updated.

use openxml_core::{Error, Result};
use openxml_opc::known::rel_types;
use openxml_schema::sml;

use crate::cell_ref::{CellRange, CellRef, MAX_COL, MAX_ROW};
use crate::comments::{remap_comments, remap_notes};
use crate::drawing::remap_anchor_markers;
use crate::formula::{Axis, Edit, adjust_formula, mentions_sheet, rename_sheet_in_formula};
use crate::table::sheet_tables;
use crate::util::{from_sqref, remove_if_orphan, remove_relationship_and_orphans, to_sqref};
use crate::value::encode_xstring;
use crate::workbook::Workbook;
use crate::worksheet::WorksheetMut;

/// A range after an edit; `None` when all of it was deleted.
fn map_range(r: CellRange, e: &Edit) -> Option<CellRange> {
    let (s, t) = (r.start(), r.end());
    match e.axis {
        Axis::Rows => {
            let (a, b) = e.map_span(s.row(), t.row())?;
            Some(CellRange::new(
                CellRef::new(a, s.col()).ok()?,
                CellRef::new(b, t.col()).ok()?,
            ))
        }
        Axis::Columns => {
            let (a, b) = e.map_span(s.col(), t.col())?;
            Some(CellRange::new(
                CellRef::new(s.row(), a).ok()?,
                CellRef::new(t.row(), b).ok()?,
            ))
        }
    }
}

fn map_ref_text(text: &mut Option<String>, e: &Edit) -> bool {
    let Some(r) = text.as_deref().and_then(|t| CellRange::parse(t).ok()) else {
        return true;
    };
    match map_range(r, e) {
        Some(n) => {
            *text = Some(n.to_string());
            true
        }
        None => false,
    }
}

fn map_sqref(list: &mut Option<openxml_xml::XmlList<String>>, e: &Edit) -> bool {
    let Some(l) = list.as_ref() else {
        return false;
    };
    let kept: Vec<CellRange> = from_sqref(l)
        .into_iter()
        .filter_map(|r| map_range(r, e))
        .collect();
    *list = Some(to_sqref(&kept));
    !kept.is_empty()
}

impl WorksheetMut<'_> {
    /// Replaces shared formulas by ordinary ones (their text depends on
    /// the distance to the master cell, which structural edits change).
    fn unshare_formulas(&mut self) -> Result<()> {
        let shared = |c: &sml::CT_Cell| {
            c.f.as_deref()
                .is_some_and(|f| f.t == Some(sml::ST_CellFormulaType::Shared))
        };
        let cells: Vec<CellRef> = self
            .data
            .sheet_data
            .iter()
            .flat_map(|d| d.row.iter())
            .flat_map(|r| r.c.iter())
            .filter(|c| shared(c))
            .filter_map(|c| CellRef::parse(c.r.as_deref()?).ok())
            .collect();
        if cells.is_empty() {
            return Ok(());
        }
        let view = self.as_view();
        let texts: Vec<(CellRef, Option<String>)> = cells
            .into_iter()
            .map(|r| Ok((r, view.cell(r)?.as_formula().map(str::to_owned))))
            .collect::<Result<_>>()?;
        for (r, text) in texts {
            let cell = self.cell_mut(r);
            match text {
                Some(t) => {
                    cell.f = Some(Box::new(sml::CT_CellFormula {
                        value: encode_xstring(&t).into_owned(),
                        ..Default::default()
                    }))
                }
                None => cell.f = None,
            }
        }
        Ok(())
    }

    /// Updates the formulas of this sheet (cells, validations, conditional
    /// formats) for an edit on sheet `target`. Returns whether any changed.
    pub(crate) fn adjust_formulas(&mut self, target: &str, e: &Edit) -> bool {
        let host = self.name.to_owned();
        let own = host.eq_ignore_ascii_case(target);
        let mut changed = false;
        let mut fix = |text: &mut String| {
            if !own && !mentions_sheet(text, target) {
                return;
            }
            let new = adjust_formula(text, Some(&host), target, e);
            if new != *text {
                *text = new;
                changed = true;
            }
        };
        for row in self.data.sheet_data.iter_mut().flat_map(|d| d.row.iter_mut()) {
            for c in &mut row.c {
                if let Some(f) = c.f.as_deref_mut() {
                    fix(&mut f.value);
                }
            }
        }
        for dv in self
            .data
            .data_validations
            .iter_mut()
            .flat_map(|d| d.data_validation.iter_mut())
        {
            for f in [&mut dv.formula1, &mut dv.formula2].into_iter().flatten() {
                fix(f);
            }
        }
        for cf in &mut self.data.conditional_formatting {
            for rule in &mut cf.cf_rule {
                for f in &mut rule.formula {
                    fix(f);
                }
            }
        }
        changed
    }

    /// Renames sheet references in this sheet's formulas. Returns whether any changed.
    pub(crate) fn rename_references(&mut self, old: &str, new: Option<&str>) -> bool {
        let mut changed = false;
        let mut fix = |text: &mut String| {
            if !mentions_sheet(text, old) {
                return;
            }
            let n = rename_sheet_in_formula(text, old, new);
            if n != *text {
                *text = n;
                changed = true;
            }
        };
        for row in self.data.sheet_data.iter_mut().flat_map(|d| d.row.iter_mut()) {
            for c in &mut row.c {
                if let Some(f) = c.f.as_deref_mut() {
                    fix(&mut f.value);
                }
            }
        }
        for dv in self
            .data
            .data_validations
            .iter_mut()
            .flat_map(|d| d.data_validation.iter_mut())
        {
            for f in [&mut dv.formula1, &mut dv.formula2].into_iter().flatten() {
                fix(f);
            }
        }
        for cf in &mut self.data.conditional_formatting {
            for rule in &mut cf.cf_rule {
                for f in &mut rule.formula {
                    fix(f);
                }
            }
        }
        changed
    }

    /// Checks that an edit is possible before anything changes.
    fn check_edit(&self, e: &Edit) -> Result<()> {
        let view = self.as_view();
        if e.insert {
            let limit = match e.axis {
                Axis::Rows => MAX_ROW,
                Axis::Columns => MAX_COL,
            };
            let lost = view.used_range().is_some_and(|u| {
                let last = match e.axis {
                    Axis::Rows => u.end().row(),
                    Axis::Columns => u.end().col(),
                };
                last >= e.at && last > limit - e.count
            });
            if lost {
                return Err(Error::InvalidArgument(
                    "the insertion would push cells off the sheet".into(),
                ));
            }
        }
        let end = e.at + e.count - 1;
        for t in view.tables()? {
            let r = t.range;
            match e.axis {
                Axis::Columns => {
                    let (a, b) = (r.start().col(), r.end().col());
                    let inside = if e.insert {
                        a < e.at && e.at <= b
                    } else {
                        e.at <= b && end >= a
                    };
                    if inside {
                        return Err(Error::InvalidArgument(format!(
                            "columns cannot be inserted into or deleted from table {}",
                            t.name
                        )));
                    }
                }
                Axis::Rows if !e.insert => {
                    let (a, b) = (r.start().row(), r.end().row());
                    let header_hit = t.header_row && e.at <= a && a <= end;
                    let data_first = a + u32::from(t.header_row);
                    let data_last = b - u32::from(t.totals_row);
                    let all_data = e.at <= data_first && end >= data_last;
                    let totals_hit = t.totals_row && e.at <= b && b <= end;
                    if (header_hit || all_data || totals_hit) && !(e.at <= a && end >= b) {
                        return Err(Error::InvalidArgument(format!(
                            "the rows would break table {} (header, totals or all data rows)",
                            t.name
                        )));
                    }
                    if e.at <= a && end >= b {
                        return Err(Error::InvalidArgument(format!(
                            "the rows contain the whole table {}; remove it first",
                            t.name
                        )));
                    }
                }
                Axis::Rows => {}
            }
        }
        Ok(())
    }

    /// Moves the content of this sheet for an insertion or deletion.
    fn apply_edit(&mut self, e: &Edit) -> Result<()> {
        self.unshare_formulas()?;
        let name = self.name.to_owned();
        self.adjust_formulas(&name, e);

        // Cells.
        if let Some(data) = self.data.sheet_data.as_mut() {
            match e.axis {
                Axis::Rows => data.row.retain_mut(|row| {
                    let r = row.r.unwrap_or(0);
                    let Some(n) = e.map(r) else {
                        return false;
                    };
                    if n != r {
                        row.r = Some(n);
                        for c in &mut row.c {
                            if let Some(cr) = c.r.as_deref().and_then(|s| CellRef::parse(s).ok())
                                && let Ok(m) = CellRef::new(n, cr.col())
                            {
                                c.r = Some(m.to_string());
                            }
                        }
                    }
                    true
                }),
                Axis::Columns => {
                    for row in &mut data.row {
                        row.c.retain_mut(|c| {
                            let Some(cr) = c.r.as_deref().and_then(|s| CellRef::parse(s).ok()) else {
                                return true;
                            };
                            match e.map(cr.col()).and_then(|n| CellRef::new(cr.row(), n).ok()) {
                                Some(m) => {
                                    c.r = Some(m.to_string());
                                    true
                                }
                                None => false,
                            }
                        });
                        row.spans = None;
                    }
                }
            }
            for row in &mut data.row {
                for c in &mut row.c {
                    if let Some(f) = c.f.as_deref_mut()
                        && f.t == Some(sml::ST_CellFormulaType::Array)
                    {
                        map_ref_text(&mut f.ref_, e);
                    }
                }
            }
        }

        // Merged ranges.
        if let Some(m) = self.data.merge_cells.as_mut() {
            m.merge_cell.retain_mut(|mc| {
                map_ref_text(&mut mc.ref_, e)
                    && mc
                        .ref_
                        .as_deref()
                        .and_then(|r| CellRange::parse(r).ok())
                        .is_some_and(|r| r.len() > 1)
            });
        }

        // Hyperlinks: drop the deleted ones (with their relationships), move the rest.
        self.remove_hyperlinks_where(|r| map_range(r, e).is_none());
        if let Some(h) = self.data.hyperlinks.as_mut() {
            for link in &mut h.hyperlink {
                map_ref_text(&mut link.ref_, e);
            }
        }

        // Validations and conditional formats.
        if let Some(dvs) = self.data.data_validations.as_mut() {
            dvs.data_validation.retain_mut(|dv| map_sqref(&mut dv.sqref, e));
            dvs.count = Some(dvs.data_validation.len() as u32);
            if dvs.data_validation.is_empty() {
                self.data.data_validations = None;
            }
        }
        self.data
            .conditional_formatting
            .retain_mut(|cf| map_sqref(&mut cf.sqref, e));

        // Auto filter.
        let mut filter_gone = false;
        if let Some(af) = self.data.auto_filter.as_mut() {
            let old = af.ref_.as_deref().and_then(|r| CellRange::parse(r).ok());
            if map_ref_text(&mut af.ref_, e) {
                let new = af.ref_.as_deref().and_then(|r| CellRange::parse(r).ok());
                if let (Some(old), Some(new), Axis::Columns) = (old, new, e.axis) {
                    af.filter_column.retain_mut(|fc| {
                        let Some(id) = fc.col_id else { return true };
                        match e.map(old.start().col() + id) {
                            Some(c) => {
                                fc.col_id = Some(c - new.start().col());
                                true
                            }
                            None => false,
                        }
                    });
                }
                if let Some(s) = af.sort_state.as_mut() {
                    map_ref_text(&mut s.ref_, e);
                    s.sort_condition.retain_mut(|c| map_ref_text(&mut c.ref_, e));
                }
            } else {
                filter_gone = true;
            }
        }
        if filter_gone {
            self.data.auto_filter = None;
        }

        // Column definitions and page breaks.
        if e.axis == Axis::Columns {
            for group in &mut self.data.cols {
                group.col.retain_mut(|c| {
                    let (Some(a), Some(b)) = (c.min, c.max) else {
                        return true;
                    };
                    match e.map_span(a, b) {
                        Some((a2, b2)) => {
                            c.min = Some(a2);
                            c.max = Some(b2);
                            true
                        }
                        None => false,
                    }
                });
            }
            self.data.cols.retain(|g| !g.col.is_empty());
        }
        let breaks = match e.axis {
            Axis::Rows => &mut self.data.row_breaks,
            Axis::Columns => &mut self.data.col_breaks,
        };
        if let Some(b) = breaks.as_mut() {
            b.brk.retain_mut(|brk| match brk.id.and_then(|id| e.map(id)) {
                Some(n) => {
                    brk.id = Some(n);
                    true
                }
                None => false,
            });
            b.count = Some(b.brk.len() as u32);
            b.manual_break_count = Some(b.brk.iter().filter(|x| x.man == Some(true)).count() as u32);
            if b.brk.is_empty() {
                *breaks = None;
            }
        }

        // Dimension.
        if self.data.dimension.is_some() {
            let used =
                crate::worksheet::used_range(self.data).map_or_else(|| "A1".to_owned(), |r| r.to_string());
            self.data.dimension.get_or_insert_with(Box::default).ref_ = Some(used);
        }

        // Tables.
        for (_, part) in sheet_tables(self.data, self.package, self.part) {
            let t = self.side.table(self.package, &part)?;
            map_ref_text(&mut t.ref_, e);
            if let Some(af) = t.auto_filter.as_mut() {
                map_ref_text(&mut af.ref_, e);
            }
            if let Some(s) = t.sort_state.as_mut() {
                map_ref_text(&mut s.ref_, e);
                s.sort_condition.retain_mut(|c| map_ref_text(&mut c.ref_, e));
            }
        }

        // Comments, notes and pictures.
        let rows = e.axis == Axis::Rows;
        let map = |i: u32| e.map(i);
        if let Some(part) = self.as_view().comments_part() {
            remap_comments(self.side.comments(self.package, &part)?, rows, &map);
        }
        if let Some(rid) = self.data.legacy_drawing.as_ref().and_then(|l| l.r_id.clone())
            && let Some(part) = self.package.relationship_target(Some(self.part), &rid)
            && self.package.contains(&part)
        {
            remap_notes(self.side.vml(self.package, &part)?, rows, &map);
        }
        if let Some(part) = self.as_view().drawing_part() {
            remap_anchor_markers(self.side.drawing(self.package, &part)?, rows, &map);
        }
        Ok(())
    }
}

impl Workbook {
    /// Inserts `count` empty rows before row `at` of a worksheet.
    ///
    /// Cells below move down, and every reference to them follows:
    /// formulas on all sheets, defined names (print area and titles
    /// included), merged ranges, hyperlinks, data validations, conditional
    /// formats, the auto filter, tables, comments, pictures and page
    /// breaks. Ranges spanning the insertion point grow. The calculation
    /// chain is dropped (Excel rebuilds it). Inserted rows are unformatted,
    /// and formulas inside chart parts are not updated.
    ///
    /// ```
    /// use openxml_xlsx::Workbook;
    ///
    /// let mut wb = Workbook::new();
    /// {
    ///     let mut s = wb.worksheet_mut("Sheet1")?;
    ///     s.set_value("A1", 1.0)?;
    ///     s.set_value("A2", 2.0)?;
    ///     s.set_formula("A3", "SUM(A1:A2)")?;
    /// }
    /// wb.insert_rows("Sheet1", 2, 3)?;
    /// let s = wb.worksheet("Sheet1")?;
    /// assert_eq!(s.cell("A5")?.as_f64(), Some(2.0));
    /// assert_eq!(s.cell("A6")?.as_formula(), Some("SUM(A1:A5)"));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn insert_rows(&mut self, sheet: &str, at: u32, count: u32) -> Result<()> {
        self.structural_edit(sheet, Axis::Rows, at, count, true)
    }

    /// Deletes rows `at..at + count` of a worksheet.
    pub fn delete_rows(&mut self, sheet: &str, at: u32, count: u32) -> Result<()> {
        self.structural_edit(sheet, Axis::Rows, at, count, false)
    }

    /// Inserts `count` empty columns before column `at` (1 = A).
    pub fn insert_columns(&mut self, sheet: &str, at: u32, count: u32) -> Result<()> {
        self.structural_edit(sheet, Axis::Columns, at, count, true)
    }

    /// Deletes columns `at..at + count` (1 = A).
    pub fn delete_columns(&mut self, sheet: &str, at: u32, count: u32) -> Result<()> {
        self.structural_edit(sheet, Axis::Columns, at, count, false)
    }

    fn structural_edit(&mut self, sheet: &str, axis: Axis, at: u32, count: u32, insert: bool) -> Result<()> {
        let limit = match axis {
            Axis::Rows => MAX_ROW,
            Axis::Columns => MAX_COL,
        };
        if count == 0 || at == 0 || at > limit || (!insert && at + count - 1 > limit) || count > limit {
            return Err(Error::InvalidArgument(format!(
                "invalid position {at} or count {count}"
            )));
        }
        let e = Edit {
            axis,
            at,
            count,
            insert,
        };
        let idx = self.worksheet_index(sheet)?;
        let name = self.sheets[idx].name.clone();
        let was_dirty = self.sheets[idx].dirty;
        if let Err(err) = self.view_mut(idx)?.check_edit(&e) {
            self.sheets[idx].dirty = was_dirty;
            return Err(err);
        }
        self.view_mut(idx)?.apply_edit(&e)?;
        for i in self.worksheet_positions() {
            if i == idx {
                continue;
            }
            let was_dirty = self.sheets[i].dirty;
            let changed = self.view_mut(i)?.adjust_formulas(&name, &e);
            if !changed {
                self.sheets[i].dirty = was_dirty;
            }
        }
        self.adjust_defined_names(|f| adjust_formula(f, None, &name, &e));
        self.drop_calc_chain();
        Ok(())
    }

    /// Rewrites every defined name with `f`; marks the workbook changed if any differ.
    pub(crate) fn adjust_defined_names(&mut self, f: impl Fn(&str) -> String) {
        let Some(names) = self.workbook.defined_names.as_mut() else {
            return;
        };
        let mut changed = false;
        for n in &mut names.defined_name {
            let new = f(&n.value);
            if new != n.value {
                n.value = new;
                changed = true;
            }
        }
        self.workbook_dirty |= changed;
    }

    /// Removes the calculation chain (it lists formula cells by position;
    /// Excel rebuilds it).
    pub(crate) fn drop_calc_chain(&mut self) {
        let part = self.workbook_part().clone();
        let rel = self
            .package
            .relationships(Some(&part))
            .and_then(|r| r.first_by_type(rel_types::CALC_CHAIN))
            .map(|r| r.id.clone());
        if let Some(id) = rel {
            for gone in remove_relationship_and_orphans(&mut self.package, &part, &id) {
                self.side.forget(&gone);
            }
        }
    }

    /// Updates references to a renamed (or, with `None`, removed) sheet in
    /// every worksheet and defined name.
    pub(crate) fn rename_references(&mut self, old: &str, new: Option<&str>) -> Result<()> {
        for i in self.worksheet_positions() {
            let was_dirty = self.sheets[i].dirty;
            let changed = self.view_mut(i)?.rename_references(old, new);
            if !changed {
                self.sheets[i].dirty = was_dirty;
            }
        }
        self.adjust_defined_names(|f| {
            if mentions_sheet(f, old) {
                rename_sheet_in_formula(f, old, new)
            } else {
                f.to_owned()
            }
        });
        Ok(())
    }

    /// Removes the parts only a removed sheet used (drawings, comments,
    /// tables, images, …).
    pub(crate) fn remove_orphans_of(&mut self, targets: Vec<openxml_opc::PartName>) {
        let mut removed = Vec::new();
        for t in targets {
            remove_if_orphan(&mut self.package, &t, &mut removed);
        }
        for p in removed {
            self.side.forget(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_follow_edits() {
        let r = CellRange::parse("B2:D6").unwrap();
        let rows = |at, count, insert| Edit {
            axis: Axis::Rows,
            at,
            count,
            insert,
        };
        let cols = |at, count, insert| Edit {
            axis: Axis::Columns,
            at,
            count,
            insert,
        };
        let text = |e: Edit| map_range(r, &e).map(|r| r.to_string());
        assert_eq!(text(rows(4, 2, true)).as_deref(), Some("B2:D8"));
        assert_eq!(text(rows(1, 1, true)).as_deref(), Some("B3:D7"));
        assert_eq!(text(rows(3, 2, false)).as_deref(), Some("B2:D4"));
        assert_eq!(text(rows(1, 10, false)), None);
        assert_eq!(text(cols(1, 1, false)).as_deref(), Some("A2:C6"));
        assert_eq!(text(cols(2, 3, false)), None);
        let mut sqref = Some(to_sqref(&[r, CellRange::parse("F1").unwrap()]));
        assert!(map_sqref(&mut sqref, &cols(6, 1, false)));
        assert_eq!(sqref.unwrap().0, ["B2:D6"]);
        let mut text_ref = Some("Z9".to_owned());
        assert!(!map_ref_text(&mut text_ref, &rows(9, 1, false)));
    }
}
