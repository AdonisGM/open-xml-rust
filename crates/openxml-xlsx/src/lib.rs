//! Read, create and edit Excel workbooks (`.xlsx`, SpreadsheetML).
//!
//! The crate is a high-level layer over the generated `openxml_schema::sml`
//! types and the `openxml_opc` package model:
//!
//! * [`Workbook`] — opens and saves packages, manages sheets, the shared
//!   string table, styles and defined names. Only the parts it edits are
//!   rewritten on save; everything else is preserved byte for byte.
//! * [`Worksheet`] / [`WorksheetMut`] — read and write cell values,
//!   formulas, styles, merged ranges, column widths, row heights and frozen
//!   panes.
//! * [`CellValue`], [`CellRef`], [`CellRange`], [`DateTime`] — values and
//!   addressing, including Excel's date serial numbers in both date systems.
//! * [`CellStyle`] — formatting registered with [`Workbook::add_style`]
//!   (fonts, pattern and gradient fills, borders, alignment, protection,
//!   number formats) and named styles ([`Workbook::add_named_style`]).
//! * [`StreamingWorksheet`] and [`Workbook::for_each_row`] — writing and
//!   reading large sheets row by row.
//!
//! Worksheet features, each in its own module:
//!
//! | Module | Feature |
//! | --- | --- |
//! | [`drawing`] | pictures (two-cell, one-cell, absolute anchors) and graphic frames |
//! | [`comments`] | cell notes with their VML shapes |
//! | [`validation`] | data validation (lists, numbers, dates, text length, formulas) |
//! | [`conditional`] | conditional formatting (rules, color scales, data bars, icon sets) |
//! | [`table`] | tables with styles, totals rows and filter buttons |
//! | [`filter`] | the sheet auto filter, criteria and sort state |
//! | [`hyperlink`] | external and internal hyperlinks |
//! | [`print`](mod@print) | page setup, margins, headers/footers, print area and titles, page breaks |
//! | [`protection`] | sheet and workbook protection (legacy and SHA-512 hashes) |
//! | [`rich_text`] | formatted text runs in cells |
//! | [`layout`] | hidden rows/columns, outline groups, default sizes |
//! | [`view`] | zoom, gridlines, direction, tab colour, selection |
//! | [`calc`] | an evaluator for a small formula subset (cached results) |
//!
//! [`Workbook::insert_rows`] and its siblings insert or delete rows and
//! columns and update formulas and everything that refers to cells;
//! renaming or removing sheets keeps references valid as well.
//!
//! ```
//! use openxml_xlsx::{Workbook, CellStyle, CellValue, DateTime, NumberFormat};
//!
//! let mut wb = Workbook::new();
//! let bold = wb.add_style(&CellStyle::new().bold());
//! let money = wb.add_style(&CellStyle::new().number_format(NumberFormat::THOUSANDS_DECIMAL_2));
//! {
//!     let mut sheet = wb.worksheet_mut("Sheet1")?;
//!     sheet.set_value("A1", "Item")?;
//!     sheet.set_value("B1", "Price")?;
//!     sheet.set_cell_style("A1", bold)?;
//!     sheet.set_cell_style("B1", bold)?;
//!     sheet.set_value("A2", "Coffee")?;
//!     sheet.set_value("B2", 3.5)?;
//!     sheet.set_cell_style("B2", money)?;
//!     sheet.set_value("A3", DateTime::from_ymd(2024, 5, 1).unwrap())?;
//!     sheet.set_formula("B3", "SUM(B2:B2)")?;
//! }
//! let bytes = wb.to_bytes()?;
//! let wb = Workbook::from_bytes(&bytes)?;
//! let sheet = wb.worksheet("Sheet1")?;
//! assert_eq!(sheet.cell("A2")?.as_str(), Some("Coffee"));
//! assert!(matches!(sheet.cell("A3")?, CellValue::DateTime(_)));
//! assert_eq!(sheet.cell("B3")?.as_formula(), Some("SUM(B2:B2)"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

#![warn(missing_docs)]

mod book;
pub mod calc;
pub mod cell_ref;
pub mod comments;
pub mod conditional;
pub mod date;
pub mod drawing;
pub mod filter;
pub mod formula;
pub mod hyperlink;
pub mod layout;
mod parts;
pub mod print;
pub mod protection;
pub mod rich_text;
pub mod shared_strings;
mod stream;
mod structure;
pub mod styles;
pub mod table;
mod util;
pub mod validation;
pub mod value;
pub mod view;
mod workbook;
mod worksheet;

pub use book::{CalcMode, CalcProperties, SheetVisibility};
pub use calc::CalcReport;
pub use cell_ref::{
    CellRange, CellRef, MAX_COL, MAX_ROW, ToCellRange, ToCellRef, ToRanges, column_index, column_name,
};
pub use comments::Comment;
pub use conditional::{CfOperator, CfRule, CfValue, ConditionalFormat, IconSetType, TimePeriod};
pub use date::{DateSystem, DateTime, is_date_format};
pub use drawing::{Anchor, AnchorPoint, EditAs, Image, Placement, SheetImage};
pub use filter::{AutoFilter, ColumnFilter, FilterOperator, SortKey};
pub use hyperlink::{Hyperlink, LinkTarget};
pub use print::{HeaderFooter, Orientation, PageMargins, PageOrder, PageSetup, PrintOptions};
pub use protection::{PasswordHash, SheetProtection, WorkbookProtection};
pub use rich_text::{RichText, TextRun};
pub use stream::StreamingWorksheet;
pub use styles::{
    Alignment, Border, BorderSide, BorderStyle, CellProtection, CellStyle, Color, Fill, Font,
    FontVerticalAlign, GradientFill, GradientKind, HorizontalAlignment, NumberFormat, PatternType, StyleId,
    UnderlineStyle, VerticalAlignment,
};
pub use table::{Table, TableColumn, TableInfo, TotalsRow};
pub use validation::{Comparison, DataValidation, ErrorStyle, ValidationRule};
pub use value::CellValue;
pub use view::SheetViewMode;
pub use workbook::{APPLICATION_NAME, DefinedName, Workbook, validate_sheet_name};
pub use worksheet::{Row, SheetKind, Worksheet, WorksheetMut};

pub use openxml_core::{Error, Length, Result};
