//! Office Open XML (ECMA-376) for Rust.
//!
//! This facade re-exports the layers of the toolkit:
//!
//! | Module | Crate | Purpose |
//! |--------|-------|---------|
//! | [`docx`] | `openxml-docx` | Word documents |
//! | [`xlsx`] | `openxml-xlsx` | Excel workbooks |
//! | [`pptx`] | `openxml-pptx` | PowerPoint presentations |
//! | [`chart`] | `openxml-chart` | DrawingML charts shared by the three formats |
//! | [`schema`] | `openxml-schema` | Every ECMA-376 schema type, generated from the official XSDs |
//! | [`opc`] | `openxml-opc` | Open Packaging Conventions (ZIP, parts, relationships) |
//! | [`xml`] | `openxml-xml` | Namespace-aware XML reader/writer and raw nodes |
//! | [`core`] | `openxml-core` | Shared errors, units and image helpers |
//!
//! ```
//! use openxml::docx::{Document, ListKind};
//! use openxml::pptx::{LayoutKind, Presentation};
//! use openxml::xlsx::{CellStyle, Workbook};
//!
//! let dir = std::env::temp_dir().join(format!("openxml-doc-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//!
//! // Word
//! let mut doc = Document::new();
//! doc.add_heading("Quarterly report", 1)?;
//! doc.add_paragraph("Revenue grew by ").add_run("12%").bold(true);
//! doc.add_list_item("New product line", ListKind::Bullet, 0)?;
//! doc.save(dir.join("report.docx"))?;
//!
//! // Excel
//! let mut wb = Workbook::new();
//! let bold = wb.add_style(&CellStyle::new().bold());
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_value("A1", "Total")?;
//! sheet.set_cell_style("A1", bold)?;
//! sheet.set_formula("B1", "SUM(B2:B10)")?;
//! wb.save(dir.join("report.xlsx"))?;
//!
//! // PowerPoint
//! let mut deck = Presentation::new();
//! let mut slide = deck.add_slide(LayoutKind::Title)?;
//! slide.set_title("Project Aurora")?;
//! deck.save(dir.join("deck.pptx"))?;
//!
//! // Reading
//! let text = Document::open(dir.join("report.docx"))?.text();
//! assert_eq!(text, "Quarterly report\nRevenue grew by 12%\nNew product line");
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), openxml::Error>(())
//! ```

#![warn(missing_docs)]

pub use openxml_chart as chart;
pub use openxml_core as core;
pub use openxml_docx as docx;
pub use openxml_opc as opc;
pub use openxml_pptx as pptx;
pub use openxml_schema as schema;
pub use openxml_xlsx as xlsx;
pub use openxml_xml as xml;

pub use openxml_core::{Error, Length, Result};
