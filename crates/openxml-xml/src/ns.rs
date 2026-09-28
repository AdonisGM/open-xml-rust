//! Registry of the XML namespaces the toolkit understands.
//!
//! Every namespace that appears in the ECMA-376 schemas (plus the handful of
//! infrastructure namespaces used by packages and markup compatibility) gets a
//! compact [`Ns`] identifier. Generated code matches on `(Ns, local_name)`
//! pairs instead of comparing namespace URIs, which keeps parsing fast.
//!
//! Strict-conformance documents (ECMA-376 Part 1 "Strict") use different
//! namespace URIs from Transitional documents. [`Ns::from_uri`] maps both
//! spellings to the same identifier, so a Strict document is read into the
//! same object model and is written back using Transitional URIs.

use std::fmt;

/// Compact identifier of a known XML namespace.
///
/// Two special values exist: [`Ns::NONE`] for names without a namespace and
/// [`Ns::OTHER`] for namespaces that are not part of the registry (their URI
/// is carried separately, e.g. in [`crate::RawName`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ns(u16);

struct NsInfo {
    ident: &'static str,
    prefix: &'static str,
    uri: &'static str,
    strict_uri: Option<&'static str>,
    default_on_root: bool,
}

macro_rules! namespaces {
    ($(
        $(#[$meta:meta])*
        $id:ident = $idx:literal, $prefix:literal, $uri:literal
            $(, strict = $strict:literal)? $(, default_on_root = $dor:literal)?;
    )*) => {
        impl Ns {
            $( $(#[$meta])* pub const $id: Ns = Ns($idx); )*
        }

        static TABLE: &[NsInfo] = &[
            $( NsInfo {
                ident: stringify!($id),
                prefix: $prefix,
                uri: $uri,
                strict_uri: namespaces!(@opt $($strict)?),
                default_on_root: namespaces!(@bool $($dor)?),
            }, )*
        ];

        #[allow(unreachable_patterns)]
        fn lookup_uri(uri: &str) -> Option<Ns> {
            match uri {
                "" => None,
                $( $uri => Some(Ns::$id), )*
                $( $( $strict => Some(Ns::$id), )? )*
                _ => None,
            }
        }
    };
    (@opt) => { None };
    (@opt $v:literal) => { Some($v) };
    (@bool) => { false };
    (@bool $v:literal) => { $v };
}

namespaces! {
    /// No namespace (unqualified names).
    NONE = 0, "", "";
    /// A namespace that is not part of the registry.
    OTHER = 1, "", "\u{0}other";
    /// The `xml:` namespace (always bound, never declared).
    XML = 2, "xml", "http://www.w3.org/XML/1998/namespace";
    /// The `xmlns` pseudo-namespace used for namespace declarations.
    XMLNS = 3, "xmlns", "http://www.w3.org/2000/xmlns/";

    /// WordprocessingML main (`w:`), ECMA-376 Part 1 §17.
    W = 4, "w", "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        strict = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    /// SpreadsheetML main (`x:`), ECMA-376 Part 1 §18.
    X = 5, "x", "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
        strict = "http://purl.oclc.org/ooxml/spreadsheetml/main", default_on_root = true;
    /// PresentationML main (`p:`), ECMA-376 Part 1 §19.
    P = 6, "p", "http://schemas.openxmlformats.org/presentationml/2006/main",
        strict = "http://purl.oclc.org/ooxml/presentationml/main";
    /// DrawingML main (`a:`), ECMA-376 Part 1 §20.1 and §21.1.
    A = 7, "a", "http://schemas.openxmlformats.org/drawingml/2006/main",
        strict = "http://purl.oclc.org/ooxml/drawingml/main";
    /// DrawingML picture (`pic:`), §20.2.
    PIC = 8, "pic", "http://schemas.openxmlformats.org/drawingml/2006/picture",
        strict = "http://purl.oclc.org/ooxml/drawingml/picture";
    /// DrawingML chart (`c:`), §21.2.
    C = 9, "c", "http://schemas.openxmlformats.org/drawingml/2006/chart",
        strict = "http://purl.oclc.org/ooxml/drawingml/chart";
    /// DrawingML chart drawing (`cdr:`), §21.3.
    CDR = 10, "cdr", "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing",
        strict = "http://purl.oclc.org/ooxml/drawingml/chartDrawing";
    /// DrawingML diagram (`dgm:`), §21.4.
    DGM = 11, "dgm", "http://schemas.openxmlformats.org/drawingml/2006/diagram",
        strict = "http://purl.oclc.org/ooxml/drawingml/diagram";
    /// DrawingML locked canvas (`lc:`), §20.3.
    LC = 12, "lc", "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas",
        strict = "http://purl.oclc.org/ooxml/drawingml/lockedCanvas";
    /// DrawingML WordprocessingML drawing (`wp:`), §20.4.
    WP = 13, "wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        strict = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
    /// DrawingML SpreadsheetML drawing (`xdr:`), §20.5.
    XDR = 14, "xdr", "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
        strict = "http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing";
    /// Office document relationship references (`r:`), §22.8.
    R = 15, "r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        strict = "http://purl.oclc.org/ooxml/officeDocument/relationships";
    /// Office Math (`m:`), §22.1.
    M = 16, "m", "http://schemas.openxmlformats.org/officeDocument/2006/math",
        strict = "http://purl.oclc.org/ooxml/officeDocument/math";
    /// Shared simple types (`s:`), §22.9.
    S = 17, "s", "http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes",
        strict = "http://purl.oclc.org/ooxml/officeDocument/sharedTypes";
    /// Extended (application) file properties, §22.2.
    EP = 18, "ep", "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        strict = "http://purl.oclc.org/ooxml/officeDocument/extendedProperties", default_on_root = true;
    /// Custom file properties, §22.3.
    OP = 19, "op", "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
        strict = "http://purl.oclc.org/ooxml/officeDocument/customProperties", default_on_root = true;
    /// Variant types (`vt:`), §22.4.
    VT = 20, "vt", "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
        strict = "http://purl.oclc.org/ooxml/officeDocument/docPropsVTypes";
    /// Custom XML data properties (`ds:`), §22.5.
    DS = 21, "ds", "http://schemas.openxmlformats.org/officeDocument/2006/customXml",
        strict = "http://purl.oclc.org/ooxml/officeDocument/customXml";
    /// Bibliography (`b:`), §22.6.
    B = 22, "b", "http://schemas.openxmlformats.org/officeDocument/2006/bibliography",
        strict = "http://purl.oclc.org/ooxml/officeDocument/bibliography";
    /// Additional characteristics (`ac:`), §22.7.
    AC = 23, "ac", "http://schemas.openxmlformats.org/officeDocument/2006/characteristics",
        strict = "http://purl.oclc.org/ooxml/officeDocument/characteristics";
    /// Custom XML schema references (`sl:`), §23.
    SL = 24, "sl", "http://schemas.openxmlformats.org/schemaLibrary/2006/main",
        strict = "http://purl.oclc.org/ooxml/schemaLibrary/main";

    /// VML main (`v:`), ECMA-376 Part 4 §19.1.
    V = 25, "v", "urn:schemas-microsoft-com:vml";
    /// VML Office drawing (`o:`), Part 4 §19.2.
    O = 26, "o", "urn:schemas-microsoft-com:office:office";
    /// VML WordprocessingML drawing (`w10:`), Part 4 §19.3.
    W10 = 27, "w10", "urn:schemas-microsoft-com:office:word";
    /// VML SpreadsheetML drawing (`x:` in VML parts), Part 4 §19.4.
    XVML = 28, "x", "urn:schemas-microsoft-com:office:excel";
    /// VML PresentationML drawing (`pvml:`), Part 4 §19.5.
    PVML = 29, "pvml", "urn:schemas-microsoft-com:office:powerpoint";

    /// Markup Compatibility and Extensibility (`mc:`), ECMA-376 Part 3.
    MC = 30, "mc", "http://schemas.openxmlformats.org/markup-compatibility/2006";
    /// OPC content types stream, ECMA-376 Part 2.
    CT = 31, "ct", "http://schemas.openxmlformats.org/package/2006/content-types",
        default_on_root = true;
    /// OPC relationships part, ECMA-376 Part 2.
    PR = 32, "pr", "http://schemas.openxmlformats.org/package/2006/relationships",
        default_on_root = true;
    /// OPC core properties (`cp:`), ECMA-376 Part 2.
    CP = 33, "cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
    /// Dublin Core elements (`dc:`).
    DC = 34, "dc", "http://purl.org/dc/elements/1.1/";
    /// Dublin Core terms (`dcterms:`).
    DCTERMS = 35, "dcterms", "http://purl.org/dc/terms/";
    /// Dublin Core types (`dcmitype:`).
    DCMITYPE = 36, "dcmitype", "http://purl.org/dc/dcmitype/";
    /// XML Schema instance (`xsi:`).
    XSI = 37, "xsi", "http://www.w3.org/2001/XMLSchema-instance";
}

impl Ns {
    /// Looks up a namespace by URI. Both Transitional and Strict URIs are accepted.
    ///
    /// Returns `None` for the empty URI and for namespaces outside the registry.
    pub fn from_uri(uri: &str) -> Option<Ns> {
        lookup_uri(uri)
    }

    /// Resolves a URI to an identifier, mapping unknown URIs to [`Ns::OTHER`]
    /// and the empty URI to [`Ns::NONE`].
    pub fn classify(uri: &str) -> Ns {
        if uri.is_empty() {
            Ns::NONE
        } else {
            lookup_uri(uri).unwrap_or(Ns::OTHER)
        }
    }

    fn info(self) -> &'static NsInfo {
        &TABLE[self.0 as usize]
    }

    /// The canonical (Transitional) namespace URI. Empty for [`Ns::NONE`] and [`Ns::OTHER`].
    pub fn uri(self) -> &'static str {
        match self {
            Ns::OTHER => "",
            _ => self.info().uri,
        }
    }

    /// The Strict-conformance URI, if this namespace has one.
    pub fn strict_uri(self) -> Option<&'static str> {
        self.info().strict_uri
    }

    /// The conventional prefix used when a declaration has to be generated.
    pub fn prefix(self) -> &'static str {
        self.info().prefix
    }

    /// Whether new documents conventionally bind this namespace as the default
    /// namespace on the root element (e.g. SpreadsheetML and OPC parts).
    pub fn default_on_root(self) -> bool {
        self.info().default_on_root
    }

    /// Whether this is a real namespace from the registry.
    pub fn is_known(self) -> bool {
        self != Ns::NONE && self != Ns::OTHER
    }

    /// The numeric index of this identifier (stable within a build).
    pub fn index(self) -> u16 {
        self.0
    }

    /// Iterates over every registered namespace (excluding `NONE` and `OTHER`).
    pub fn all() -> impl Iterator<Item = Ns> {
        (2..TABLE.len() as u16).map(Ns)
    }

    /// The constant name of this namespace, e.g. `"W"`.
    pub fn ident(self) -> &'static str {
        self.info().ident
    }
}

impl fmt::Debug for Ns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ns::{}", self.ident())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_indices_match_constants() {
        for (i, info) in TABLE.iter().enumerate() {
            let ns = Ns(i as u16);
            assert_eq!(ns.ident(), info.ident, "index {i} is out of order");
        }
    }

    #[test]
    fn uris_round_trip_through_lookup() {
        for ns in Ns::all() {
            assert_eq!(Ns::from_uri(ns.uri()), Some(ns), "{ns:?}");
            if let Some(strict) = ns.strict_uri() {
                assert_eq!(Ns::from_uri(strict), Some(ns), "strict {ns:?}");
            }
        }
    }

    #[test]
    fn uris_and_prefixes_are_unique() {
        let mut uris = std::collections::HashSet::new();
        for ns in Ns::all() {
            assert!(uris.insert(ns.uri()), "duplicate uri {}", ns.uri());
            if let Some(s) = ns.strict_uri() {
                assert!(uris.insert(s), "duplicate strict uri {s}");
            }
        }
        // Prefixes may legitimately collide only for XVML ("x" in VML parts) and X.
        let mut prefixes = std::collections::HashMap::new();
        for ns in Ns::all() {
            if let Some(prev) = prefixes.insert(ns.prefix(), ns) {
                assert!(
                    matches!((prev, ns), (Ns::X, Ns::XVML)),
                    "unexpected prefix clash {prev:?} / {ns:?}"
                );
            }
        }
    }

    #[test]
    fn strict_maps_to_transitional() {
        let ns = Ns::from_uri("http://purl.oclc.org/ooxml/wordprocessingml/main").unwrap();
        assert_eq!(ns, Ns::W);
        assert_eq!(
            ns.uri(),
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
        );
    }

    #[test]
    fn classify_handles_special_cases() {
        assert_eq!(Ns::classify(""), Ns::NONE);
        assert_eq!(Ns::classify("urn:example"), Ns::OTHER);
        assert_eq!(Ns::classify(Ns::A.uri()), Ns::A);
        assert!(!Ns::NONE.is_known());
        assert!(!Ns::OTHER.is_known());
        assert!(Ns::MC.is_known());
        assert_eq!(Ns::OTHER.uri(), "");
    }

    #[test]
    fn default_on_root_flags() {
        assert!(Ns::X.default_on_root());
        assert!(Ns::CT.default_on_root());
        assert!(!Ns::W.default_on_root());
        assert!(!Ns::P.default_on_root());
    }
}
