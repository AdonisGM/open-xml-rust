use std::fmt;

/// Errors of the document-level APIs.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The package (ZIP container, content types, relationships) is invalid.
    Package(openxml_opc::Error),
    /// An XML part could not be parsed.
    Xml {
        /// Name of the part.
        part: String,
        /// Parser error.
        source: openxml_xml::Error,
    },
    /// A part required by the document type is missing.
    MissingPart(String),
    /// The document structure is invalid (e.g. the main part has the wrong type).
    InvalidDocument(String),
    /// A requested item (sheet, slide, style, …) does not exist.
    NotFound(String),
    /// An argument is out of range or malformed.
    InvalidArgument(String),
    /// An image could not be recognised.
    UnsupportedImage,
    /// I/O failure.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Package(e) => write!(f, "{e}"),
            Error::Xml { part, source } => write!(f, "{part}: {source}"),
            Error::MissingPart(p) => write!(f, "required part is missing: {p}"),
            Error::InvalidDocument(m) => write!(f, "invalid document: {m}"),
            Error::NotFound(m) => write!(f, "not found: {m}"),
            Error::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
            Error::UnsupportedImage => write!(f, "unsupported or unrecognised image format"),
            Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Package(e) => Some(e),
            Error::Xml { source, .. } => Some(source),
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<openxml_opc::Error> for Error {
    fn from(e: openxml_opc::Error) -> Self {
        match e {
            openxml_opc::Error::Io(io) => Error::Io(io),
            other => Error::Package(other),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Result alias of the document-level APIs.
pub type Result<T, E = Error> = std::result::Result<T, E>;
