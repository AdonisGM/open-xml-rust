//! Office Open XML (ECMA-376) for Rust.
//!
//! This facade re-exports the layers of the toolkit:
//!
//! | Module | Crate | Purpose |
//! |--------|-------|---------|
//! | [`docx`] | `openxml-docx` | Word documents |
//! | [`xlsx`] | `openxml-xlsx` | Excel workbooks |
//! | [`pptx`] | `openxml-pptx` | PowerPoint presentations |
//! | [`schema`] | `openxml-schema` | Every ECMA-376 schema type, generated from the official XSDs |
//! | [`opc`] | `openxml-opc` | Open Packaging Conventions (ZIP, parts, relationships) |
//! | [`xml`] | `openxml-xml` | Namespace-aware XML reader/writer and raw nodes |
//! | [`core`] | `openxml-core` | Shared errors, units and image helpers |

#![warn(missing_docs)]

pub use openxml_core as core;
pub use openxml_docx as docx;
pub use openxml_opc as opc;
pub use openxml_pptx as pptx;
pub use openxml_schema as schema;
pub use openxml_xlsx as xlsx;
pub use openxml_xml as xml;

pub use openxml_core::{Error, Length, Result};
