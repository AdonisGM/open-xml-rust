use std::fmt;

/// Errors produced while reading or interpreting XML.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The document is not well-formed XML.
    Syntax(String),
    /// The input ended before the document was complete.
    UnexpectedEof,
    /// The input is not valid UTF-8 / UTF-16 text.
    Encoding(String),
    /// The document has no root element.
    NoRootElement,
    /// The root element is not the one the caller asked for.
    UnexpectedRoot {
        /// Expected `{namespace}local` name.
        expected: String,
        /// Actual `{namespace}local` name.
        found: String,
    },
    /// A value could not be parsed as the type required by the schema.
    InvalidValue {
        /// Name of the element or attribute holding the value.
        name: String,
        /// The offending lexical value.
        value: String,
    },
    /// The element nesting exceeds [`crate::MAX_DEPTH`].
    TooDeep,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax(msg) => write!(f, "malformed XML: {msg}"),
            Error::UnexpectedEof => write!(f, "unexpected end of XML input"),
            Error::Encoding(msg) => write!(f, "invalid text encoding: {msg}"),
            Error::NoRootElement => write!(f, "the XML document has no root element"),
            Error::UnexpectedRoot { expected, found } => {
                write!(f, "unexpected root element {found}, expected {expected}")
            }
            Error::InvalidValue { name, value } => {
                write!(f, "invalid value {value:?} for {name}")
            }
            Error::TooDeep => write!(f, "element nesting is too deep"),
        }
    }
}

impl std::error::Error for Error {}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        match e {
            quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::UnclosedTag)
            | quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::UnclosedComment)
            | quick_xml::Error::Syntax(quick_xml::errors::SyntaxError::UnclosedCData) => Error::UnexpectedEof,
            other => Error::Syntax(other.to_string()),
        }
    }
}

impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        Error::Syntax(e.to_string())
    }
}

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
