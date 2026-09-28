//! Part names (ECMA-376 Part 2 §6.2.2) and relationship target resolution.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::error::{Error, Result};

/// The name of a part inside a package, e.g. `/word/document.xml`.
///
/// Part names are stored in their decoded form (the form used for ZIP item
/// names). Comparison, hashing and ordering are ASCII case-insensitive as
/// required by Part 2 §6.2.2.3.
#[derive(Clone)]
pub struct PartName {
    name: String,
    key: String,
}

fn invalid(name: &str, reason: &'static str) -> Error {
    Error::InvalidPartName {
        name: name.to_owned(),
        reason,
    }
}

impl PartName {
    /// Validates and creates a part name. The name must start with `/`.
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if !name.starts_with('/') {
            return Err(invalid(&name, "a part name must start with a forward slash"));
        }
        if name.len() == 1 {
            return Err(invalid(&name, "a part name must contain at least one segment"));
        }
        if name.ends_with('/') {
            return Err(invalid(&name, "a part name must not end with a forward slash"));
        }
        for segment in name[1..].split('/') {
            if segment.is_empty() {
                return Err(invalid(&name, "a part name must not contain empty segments"));
            }
            if segment.ends_with('.') {
                return Err(invalid(&name, "a segment must not end with a dot"));
            }
            if segment.contains('\\') {
                return Err(invalid(&name, "a segment must not contain a backslash"));
            }
            if segment.chars().any(|c| c.is_control()) {
                return Err(invalid(&name, "a segment must not contain control characters"));
            }
        }
        let key = name.to_ascii_lowercase();
        Ok(PartName { name, key })
    }

    /// Creates a part name from a ZIP item name (which has no leading slash).
    pub fn from_zip_name(item: &str) -> Result<Self> {
        PartName::new(format!("/{item}"))
    }

    /// The part name as a string (with leading slash).
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The ZIP item name corresponding to this part (without leading slash).
    pub fn zip_name(&self) -> &str {
        &self.name[1..]
    }

    /// The last segment, e.g. `document.xml`.
    pub fn file_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or("")
    }

    /// The extension of the last segment, without the dot (as written).
    pub fn extension(&self) -> Option<&str> {
        let file = self.file_name();
        file.rfind('.').map(|i| &file[i + 1..]).filter(|e| !e.is_empty())
    }

    /// The directory containing the part, with trailing slash, e.g. `/word/`.
    pub fn directory(&self) -> &str {
        let i = self.name.rfind('/').unwrap_or(0);
        &self.name[..=i]
    }

    /// Name of the relationships part that holds this part's relationships,
    /// e.g. `/word/_rels/document.xml.rels`.
    pub fn rels_part_name(&self) -> PartName {
        PartName::new(format!("{}_rels/{}.rels", self.directory(), self.file_name()))
            .expect("derived relationships part name is valid")
    }

    /// Whether this is a relationships part (`.../_rels/*.rels`).
    pub fn is_rels_part(&self) -> bool {
        self.key.ends_with(".rels") && self.directory().to_ascii_lowercase().ends_with("/_rels/")
    }

    /// For a relationships part, the source it describes: `Some(None)` for the
    /// package relationships (`/_rels/.rels`), `Some(Some(part))` for a part.
    pub fn rels_source(&self) -> Option<Option<PartName>> {
        if !self.is_rels_part() {
            return None;
        }
        let dir = self.directory();
        let parent = &dir[..dir.len() - "_rels/".len()];
        let file = self.file_name();
        let source_file = &file[..file.len() - ".rels".len()];
        if source_file.is_empty() {
            return (parent == "/").then_some(None);
        }
        PartName::new(format!("{parent}{source_file}")).ok().map(Some)
    }

    /// Resolves a relationship target against its source part (`None` means the
    /// package root). Fragments are removed and percent-encoding is decoded.
    pub fn resolve(source: Option<&PartName>, target: &str) -> Result<PartName> {
        let target = target.split('#').next().unwrap_or("");
        let decoded = percent_decode(target).ok_or_else(|| invalid(target, "invalid percent-encoding"))?;
        let decoded = decoded.replace('\\', "/");
        let joined = if decoded.starts_with('/') {
            decoded
        } else {
            let base = source.map_or("/", |s| s.directory());
            format!("{base}{decoded}")
        };
        let mut segments: Vec<&str> = Vec::new();
        for seg in joined.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                s => segments.push(s),
            }
        }
        PartName::new(format!("/{}", segments.join("/")))
    }

    /// Computes the relative reference from `source` (or the package root) to
    /// this part, percent-encoding characters that are not allowed in URIs.
    pub fn relative_to(&self, source: Option<&PartName>) -> String {
        let base_dir = source.map_or("/", |s| s.directory());
        let base: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
        let target: Vec<&str> = self.name.split('/').filter(|s| !s.is_empty()).collect();
        let (target_dirs, file) = target.split_at(target.len() - 1);
        let common = base
            .iter()
            .zip(target_dirs)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        let mut out = String::new();
        for _ in common..base.len() {
            out.push_str("../");
        }
        for seg in &target_dirs[common..] {
            out.push_str(&percent_encode(seg));
            out.push('/');
        }
        out.push_str(&percent_encode(file[0]));
        out
    }
}

impl PartialEq for PartName {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for PartName {}

impl Hash for PartName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state)
    }
}

impl PartialOrd for PartName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PartName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl fmt::Display for PartName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

impl fmt::Debug for PartName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartName({:?})", self.name)
    }
}

impl std::str::FromStr for PartName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        PartName::new(s)
    }
}

impl TryFrom<&str> for PartName {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        PartName::new(s)
    }
}

/// Decodes `%XX` escapes. Returns `None` for malformed escapes or invalid UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_owned());
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encodes a path segment: everything except RFC 3986 unreserved and
/// sub-delimiter characters (plus `:` and `@`) is escaped.
pub fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
            );
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pn(s: &str) -> PartName {
        PartName::new(s).unwrap()
    }

    #[test]
    fn validation() {
        assert!(PartName::new("/word/document.xml").is_ok());
        assert!(PartName::new("/a").is_ok());
        for bad in [
            "",
            "/",
            "word/document.xml",
            "/word/",
            "/word//a.xml",
            "/word/a.",
            "/a/./b",
            "/a\\b",
            "/a/\u{1}",
        ] {
            assert!(PartName::new(bad).is_err(), "{bad:?} should be rejected");
        }
        let err = PartName::new("/x/").unwrap_err();
        assert!(err.to_string().contains("forward slash"));
    }

    #[test]
    fn comparison_is_case_insensitive() {
        assert_eq!(pn("/Word/Document.XML"), pn("/word/document.xml"));
        let mut set = std::collections::HashSet::new();
        set.insert(pn("/A.xml"));
        assert!(set.contains(&pn("/a.xml")));
        assert!(pn("/a.xml") < pn("/B.xml"));
        assert_eq!(
            pn("/Word/Document.XML").as_str(),
            "/Word/Document.XML",
            "original case is kept"
        );
    }

    #[test]
    fn components() {
        let p = pn("/word/media/image1.PNG");
        assert_eq!(p.file_name(), "image1.PNG");
        assert_eq!(p.extension(), Some("PNG"));
        assert_eq!(p.directory(), "/word/media/");
        assert_eq!(p.zip_name(), "word/media/image1.PNG");
        assert_eq!(pn("/noext").extension(), None);
        assert_eq!(
            PartName::from_zip_name("xl/workbook.xml").unwrap(),
            pn("/xl/workbook.xml")
        );
        assert_eq!(format!("{p}"), "/word/media/image1.PNG");
        assert_eq!(format!("{p:?}"), "PartName(\"/word/media/image1.PNG\")");
        assert_eq!("/a".parse::<PartName>().unwrap(), pn("/a"));
        assert_eq!(PartName::try_from("/a").unwrap(), pn("/a"));
    }

    #[test]
    fn relationship_parts() {
        let doc = pn("/word/document.xml");
        let rels = doc.rels_part_name();
        assert_eq!(rels.as_str(), "/word/_rels/document.xml.rels");
        assert!(rels.is_rels_part());
        assert_eq!(rels.rels_source(), Some(Some(doc)));
        let pkg = pn("/_rels/.rels");
        assert!(pkg.is_rels_part());
        assert_eq!(pkg.rels_source(), Some(None));
        assert!(!pn("/word/document.xml").is_rels_part());
        assert_eq!(pn("/word/document.xml").rels_source(), None);
        assert_eq!(
            pn("/word/_rels/.rels").rels_source(),
            None,
            "only the root may hold package rels"
        );
    }

    #[test]
    fn resolve_targets() {
        let doc = pn("/word/document.xml");
        assert_eq!(
            PartName::resolve(Some(&doc), "media/image1.png").unwrap(),
            pn("/word/media/image1.png")
        );
        assert_eq!(
            PartName::resolve(Some(&doc), "../customXml/item1.xml").unwrap(),
            pn("/customXml/item1.xml")
        );
        assert_eq!(
            PartName::resolve(Some(&doc), "/xl/a.xml").unwrap(),
            pn("/xl/a.xml")
        );
        assert_eq!(PartName::resolve(None, "word/document.xml").unwrap(), doc);
        assert_eq!(PartName::resolve(None, "./word/document.xml").unwrap(), doc);
        assert_eq!(
            PartName::resolve(Some(&doc), "media/my%20pic.png#frag")
                .unwrap()
                .as_str(),
            "/word/media/my pic.png"
        );
        assert_eq!(
            PartName::resolve(Some(&doc), "media\\x.png").unwrap(),
            pn("/word/media/x.png")
        );
        assert!(PartName::resolve(Some(&doc), "%zz").is_err());
        assert!(PartName::resolve(None, "").is_err());
    }

    #[test]
    fn relative_references() {
        let doc = pn("/word/document.xml");
        assert_eq!(
            pn("/word/media/image1.png").relative_to(Some(&doc)),
            "media/image1.png"
        );
        assert_eq!(pn("/word/styles.xml").relative_to(Some(&doc)), "styles.xml");
        assert_eq!(
            pn("/customXml/item1.xml").relative_to(Some(&doc)),
            "../customXml/item1.xml"
        );
        assert_eq!(pn("/word/document.xml").relative_to(None), "word/document.xml");
        assert_eq!(
            pn("/ppt/slides/slide1.xml").relative_to(Some(&pn("/ppt/slideLayouts/slideLayout1.xml"))),
            "../slides/slide1.xml"
        );
        assert_eq!(pn("/a b/ü.xml").relative_to(None), "a%20b/%C3%BC.xml");
        // Every relative reference resolves back to the same part.
        for (target, source) in [
            ("/word/media/image1.png", Some("/word/document.xml")),
            ("/a b/ü.xml", None),
            ("/x/y/z.xml", Some("/x/q/w/e.xml")),
        ] {
            let t = pn(target);
            let s = source.map(pn);
            let rel = t.relative_to(s.as_ref());
            assert_eq!(PartName::resolve(s.as_ref(), &rel).unwrap(), t, "{rel}");
        }
    }

    #[test]
    fn percent_coding() {
        assert_eq!(percent_decode("a%20b%C3%BC").as_deref(), Some("a bü"));
        assert_eq!(percent_decode("plain").as_deref(), Some("plain"));
        assert_eq!(percent_decode("%2"), None);
        assert_eq!(percent_decode("%FF"), None);
        assert_eq!(percent_encode("a b#%"), "a%20b%23%25");
    }
}
