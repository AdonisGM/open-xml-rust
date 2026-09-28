//! Read, create and edit WordprocessingML documents (`.docx`).
//!
//! This crate is a high-level layer over the generated
//! [`openxml_schema::wml`] types. A [`Document`] owns the whole package;
//! the parts it manages (main document, styles, numbering, settings,
//! comments, footnotes, endnotes, headers and footers) are kept as typed
//! schema objects and written back only when they change; every other part
//! is saved unchanged, so documents keep content this API does not know about.
//!
//! Features, by area:
//!
//! * text and formatting: paragraphs, runs, [`ParagraphFormat`] (borders,
//!   shading, tab stops, [`LineSpacing`]), custom styles ([`StyleDefinition`]);
//! * tables: formatting ([`TableFormat`]), row heights, header rows, nested tables;
//! * lists: built-in bullets and numbers, custom [`ListDefinition`]s, heading outlines;
//! * annotations: comments ([`NewComment`]), footnotes and endnotes,
//!   tracked changes ([`RevisionInfo`], accept/reject), bookmarks, fields and a
//!   table of contents ([`Field`], [`TableOfContents`]);
//! * objects: inline and floating pictures ([`PictureOptions`], [`Floating`]),
//!   VML text boxes and shapes ([`ShapeOptions`]), watermarks, equations ([`Math`]),
//!   content controls ([`ContentControl`]) and form check boxes;
//! * layout: sections ([`SectionMut`]) with breaks, columns, page numbering,
//!   page borders, line numbering and per-section headers and footers;
//! * document: core, application and custom properties ([`PropertyValue`]),
//!   settings, editing protection ([`Protection`]) and the page background.
//!
//! Everything written is valid against the ECMA-376 Transitional schemas;
//! Microsoft extensions (`w14`, `w15`, `wps`) are never produced but are
//! preserved when present.
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

mod bookmarks;
mod chart;
mod comments;
mod document;
mod drawing;
mod fields;
mod format;
mod markup;
mod math;
mod notes;
mod numbering;
mod paragraph;
mod picture;
mod properties;
mod revisions;
mod run;
mod sdt;
mod section;
mod styles;
mod table;
mod template;
mod text;
mod util;
mod vml;
mod walk;

pub use bookmarks::Bookmark;
pub use comments::{Comment, NewComment};
pub use document::{Block, Document};
pub use drawing::{
    Floating, HorizontalAlignment, HorizontalAnchor, HorizontalPosition, PictureInfo, PictureOptions,
    VerticalAlignment, VerticalAnchor, VerticalPosition, Wrap,
};
pub use fields::{Field, FieldInfo, TableOfContents};
pub use format::{
    Border, BorderStyle, CellMargins, HeightRule, LineSpacing, ParagraphBorders, ParagraphFormat, RunFormat,
    TabAlignment, TabLeader, TabStop, TableAlignment, TableBorders, TableFormat, TableLayout, TableWidth,
};
pub use markup::TextSpan;
pub use math::Math;
pub use notes::{Note, NoteKind, NotePosition, NoteProperties, NoteRestart};
pub use numbering::{ListDefinition, ListKind, ListLevel};
pub use paragraph::{Alignment, HyperlinkRef, LinkTarget, Paragraph, ParagraphMut};
pub use revisions::{Revision, RevisionInfo, RevisionKind};
pub use run::{BreakKind, Run, RunMut, VerticalAlign};
pub use sdt::{
    Checkbox, ContentControl, ContentControlInfo, ContentControlKind, ContentControlType, ListItem,
};
pub use section::{
    Columns, HeaderFooter, HeaderFooterKind, HeaderFooterType, LineNumberRestart, LineNumbering, Margins,
    Orientation, PageBorders, PageNumbering, PageSetup, Section, SectionBreak, SectionMut,
};
pub use styles::{StyleDefinition, StyleKind};
pub use table::{CellAlign, CellMut, Table, TableCell, TableMut, TableRow};
pub use vml::{ShapeKind, ShapeOptions, TextBoxContent};

pub use openxml_core::{Error, FontSize, Length, Result};
pub use openxml_opc::CoreProperties;
pub use openxml_schema::shared_extended_properties::CT_Properties as AppProperties;
pub use openxml_schema::wml::{
    ST_HighlightColor as HighlightColor, ST_NumberFormat as NumberFormat, ST_Underline as UnderlineStyle,
};
pub use properties::{PropertyValue, Protection};

pub use openxml_chart::{Chart, ChartInfo, ChartKind, Grouping, LegendPosition, Series};
