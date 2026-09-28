//! XML infrastructure for the Open XML toolkit.
//!
//! This crate provides the pieces every higher layer builds on:
//!
//! * [`Ns`] — the registry of namespaces used by Office Open XML, including
//!   the mapping of Strict-conformance URIs onto their Transitional twins;
//! * [`XmlReader`] — a namespace-aware pull parser (built on `quick-xml`);
//! * [`XmlWriter`] — a writer that manages namespace prefixes automatically;
//! * [`RawElement`] and friends — schema-less nodes used to preserve content
//!   the typed model does not describe;
//! * [`XmlValue`], [`XmlRead`], [`XmlWrite`] — the traits implemented by the
//!   generated schema types.

#![warn(missing_docs)]

mod element;
mod error;
mod ns;
mod raw;
mod reader;
mod value;
mod writer;

pub use element::{ElementDef, XmlRead, XmlWrite, read_document, rt};
pub use error::{Error, Result};
pub use ns::Ns;
pub use raw::{ExtraChild, RawAttribute, RawElement, RawName, RawNode};
pub use reader::{Attr, Event, MAX_DEPTH, StartTag, XmlReader, decode_xml_bytes};
pub use value::{Base64Binary, HexBinary, XmlList, XmlValue};
pub use writer::{XML_DECLARATION, XmlWriter, escape_attr, escape_text};
