//! Shared building blocks of the document-level APIs
//! (`openxml-docx`, `openxml-xlsx`, `openxml-pptx`).
//!
//! * [`Error`] / [`Result`] — the error type of all document APIs;
//! * [`part`] — reading and writing typed XML parts of a package;
//! * [`units`] — lengths in EMU/twips/points and font sizes;
//! * [`image`] — image format and size detection;
//! * [`properties`] — custom and extended document properties.

#![warn(missing_docs)]

mod error;
pub mod image;
pub mod part;
pub mod properties;
pub mod units;

pub use error::{Error, Result};
pub use image::{ImageFormat, ImageInfo, sniff_image};
pub use units::{FontSize, Length};

pub use openxml_opc as opc;
pub use openxml_schema as schema;
pub use openxml_xml as xml;
