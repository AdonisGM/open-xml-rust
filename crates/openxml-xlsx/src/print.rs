//! Printing: page setup, margins, print options, headers and footers,
//! print area and titles, and manual page breaks (ECMA-376 Part 1
//! §18.3.1.51, §18.3.1.62, §18.3.1.70, §18.3.1.46, §18.3.1.73).
//!
//! ```
//! use openxml_xlsx::{Workbook, PageSetup, Orientation, HeaderFooter, print::{paper, hf}};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_page_setup(&PageSetup {
//!     orientation: Some(Orientation::Landscape),
//!     paper_size: Some(paper::A4),
//!     ..PageSetup::default()
//! })?;
//! sheet.fit_to_pages(1, 0)?;
//! sheet.set_header_footer(&HeaderFooter::new()
//!     .header(hf::sections("", "&BQuarterly report", ""))
//!     .footer(hf::sections("", &format!("Page {} of {}", hf::PAGE, hf::PAGES), "")))?;
//! sheet.set_print_area("A1:F40")?;
//! sheet.set_print_titles(Some((1, 1)), None)?;
//! sheet.add_row_break(20)?;
//! assert_eq!(sheet.as_view().print_area().as_deref(), Some("Sheet1!$A$1:$F$40"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{MAX_COL, MAX_ROW, ToCellRange, column_name};
use crate::util::{local_name, qualified_range, quote_sheet, remove_local_name, set_local_name};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use sml::{ST_Orientation as Orientation, ST_PageOrder as PageOrder};

const PRINT_AREA: &str = "_xlnm.Print_Area";
const PRINT_TITLES: &str = "_xlnm.Print_Titles";

/// Paper sizes (the `paperSize` codes of §18.3.1.63).
pub mod paper {
    /// Letter, 8.5 × 11 in.
    pub const LETTER: u32 = 1;
    /// Tabloid, 11 × 17 in.
    pub const TABLOID: u32 = 3;
    /// Ledger, 17 × 11 in.
    pub const LEDGER: u32 = 4;
    /// Legal, 8.5 × 14 in.
    pub const LEGAL: u32 = 5;
    /// Statement, 5.5 × 8.5 in.
    pub const STATEMENT: u32 = 6;
    /// Executive, 7.25 × 10.5 in.
    pub const EXECUTIVE: u32 = 7;
    /// A3, 297 × 420 mm.
    pub const A3: u32 = 8;
    /// A4, 210 × 297 mm.
    pub const A4: u32 = 9;
    /// A5, 148 × 210 mm.
    pub const A5: u32 = 11;
    /// B4 (JIS), 250 × 353 mm.
    pub const B4: u32 = 12;
    /// B5 (JIS), 182 × 257 mm.
    pub const B5: u32 = 13;
    /// Folio, 8.5 × 13 in.
    pub const FOLIO: u32 = 14;
    /// Envelope #10, 4.125 × 9.5 in.
    pub const ENVELOPE_10: u32 = 20;
    /// Envelope DL, 110 × 220 mm.
    pub const ENVELOPE_DL: u32 = 27;
    /// Envelope C5, 162 × 229 mm.
    pub const ENVELOPE_C5: u32 = 28;
}

/// Header and footer codes (§18.3.1.46).
pub mod hf {
    /// Current page number.
    pub const PAGE: &str = "&P";
    /// Number of pages.
    pub const PAGES: &str = "&N";
    /// Current date.
    pub const DATE: &str = "&D";
    /// Current time.
    pub const TIME: &str = "&T";
    /// Workbook file name.
    pub const FILE: &str = "&F";
    /// Workbook path.
    pub const PATH: &str = "&Z";
    /// Sheet name.
    pub const SHEET: &str = "&A";
    /// Toggles bold.
    pub const BOLD: &str = "&B";
    /// Toggles italic.
    pub const ITALIC: &str = "&I";

    /// A header or footer with left, centre and right sections (empty
    /// sections are omitted). A literal `&` must be written `&&`.
    pub fn sections(left: &str, center: &str, right: &str) -> String {
        let mut out = String::new();
        for (code, text) in [("&L", left), ("&C", center), ("&R", right)] {
            if !text.is_empty() {
                out.push_str(code);
                out.push_str(text);
            }
        }
        out
    }

    /// Text in a font, e.g. `font("Arial", "Bold")` then the text.
    pub fn font(name: &str, style: &str) -> String {
        format!("&\"{name},{style}\"")
    }

    /// Font size in points.
    pub fn size(points: u32) -> String {
        format!("&{points}")
    }
}

/// Page setup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageSetup {
    /// Portrait or landscape.
    pub orientation: Option<Orientation>,
    /// Paper size code, see [`paper`].
    pub paper_size: Option<u32>,
    /// Print scale in percent (10–400).
    pub scale: Option<u32>,
    /// Fit to this many pages wide (0 = as many as needed); see [`WorksheetMut::fit_to_pages`].
    pub fit_to_width: Option<u32>,
    /// Fit to this many pages tall (0 = as many as needed).
    pub fit_to_height: Option<u32>,
    /// Number of the first page.
    pub first_page_number: Option<u32>,
    /// Order of pages.
    pub page_order: Option<PageOrder>,
    /// Print in black and white.
    pub black_and_white: bool,
    /// Draft quality.
    pub draft: bool,
    /// Number of copies.
    pub copies: Option<u32>,
}

/// Page margins in inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMargins {
    /// Left.
    pub left: f64,
    /// Right.
    pub right: f64,
    /// Top.
    pub top: f64,
    /// Bottom.
    pub bottom: f64,
    /// Header distance from the top edge.
    pub header: f64,
    /// Footer distance from the bottom edge.
    pub footer: f64,
}

impl Default for PageMargins {
    /// Excel's *Normal* margins.
    fn default() -> Self {
        PageMargins {
            left: 0.7,
            right: 0.7,
            top: 0.75,
            bottom: 0.75,
            header: 0.3,
            footer: 0.3,
        }
    }
}

/// Print options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrintOptions {
    /// Print gridlines.
    pub gridlines: bool,
    /// Print row and column headings.
    pub headings: bool,
    /// Center on the page horizontally.
    pub center_horizontally: bool,
    /// Center on the page vertically.
    pub center_vertically: bool,
}

/// Headers and footers (texts use the codes of [`hf`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderFooter {
    /// Header of odd (or all) pages.
    pub odd_header: Option<String>,
    /// Footer of odd (or all) pages.
    pub odd_footer: Option<String>,
    /// Header of even pages (with `different_odd_even`).
    pub even_header: Option<String>,
    /// Footer of even pages.
    pub even_footer: Option<String>,
    /// Header of the first page (with `different_first`).
    pub first_header: Option<String>,
    /// Footer of the first page.
    pub first_footer: Option<String>,
    /// Even pages have their own header and footer.
    pub different_odd_even: bool,
    /// The first page has its own header and footer.
    pub different_first: bool,
    /// Scale with the document.
    pub scale_with_document: bool,
    /// Align with the page margins.
    pub align_with_margins: bool,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl HeaderFooter {
    /// No headers or footers.
    pub fn new() -> Self {
        HeaderFooter {
            odd_header: None,
            odd_footer: None,
            even_header: None,
            even_footer: None,
            first_header: None,
            first_footer: None,
            different_odd_even: false,
            different_first: false,
            scale_with_document: true,
            align_with_margins: true,
        }
    }

    /// The header of every page.
    pub fn header(mut self, text: impl Into<String>) -> Self {
        self.odd_header = Some(text.into());
        self
    }

    /// The footer of every page.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.odd_footer = Some(text.into());
        self
    }

    /// Separate header and footer for even pages.
    pub fn even(mut self, header: impl Into<String>, footer: impl Into<String>) -> Self {
        self.different_odd_even = true;
        self.even_header = Some(header.into());
        self.even_footer = Some(footer.into());
        self
    }

    /// Separate header and footer for the first page.
    pub fn first(mut self, header: impl Into<String>, footer: impl Into<String>) -> Self {
        self.different_first = true;
        self.first_header = Some(header.into());
        self.first_footer = Some(footer.into());
        self
    }
}

fn breaks(b: &Option<Box<sml::CT_PageBreak>>) -> Vec<u32> {
    b.iter().flat_map(|b| b.brk.iter()).filter_map(|b| b.id).collect()
}

impl Worksheet<'_> {
    /// The page setup.
    pub fn page_setup(&self) -> PageSetup {
        let Some(p) = self.data.page_setup.as_deref() else {
            return PageSetup::default();
        };
        let fit = self
            .data
            .sheet_pr
            .as_ref()
            .and_then(|s| s.page_set_up_pr.as_ref())
            .and_then(|p| p.fit_to_page)
            .unwrap_or(false);
        PageSetup {
            orientation: p.orientation,
            paper_size: p.paper_size,
            scale: p.scale,
            fit_to_width: fit.then(|| p.fit_to_width.unwrap_or(1)),
            fit_to_height: fit.then(|| p.fit_to_height.unwrap_or(1)),
            first_page_number: p
                .first_page_number
                .filter(|_| p.use_first_page_number == Some(true)),
            page_order: p.page_order,
            black_and_white: p.black_and_white.unwrap_or(false),
            draft: p.draft.unwrap_or(false),
            copies: p.copies,
        }
    }

    /// The page margins, if set.
    pub fn page_margins(&self) -> Option<PageMargins> {
        let m = self.data.page_margins.as_deref()?;
        let d = PageMargins::default();
        Some(PageMargins {
            left: m.left.unwrap_or(d.left),
            right: m.right.unwrap_or(d.right),
            top: m.top.unwrap_or(d.top),
            bottom: m.bottom.unwrap_or(d.bottom),
            header: m.header.unwrap_or(d.header),
            footer: m.footer.unwrap_or(d.footer),
        })
    }

    /// The print options.
    pub fn print_options(&self) -> PrintOptions {
        let Some(p) = self.data.print_options.as_deref() else {
            return PrintOptions::default();
        };
        PrintOptions {
            gridlines: p.grid_lines.unwrap_or(false),
            headings: p.headings.unwrap_or(false),
            center_horizontally: p.horizontal_centered.unwrap_or(false),
            center_vertically: p.vertical_centered.unwrap_or(false),
        }
    }

    /// Headers and footers, if set.
    pub fn header_footer(&self) -> Option<HeaderFooter> {
        let h = self.data.header_footer.as_deref()?;
        Some(HeaderFooter {
            odd_header: h.odd_header.clone(),
            odd_footer: h.odd_footer.clone(),
            even_header: h.even_header.clone(),
            even_footer: h.even_footer.clone(),
            first_header: h.first_header.clone(),
            first_footer: h.first_footer.clone(),
            different_odd_even: h.different_odd_even.unwrap_or(false),
            different_first: h.different_first.unwrap_or(false),
            scale_with_document: h.scale_with_doc.unwrap_or(true),
            align_with_margins: h.align_with_margins.unwrap_or(true),
        })
    }

    /// The print area formula (e.g. `Sheet1!$A$1:$F$40`).
    pub fn print_area(&self) -> Option<String> {
        local_name(self.env.workbook, PRINT_AREA, self.env.index).map(str::to_owned)
    }

    /// The print titles formula (e.g. `Sheet1!$1:$1,Sheet1!$A:$A`).
    pub fn print_titles(&self) -> Option<String> {
        local_name(self.env.workbook, PRINT_TITLES, self.env.index).map(str::to_owned)
    }

    /// Rows after which a manual page break is set.
    pub fn row_breaks(&self) -> Vec<u32> {
        breaks(&self.data.row_breaks)
    }

    /// Columns after which a manual page break is set.
    pub fn column_breaks(&self) -> Vec<u32> {
        breaks(&self.data.col_breaks)
    }
}

impl WorksheetMut<'_> {
    /// Sets the page setup (the printer settings reference of an existing
    /// setup is kept).
    pub fn set_page_setup(&mut self, setup: &PageSetup) -> Result<()> {
        if setup.scale.is_some_and(|s| !(10..=400).contains(&s)) {
            return Err(Error::InvalidArgument("the print scale must be 10–400".into()));
        }
        let fit = setup.fit_to_width.is_some() || setup.fit_to_height.is_some();
        let p = self.data.page_setup.get_or_insert_with(Box::default);
        p.orientation = setup.orientation;
        p.paper_size = setup.paper_size;
        p.scale = setup.scale;
        p.fit_to_width = setup.fit_to_width.filter(|&w| w != 1);
        p.fit_to_height = setup.fit_to_height.filter(|&h| h != 1);
        p.first_page_number = setup.first_page_number;
        p.use_first_page_number = setup.first_page_number.map(|_| true);
        p.page_order = setup.page_order;
        p.black_and_white = setup.black_and_white.then_some(true);
        p.draft = setup.draft.then_some(true);
        p.copies = setup.copies;
        let pr = self.data.sheet_pr.get_or_insert_with(Box::default);
        if fit {
            pr.page_set_up_pr.get_or_insert_with(Box::default).fit_to_page = Some(true);
        } else if let Some(p) = pr.page_set_up_pr.as_mut() {
            p.fit_to_page = None;
        }
        Ok(())
    }

    /// Scales the printout to fit `width` pages wide and `height` pages
    /// tall (0 = as many as needed).
    pub fn fit_to_pages(&mut self, width: u32, height: u32) -> Result<()> {
        let mut setup = self.as_view().page_setup();
        setup.fit_to_width = Some(width);
        setup.fit_to_height = Some(height);
        setup.scale = None;
        self.set_page_setup(&setup)
    }

    /// Sets the orientation.
    pub fn set_orientation(&mut self, orientation: Orientation) -> Result<()> {
        let mut setup = self.as_view().page_setup();
        setup.orientation = Some(orientation);
        self.set_page_setup(&setup)
    }

    /// Sets the paper size (see [`paper`]).
    pub fn set_paper_size(&mut self, code: u32) -> Result<()> {
        let mut setup = self.as_view().page_setup();
        setup.paper_size = Some(code);
        self.set_page_setup(&setup)
    }

    /// Sets the page margins.
    pub fn set_page_margins(&mut self, m: &PageMargins) -> Result<()> {
        let all = [m.left, m.right, m.top, m.bottom, m.header, m.footer];
        if all.iter().any(|v| !v.is_finite() || *v < 0.0 || *v >= 49.0) {
            return Err(Error::InvalidArgument("margins must be 0–49 inches".into()));
        }
        self.data.page_margins = Some(Box::new(sml::CT_PageMargins {
            left: Some(m.left),
            right: Some(m.right),
            top: Some(m.top),
            bottom: Some(m.bottom),
            header: Some(m.header),
            footer: Some(m.footer),
            ..Default::default()
        }));
        Ok(())
    }

    /// Sets the print options.
    pub fn set_print_options(&mut self, o: &PrintOptions) {
        let any = o.gridlines || o.headings || o.center_horizontally || o.center_vertically;
        self.data.print_options = any.then(|| {
            Box::new(sml::CT_PrintOptions {
                horizontal_centered: o.center_horizontally.then_some(true),
                vertical_centered: o.center_vertically.then_some(true),
                headings: o.headings.then_some(true),
                grid_lines: o.gridlines.then_some(true),
                ..Default::default()
            })
        });
    }

    /// Sets headers and footers (each text at most 255 characters).
    pub fn set_header_footer(&mut self, h: &HeaderFooter) -> Result<()> {
        let texts = [
            &h.odd_header,
            &h.odd_footer,
            &h.even_header,
            &h.even_footer,
            &h.first_header,
            &h.first_footer,
        ];
        if texts
            .iter()
            .any(|t| t.as_ref().is_some_and(|t| t.chars().count() > 255))
        {
            return Err(Error::InvalidArgument(
                "header and footer texts are limited to 255 characters".into(),
            ));
        }
        self.data.header_footer = Some(Box::new(sml::CT_HeaderFooter {
            different_odd_even: h.different_odd_even.then_some(true),
            different_first: h.different_first.then_some(true),
            scale_with_doc: (!h.scale_with_document).then_some(false),
            align_with_margins: (!h.align_with_margins).then_some(false),
            odd_header: h.odd_header.clone(),
            odd_footer: h.odd_footer.clone(),
            even_header: h.even_header.clone(),
            even_footer: h.even_footer.clone(),
            first_header: h.first_header.clone(),
            first_footer: h.first_footer.clone(),
            ..Default::default()
        }));
        Ok(())
    }

    /// Restricts printing to a range (the `_xlnm.Print_Area` name).
    pub fn set_print_area(&mut self, range: impl ToCellRange) -> Result<()> {
        let range = range.to_cell_range()?;
        set_local_name(
            self.workbook,
            PRINT_AREA,
            self.index,
            qualified_range(self.name, range),
            false,
        );
        *self.workbook_dirty = true;
        Ok(())
    }

    /// Removes the print area. Returns whether one was set.
    pub fn clear_print_area(&mut self) -> bool {
        let removed = remove_local_name(self.workbook, PRINT_AREA, self.index);
        *self.workbook_dirty |= removed;
        removed
    }

    /// Rows (first, last) and/or columns (first, last) repeated on every
    /// printed page; `None` for both removes the titles.
    pub fn set_print_titles(&mut self, rows: Option<(u32, u32)>, cols: Option<(u32, u32)>) -> Result<()> {
        let sheet = quote_sheet(self.name);
        let mut parts = Vec::new();
        if let Some((a, b)) = rows {
            if a == 0 || a > b || b > MAX_ROW {
                return Err(Error::InvalidArgument(format!("invalid title rows {a}:{b}")));
            }
            parts.push(format!("{sheet}!${a}:${b}"));
        }
        if let Some((a, b)) = cols {
            if a == 0 || a > b || b > MAX_COL {
                return Err(Error::InvalidArgument(format!("invalid title columns {a}:{b}")));
            }
            parts.push(format!("{sheet}!${}:${}", column_name(a), column_name(b)));
        }
        if parts.is_empty() {
            remove_local_name(self.workbook, PRINT_TITLES, self.index);
        } else {
            set_local_name(self.workbook, PRINT_TITLES, self.index, parts.join(","), false);
        }
        *self.workbook_dirty = true;
        Ok(())
    }

    /// Adds a manual page break after `row`.
    pub fn add_row_break(&mut self, row: u32) -> Result<()> {
        if !(1..MAX_ROW).contains(&row) {
            return Err(Error::InvalidArgument(format!("row out of range: {row}")));
        }
        add_break(&mut self.data.row_breaks, row, MAX_COL - 1);
        Ok(())
    }

    /// Adds a manual page break after column `col`.
    pub fn add_column_break(&mut self, col: u32) -> Result<()> {
        if !(1..MAX_COL).contains(&col) {
            return Err(Error::InvalidArgument(format!("column out of range: {col}")));
        }
        add_break(&mut self.data.col_breaks, col, MAX_ROW - 1);
        Ok(())
    }

    /// Removes the page break after `row`. Returns whether it existed.
    pub fn remove_row_break(&mut self, row: u32) -> bool {
        remove_break(&mut self.data.row_breaks, row)
    }

    /// Removes the page break after column `col`. Returns whether it existed.
    pub fn remove_column_break(&mut self, col: u32) -> bool {
        remove_break(&mut self.data.col_breaks, col)
    }
}

fn recount(b: &mut sml::CT_PageBreak) {
    b.count = Some(b.brk.len() as u32);
    b.manual_break_count = Some(b.brk.iter().filter(|b| b.man == Some(true)).count() as u32);
}

fn add_break(slot: &mut Option<Box<sml::CT_PageBreak>>, id: u32, max: u32) {
    let b = slot.get_or_insert_with(Box::default);
    if !b.brk.iter().any(|x| x.id == Some(id)) {
        let at = b
            .brk
            .iter()
            .position(|x| x.id.is_some_and(|x| x > id))
            .unwrap_or(b.brk.len());
        b.brk.insert(
            at,
            sml::CT_Break {
                id: Some(id),
                max: Some(max),
                man: Some(true),
                ..Default::default()
            },
        );
    }
    recount(b);
}

fn remove_break(slot: &mut Option<Box<sml::CT_PageBreak>>, id: u32) -> bool {
    let Some(b) = slot.as_mut() else {
        return false;
    };
    let before = b.brk.len();
    b.brk.retain(|x| x.id != Some(id));
    let removed = before != b.brk.len();
    recount(b);
    if b.brk.is_empty() {
        *slot = None;
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_footer_codes() {
        assert_eq!(hf::sections("a", "", "&P"), "&La&R&P");
        assert_eq!(hf::sections("", "", ""), "");
        assert_eq!(hf::font("Arial", "Bold"), "&\"Arial,Bold\"");
        assert_eq!(hf::size(14), "&14");
    }

    #[test]
    fn breaks_are_sorted_and_counted() {
        let mut slot = None;
        add_break(&mut slot, 20, 16383);
        add_break(&mut slot, 10, 16383);
        add_break(&mut slot, 20, 16383);
        let b = slot.as_ref().unwrap();
        assert_eq!(breaks(&slot), [10, 20]);
        assert_eq!((b.count, b.manual_break_count), (Some(2), Some(2)));
        assert!(remove_break(&mut slot, 10));
        assert!(!remove_break(&mut slot, 10));
        assert!(remove_break(&mut slot, 20));
        assert!(slot.is_none());
    }
}
