//! Workbook-level settings: sheet visibility and order, calculation
//! properties and named cell styles.
//!
//! ```
//! use openxml_xlsx::{Workbook, SheetVisibility, CalcProperties, CellStyle};
//!
//! let mut wb = Workbook::new();
//! wb.add_worksheet("Data")?;
//! wb.add_worksheet("Lookup")?;
//! wb.set_sheet_visibility("Lookup", SheetVisibility::VeryHidden)?;
//! wb.move_sheet("Data", 0)?;
//! assert_eq!(wb.sheet_names(), ["Data", "Sheet1", "Lookup"]);
//! wb.set_calc_properties(&CalcProperties { full_calc_on_load: true, ..wb.calc_properties() });
//! let title = wb.add_named_style("Report title", &CellStyle::new().bold().font_size(16.0))?;
//! assert_eq!(wb.style_name(title).as_deref(), Some("Report title"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::styles::{CellStyle, StyleId};
use crate::workbook::Workbook;
use crate::worksheet::SheetKind;

pub use sml::{ST_CalcMode as CalcMode, ST_SheetState as SheetVisibility};

/// Calculation settings of the workbook (§18.2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct CalcProperties {
    /// Automatic or manual calculation.
    pub mode: CalcMode,
    /// Recalculate every formula when the file is opened.
    pub full_calc_on_load: bool,
    /// Allow circular references, iterating up to `iterate_count` times.
    pub iterate: bool,
    /// Maximum iterations (Excel's default 100).
    pub iterate_count: u32,
    /// Maximum change between iterations (Excel's default 0.001).
    pub iterate_delta: f64,
    /// Calculate with full precision (not as displayed).
    pub full_precision: bool,
}

impl Default for CalcProperties {
    fn default() -> Self {
        CalcProperties {
            mode: CalcMode::Auto,
            full_calc_on_load: false,
            iterate: false,
            iterate_count: 100,
            iterate_delta: 0.001,
            full_precision: true,
        }
    }
}

impl Workbook {
    /// Visibility of a sheet.
    pub fn sheet_visibility(&self, name: &str) -> Result<SheetVisibility> {
        let i = self.index_of(name)?;
        Ok(self
            .sheet_record(i)
            .and_then(|s| s.state)
            .unwrap_or(SheetVisibility::Visible))
    }

    pub(crate) fn sheet_record(&self, index: usize) -> Option<&sml::CT_Sheet> {
        let rel_id = &self.sheets.get(index)?.rel_id;
        self.workbook
            .sheets
            .as_ref()?
            .sheet
            .iter()
            .find(|s| s.r_id.as_deref() == Some(rel_id.as_str()))
    }

    /// Shows or hides a sheet. `VeryHidden` sheets can only be shown again
    /// programmatically. At least one sheet must stay visible; hiding the
    /// active sheet activates the first visible one.
    pub fn set_sheet_visibility(&mut self, name: &str, visibility: SheetVisibility) -> Result<()> {
        let i = self.index_of(name)?;
        if visibility != SheetVisibility::Visible {
            let others_visible = (0..self.sheets.len()).any(|j| {
                j != i
                    && self
                        .sheet_record(j)
                        .and_then(|s| s.state)
                        .unwrap_or(SheetVisibility::Visible)
                        == SheetVisibility::Visible
            });
            if !others_visible {
                return Err(Error::InvalidArgument(
                    "a workbook must keep at least one visible sheet".into(),
                ));
            }
        }
        let rel_id = self.sheets[i].rel_id.clone();
        if let Some(s) = self.workbook.sheets.as_mut().and_then(|s| {
            s.sheet
                .iter_mut()
                .find(|s| s.r_id.as_deref() == Some(rel_id.as_str()))
        }) {
            s.state = (visibility != SheetVisibility::Visible).then_some(visibility);
        }
        self.workbook_dirty = true;
        if visibility != SheetVisibility::Visible && self.active_sheet() == i {
            let first_visible = (0..self.sheets.len())
                .find(|&j| {
                    self.sheet_record(j)
                        .and_then(|s| s.state)
                        .unwrap_or(SheetVisibility::Visible)
                        == SheetVisibility::Visible
                })
                .expect("checked above");
            self.set_active_sheet(first_visible)?;
        }
        Ok(())
    }

    /// Moves a sheet to tab position `to` (0-based). Sheet-local names and
    /// the active sheet follow the move.
    pub fn move_sheet(&mut self, name: &str, to: usize) -> Result<()> {
        let from = self.index_of(name)?;
        if to >= self.sheets.len() {
            return Err(Error::InvalidArgument(format!("position {to} is out of range")));
        }
        if from == to {
            return Ok(());
        }
        // New position of every old index.
        let mut order: Vec<usize> = (0..self.sheets.len()).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let mut new_index = vec![0; order.len()];
        for (new, &old) in order.iter().enumerate() {
            new_index[old] = new;
        }
        let active = self.active_sheet();
        let entry = self.sheets.remove(from);
        self.sheets.insert(to, entry);
        if let Some(list) = self.workbook.sheets.as_mut() {
            let rel_id = self.sheets[to].rel_id.clone();
            if let Some(pos) = list
                .sheet
                .iter()
                .position(|s| s.r_id.as_deref() == Some(rel_id.as_str()))
            {
                let rec = list.sheet.remove(pos);
                let at = to.min(list.sheet.len());
                list.sheet.insert(at, rec);
            }
        }
        if let Some(names) = self.workbook.defined_names.as_mut() {
            for n in &mut names.defined_name {
                if let Some(id) = n.local_sheet_id
                    && let Some(&new) = new_index.get(id as usize)
                {
                    n.local_sheet_id = Some(new as u32);
                }
            }
        }
        if let Some(views) = self.workbook.book_views.as_mut() {
            for v in &mut views.workbook_view {
                v.active_tab = (new_index[active] > 0).then_some(new_index[active] as u32);
                v.first_sheet = None;
            }
        }
        self.workbook_dirty = true;
        Ok(())
    }

    /// The calculation settings.
    pub fn calc_properties(&self) -> CalcProperties {
        let d = CalcProperties::default();
        let Some(c) = self.workbook.calc_pr.as_deref() else {
            return d;
        };
        CalcProperties {
            mode: c.calc_mode.unwrap_or(d.mode),
            full_calc_on_load: c.full_calc_on_load.unwrap_or(false),
            iterate: c.iterate.unwrap_or(false),
            iterate_count: c.iterate_count.unwrap_or(d.iterate_count),
            iterate_delta: c.iterate_delta.unwrap_or(d.iterate_delta),
            full_precision: c.full_precision.unwrap_or(true),
        }
    }

    /// Replaces the calculation settings (the calculation engine id is kept).
    pub fn set_calc_properties(&mut self, p: &CalcProperties) {
        let d = CalcProperties::default();
        let c = self.workbook.calc_pr.get_or_insert_with(Box::default);
        c.calc_mode = (p.mode != CalcMode::Auto).then_some(p.mode);
        c.full_calc_on_load = p.full_calc_on_load.then_some(true);
        c.iterate = p.iterate.then_some(true);
        c.iterate_count = (p.iterate_count != d.iterate_count).then_some(p.iterate_count);
        c.iterate_delta = (p.iterate_delta != d.iterate_delta).then_some(p.iterate_delta);
        c.full_precision = (!p.full_precision).then_some(false);
        self.workbook_dirty = true;
    }

    /// Adds a named cell style (listed in Excel's *Cell Styles* gallery) and
    /// returns a cell format using it. Adding an identical style again
    /// returns the same format; a different style with the same name fails.
    pub fn add_named_style(&mut self, name: &str, style: &CellStyle) -> Result<StyleId> {
        self.styles
            .add_named_style(name, style)
            .map_err(Error::InvalidArgument)
    }

    /// Names of the named cell styles.
    pub fn named_styles(&self) -> Vec<String> {
        self.styles.named_styles()
    }

    /// Name of the named style a cell format is based on (`Normal` for most).
    pub fn style_name(&self, id: StyleId) -> Option<String> {
        self.styles.style_name(id)
    }

    /// Positions of the worksheets (not chart or macro sheets).
    pub(crate) fn worksheet_positions(&self) -> Vec<usize> {
        (0..self.sheets.len())
            .filter(|&i| self.sheets[i].kind == SheetKind::Worksheet)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_sheets_remaps_local_names_and_the_active_tab() {
        let mut wb = Workbook::new();
        wb.add_worksheet("B").unwrap();
        wb.add_worksheet("C").unwrap();
        wb.set_defined_name("OnC", "C!$A$1", Some(2)).unwrap();
        wb.set_defined_name("OnSheet1", "Sheet1!$A$1", Some(0)).unwrap();
        wb.set_active_sheet(2).unwrap();
        wb.move_sheet("C", 0).unwrap();
        assert_eq!(wb.sheet_names(), ["C", "Sheet1", "B"]);
        let names = wb.defined_names();
        assert_eq!(names[0].local_sheet, Some(0));
        assert_eq!(names[1].local_sheet, Some(1));
        assert_eq!(wb.active_sheet(), 0, "the active sheet moved with it");
        wb.move_sheet("C", 2).unwrap();
        assert_eq!(wb.sheet_names(), ["Sheet1", "B", "C"]);
        assert_eq!(wb.defined_names()[0].local_sheet, Some(2));
        assert!(wb.move_sheet("C", 3).is_err());
        assert!(wb.move_sheet("Z", 0).is_err());
    }

    #[test]
    fn calc_properties_defaults_are_not_written() {
        let mut wb = Workbook::new();
        assert_eq!(wb.calc_properties(), CalcProperties::default());
        wb.set_calc_properties(&CalcProperties::default());
        let c = wb.raw_workbook().calc_pr.clone().unwrap();
        assert_eq!(c.calc_id, Some(191_029), "the engine id is kept");
        assert_eq!((c.calc_mode, c.iterate, c.iterate_count), (None, None, None));
    }
}
