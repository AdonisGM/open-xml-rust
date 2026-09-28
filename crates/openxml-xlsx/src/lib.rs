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
//! * [`CellStyle`] — formatting registered with [`Workbook::add_style`].
//! * [`StreamingWorksheet`] and [`Workbook::for_each_row`] — writing and
//!   reading large sheets row by row.
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

pub mod cell_ref;
pub mod date;
pub mod formula;
pub mod shared_strings;
mod stream;
pub mod styles;
pub mod value;
mod workbook;
mod worksheet;

pub use cell_ref::{CellRange, CellRef, MAX_COL, MAX_ROW, ToCellRef, column_index, column_name};
pub use date::{DateSystem, DateTime, is_date_format};
pub use stream::StreamingWorksheet;
pub use styles::{
    Alignment, Border, BorderSide, BorderStyle, CellStyle, Color, Fill, Font, HorizontalAlignment,
    NumberFormat, PatternType, StyleId, VerticalAlignment,
};
pub use value::CellValue;
pub use workbook::{APPLICATION_NAME, DefinedName, Workbook, validate_sheet_name};
pub use worksheet::{Row, SheetKind, Worksheet, WorksheetMut};

pub use openxml_core::{Error, Result};
