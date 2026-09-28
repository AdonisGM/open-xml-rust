use std::fmt;

/// Errors produced while reading, manipulating or writing a package.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// I/O failure.
    Io(std::io::Error),
    /// The ZIP container is invalid.
    Zip(String),
    /// An XML part of the package infrastructure is invalid.
    Xml(openxml_xml::Error),
    /// A part name violates ECMA-376 Part 2 §6.2.2.
    InvalidPartName {
        /// The rejected name.
        name: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// The package has no `[Content_Types].xml` stream.
    MissingContentTypes,
    /// A part has no content type.
    MissingContentType(String),
    /// A part with this name already exists.
    DuplicatePart(String),
    /// A required part is absent.
    MissingPart(String),
    /// A relationship is invalid.
    InvalidRelationship(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Zip(e) => write!(f, "invalid ZIP container: {e}"),
            Error::Xml(e) => write!(f, "invalid package XML: {e}"),
            Error::InvalidPartName { name, reason } => {
                write!(f, "invalid part name {name:?}: {reason}")
            }
            Error::MissingContentTypes => write!(f, "the package has no [Content_Types].xml"),
            Error::MissingContentType(p) => write!(f, "part {p} has no content type"),
            Error::DuplicatePart(p) => write!(f, "part {p} already exists"),
            Error::MissingPart(p) => write!(f, "part {p} does not exist"),
            Error::InvalidRelationship(m) => write!(f, "invalid relationship: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Xml(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => Error::Io(io),
            other => Error::Zip(other.to_string()),
        }
    }
}

impl From<openxml_xml::Error> for Error {
    fn from(e: openxml_xml::Error) -> Self {
        Error::Xml(e)
    }
}

/// Result alias for package operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;
