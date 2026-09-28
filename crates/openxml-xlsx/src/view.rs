//! How a worksheet is displayed: zoom, gridlines, headings, direction,
//! view mode, tab colour and selection (ECMA-376 Part 1 §18.3.1.87,
//! §18.3.1.82).
//!
//! ```
//! use openxml_xlsx::{Workbook, Color, SheetViewMode};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_zoom(125)?;
//! sheet.set_show_gridlines(false);
//! sheet.set_tab_color(Some(Color::Rgb(0x2E, 0x75, 0xB6)));
//! sheet.set_view_mode(SheetViewMode::PageLayout);
//! sheet.set_selection("C3", "C3:E6")?;
//! let view = sheet.as_view();
//! assert_eq!(view.zoom(), 125);
//! assert!(!view.show_gridlines());
//! assert_eq!(view.active_cell().unwrap().to_string(), "C3");
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{CellRange, CellRef, ToCellRef, ToRanges};
use crate::styles::Color;
use crate::util::{from_sqref, to_sqref};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use sml::ST_SheetViewType as SheetViewMode;

impl Worksheet<'_> {
    fn first_view(&self) -> Option<&sml::CT_SheetView> {
        self.data.sheet_views.as_ref()?.sheet_view.first()
    }

    /// Zoom in percent (100 when not set).
    pub fn zoom(&self) -> u32 {
        self.first_view().and_then(|v| v.zoom_scale).unwrap_or(100)
    }

    /// Whether gridlines are shown.
    pub fn show_gridlines(&self) -> bool {
        self.first_view().and_then(|v| v.show_grid_lines).unwrap_or(true)
    }

    /// Whether row and column headings are shown.
    pub fn show_headings(&self) -> bool {
        self.first_view()
            .and_then(|v| v.show_row_col_headers)
            .unwrap_or(true)
    }

    /// Whether zero values are shown.
    pub fn show_zeros(&self) -> bool {
        self.first_view().and_then(|v| v.show_zeros).unwrap_or(true)
    }

    /// Whether the sheet is laid out right to left.
    pub fn right_to_left(&self) -> bool {
        self.first_view().and_then(|v| v.right_to_left).unwrap_or(false)
    }

    /// The view mode.
    pub fn view_mode(&self) -> SheetViewMode {
        self.first_view()
            .and_then(|v| v.view)
            .unwrap_or(SheetViewMode::Normal)
    }

    /// Colour of the sheet tab.
    pub fn tab_color(&self) -> Option<Color> {
        Color::from_ct(self.data.sheet_pr.as_ref()?.tab_color.as_deref()?)
    }

    fn active_selection(&self) -> Option<&sml::CT_Selection> {
        let view = self.first_view()?;
        let pane = view.pane.as_ref().and_then(|p| p.active_pane);
        view.selection
            .iter()
            .find(|s| s.pane == pane)
            .or_else(|| view.selection.first())
    }

    /// The active cell.
    pub fn active_cell(&self) -> Option<CellRef> {
        CellRef::parse(self.active_selection()?.active_cell.as_deref()?).ok()
    }

    /// The selected ranges.
    pub fn selection(&self) -> Vec<CellRange> {
        self.active_selection()
            .and_then(|s| s.sqref.as_ref())
            .map(from_sqref)
            .unwrap_or_default()
    }

    /// The top-left visible cell, if set.
    pub fn top_left_cell(&self) -> Option<CellRef> {
        CellRef::parse(self.first_view()?.top_left_cell.as_deref()?).ok()
    }
}

impl WorksheetMut<'_> {
    fn view_mut(&mut self) -> &mut sml::CT_SheetView {
        let views = self.data.sheet_views.get_or_insert_with(Box::default);
        if views.sheet_view.is_empty() {
            views.sheet_view.push(sml::CT_SheetView {
                workbook_view_id: Some(0),
                ..Default::default()
            });
        }
        &mut views.sheet_view[0]
    }

    /// Sets the zoom (10–400 percent).
    pub fn set_zoom(&mut self, percent: u32) -> Result<()> {
        if !(10..=400).contains(&percent) {
            return Err(Error::InvalidArgument(format!("zoom out of range: {percent}")));
        }
        let v = self.view_mut();
        v.zoom_scale = (percent != 100).then_some(percent);
        match v.view.unwrap_or(SheetViewMode::Normal) {
            SheetViewMode::Normal => v.zoom_scale_normal = (percent != 100).then_some(percent),
            SheetViewMode::PageLayout => v.zoom_scale_page_layout_view = Some(percent),
            SheetViewMode::PageBreakPreview => v.zoom_scale_sheet_layout_view = Some(percent),
        }
        Ok(())
    }

    /// Shows or hides gridlines.
    pub fn set_show_gridlines(&mut self, show: bool) {
        self.view_mut().show_grid_lines = (!show).then_some(false);
    }

    /// Shows or hides row and column headings.
    pub fn set_show_headings(&mut self, show: bool) {
        self.view_mut().show_row_col_headers = (!show).then_some(false);
    }

    /// Shows or hides zero values.
    pub fn set_show_zeros(&mut self, show: bool) {
        self.view_mut().show_zeros = (!show).then_some(false);
    }

    /// Shows formulas instead of their results.
    pub fn set_show_formulas(&mut self, show: bool) {
        self.view_mut().show_formulas = show.then_some(true);
    }

    /// Lays the sheet out right to left (for Arabic, Hebrew…).
    pub fn set_right_to_left(&mut self, rtl: bool) {
        self.view_mut().right_to_left = rtl.then_some(true);
    }

    /// Sets the view mode (normal, page layout, page break preview).
    pub fn set_view_mode(&mut self, mode: SheetViewMode) {
        self.view_mut().view = (mode != SheetViewMode::Normal).then_some(mode);
    }

    /// Sets (or removes) the colour of the sheet tab.
    pub fn set_tab_color(&mut self, color: Option<Color>) {
        let pr = self.data.sheet_pr.get_or_insert_with(Box::default);
        pr.tab_color = color.map(|c| Box::new(c.to_ct()));
    }

    /// Sets the first visible cell.
    pub fn set_top_left_cell(&mut self, cell: impl ToCellRef) -> Result<()> {
        let cell = cell.to_cell_ref()?;
        self.view_mut().top_left_cell = (cell.row() > 1 || cell.col() > 1).then(|| cell.to_string());
        Ok(())
    }

    /// Selects `ranges` with `active` as the active cell (in the active pane
    /// when panes are frozen).
    pub fn set_selection(&mut self, active: impl ToCellRef, ranges: impl ToRanges) -> Result<()> {
        let active = active.to_cell_ref()?;
        let ranges = ranges.to_ranges()?;
        if !ranges.iter().any(|r| r.contains(active)) {
            return Err(Error::InvalidArgument(format!(
                "the active cell {active} is outside the selection"
            )));
        }
        let v = self.view_mut();
        let pane = v.pane.as_ref().and_then(|p| p.active_pane);
        let sel = sml::CT_Selection {
            pane,
            active_cell: Some(active.to_string()),
            sqref: Some(to_sqref(&ranges)),
            ..Default::default()
        };
        match v.selection.iter_mut().find(|s| s.pane == pane) {
            Some(s) => *s = sel,
            None => v.selection.push(sel),
        }
        Ok(())
    }

    /// Makes `cell` the active (selected) cell.
    pub fn set_active_cell(&mut self, cell: impl ToCellRef) -> Result<()> {
        let cell = cell.to_cell_ref()?;
        self.set_selection(cell, CellRange::single(cell))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;

    #[test]
    fn zoom_and_selection_follow_the_view() {
        let mut wb = Workbook::new();
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_view_mode(SheetViewMode::PageLayout);
        s.set_zoom(80).unwrap();
        let v = &s.raw_mut().sheet_views.as_ref().unwrap().sheet_view[0];
        assert_eq!(
            (v.zoom_scale, v.zoom_scale_page_layout_view),
            (Some(80), Some(80))
        );
        s.set_view_mode(SheetViewMode::Normal);
        s.set_zoom(100).unwrap();
        assert_eq!(s.as_view().zoom(), 100);
        assert_eq!(
            s.raw_mut().sheet_views.as_ref().unwrap().sheet_view[0].zoom_scale,
            None
        );
        s.freeze_panes("C3").unwrap();
        s.set_active_cell("D5").unwrap();
        let v = &s.raw_mut().sheet_views.as_ref().unwrap().sheet_view[0];
        assert_eq!(v.selection.len(), 1, "the frozen pane's selection is replaced");
        assert_eq!(v.selection[0].pane, Some(sml::ST_Pane::BottomRight));
        assert_eq!(s.as_view().active_cell().unwrap().to_string(), "D5");
        s.set_tab_color(None);
        assert_eq!(s.as_view().tab_color(), None);
        s.set_top_left_cell("A1").unwrap();
        assert_eq!(s.as_view().top_left_cell(), None);
    }
}
