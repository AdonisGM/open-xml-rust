//! Read, create and edit WordprocessingML documents (`.docx`).
//!
//! This crate is a high-level layer over the generated
//! [`openxml_schema::wml`] types. A [`Document`] owns the whole package;
//! the parts it manages (main document, styles, numbering, headers and
//! footers) are kept as typed schema objects and every other part is saved
//! unchanged, so documents keep content this API does not know about.
//!
//! ```
//! use openxml_docx::{Alignment, Document, FontSize, Length, ListKind};
//!
//! let mut doc = Document::new();
//! doc.add_heading("Quarterly report", 1)?;
//! let mut p = doc.add_paragraph("Revenue grew by ");
//! p.add_run("12%").bold(true).size(FontSize(12.0));
//! p.set_alignment(Alignment::Justify);
//! doc.add_list_item("First point", ListKind::Bullet, 0)?;
//! let mut table = doc.add_table(2, 2)?;
//! table.cell(0, 0)?.set_text("Region");
//! doc.set_footer("Confidential")?;
//!
//! let bytes = doc.to_bytes()?;
//! let reopened = Document::from_bytes(&bytes)?;
//! assert!(reopened.text().starts_with("Quarterly report\nRevenue grew by 12%"));
//! # Ok::<(), openxml_docx::Error>(())
//! ```
//!
//! Anything not covered by the convenience API is reachable through the
//! typed escape hatches: [`Document::document_mut`], [`Document::body_mut`],
//! [`ParagraphMut::raw`], [`RunMut::raw`] and [`Document::package_mut`].

#![warn(missing_docs)]

mod document;
mod numbering;
mod paragraph;
mod picture;
mod run;
mod section;
mod table;
mod template;
mod text;
mod util;

pub use document::{Block, Document};
pub use numbering::ListKind;
pub use paragraph::{Alignment, HyperlinkRef, Paragraph, ParagraphMut};
pub use run::{BreakKind, Run, RunMut, VerticalAlign};
pub use section::{HeaderFooter, HeaderFooterKind, Margins, Orientation, PageSetup};
pub use table::{CellAlign, CellMut, Table, TableCell, TableMut, TableRow};

pub use openxml_core::{Error, FontSize, Length, Result};
pub use openxml_opc::CoreProperties;
pub use openxml_schema::wml::{ST_HighlightColor as HighlightColor, ST_Underline as UnderlineStyle};
