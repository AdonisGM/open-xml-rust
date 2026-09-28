//! Open Packaging Conventions (ECMA-376 Part 2) for Office Open XML.
//!
//! A package is a ZIP container holding *parts* (named byte streams with a
//! content type) connected by *relationships*. This crate reads and writes
//! packages, validates part names, resolves relationship targets and manages
//! the `[Content_Types].xml` stream and core properties.
//!
//! ```
//! use openxml_opc::{Package, PartName, known::{content_types, rel_types}};
//!
//! let mut pkg = Package::new();
//! let doc = PartName::new("/word/document.xml")?;
//! pkg.add_part(doc.clone(), content_types::WML_DOCUMENT, b"<w:document/>".to_vec())?;
//! pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &doc)?;
//! let bytes = pkg.to_bytes()?;
//!
//! let reopened = Package::from_bytes(&bytes)?;
//! assert_eq!(reopened.main_part(), Some(doc));
//! # Ok::<(), openxml_opc::Error>(())
//! ```

#![warn(missing_docs)]

mod content_types;
mod core_properties;
mod error;
pub mod known;
mod package;
mod part_name;
mod relationships;

pub use content_types::{CONTENT_TYPES_ITEM, ContentTypes};
pub use core_properties::{CoreProperties, w3cdtf_from_unix, w3cdtf_now};
pub use error::{Error, Result};
pub use package::{FALLBACK_CONTENT_TYPE, Package, Part};
pub use part_name::{PartName, percent_decode, percent_encode};
pub use relationships::{Relationship, Relationships, TargetMode};
