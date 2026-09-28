//! The content types stream `[Content_Types].xml` (ECMA-376 Part 2 §8.1.2).

use openxml_xml::{Ns, RawElement, XmlWriter};

use crate::error::{Error, Result};
use crate::known::content_types as ct;
use crate::part_name::{PartName, percent_decode};

/// Name of the content types stream inside the ZIP container.
pub const CONTENT_TYPES_ITEM: &str = "[Content_Types].xml";

/// Maps parts to content types through extension defaults and per-part overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTypes {
    defaults: Vec<(String, String)>,
    overrides: Vec<(PartName, String)>,
}

impl Default for ContentTypes {
    fn default() -> Self {
        ContentTypes {
            defaults: vec![
                ("rels".into(), ct::RELATIONSHIPS.into()),
                ("xml".into(), ct::XML.into()),
            ],
            overrides: Vec::new(),
        }
    }
}

impl ContentTypes {
    /// Creates a table with the `rels` and `xml` defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty table (no defaults).
    pub fn empty() -> Self {
        ContentTypes {
            defaults: Vec::new(),
            overrides: Vec::new(),
        }
    }

    /// Parses `[Content_Types].xml`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = RawElement::parse_bytes(bytes)?;
        if !root.name.is(Ns::CT, "Types") {
            return Err(Error::Xml(openxml_xml::Error::UnexpectedRoot {
                expected: format!("{{{}}}Types", Ns::CT.uri()),
                found: format!("{{{}}}{}", root.name.uri(), root.name.local),
            }));
        }
        let mut table = ContentTypes::empty();
        for el in root.elements() {
            if el.name.is(Ns::CT, "Default") {
                if let (Some(ext), Some(ty)) =
                    (el.attr(Ns::NONE, "Extension"), el.attr(Ns::NONE, "ContentType"))
                {
                    table.set_default(ext, ty);
                }
            } else if el.name.is(Ns::CT, "Override")
                && let (Some(name), Some(ty)) =
                    (el.attr(Ns::NONE, "PartName"), el.attr(Ns::NONE, "ContentType"))
            {
                let decoded = percent_decode(name).unwrap_or_else(|| name.to_owned());
                table.set_override(PartName::new(decoded)?, ty);
            }
        }
        Ok(table)
    }

    /// Serializes the table.
    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::with_declaration();
        w.predeclare(&[Ns::CT]);
        w.start(Ns::CT, "Types");
        for (ext, ty) in &self.defaults {
            w.start(Ns::CT, "Default");
            w.attr(Ns::NONE, "Extension", ext);
            w.attr(Ns::NONE, "ContentType", ty);
            w.end();
        }
        for (name, ty) in &self.overrides {
            w.start(Ns::CT, "Override");
            w.attr(Ns::NONE, "PartName", name.as_str());
            w.attr(Ns::NONE, "ContentType", ty);
            w.end();
        }
        w.end();
        w.finish()
    }

    /// The content type of a part: its override, else the default for its extension.
    pub fn content_type(&self, part: &PartName) -> Option<&str> {
        if let Some((_, ty)) = self.overrides.iter().find(|(n, _)| n == part) {
            return Some(ty);
        }
        let ext = part.extension()?;
        self.default_for(ext)
    }

    /// The default content type registered for an extension (case-insensitive).
    pub fn default_for(&self, ext: &str) -> Option<&str> {
        self.defaults
            .iter()
            .find(|(e, _)| e.eq_ignore_ascii_case(ext))
            .map(|(_, t)| t.as_str())
    }

    /// Adds or replaces an extension default.
    pub fn set_default(&mut self, ext: &str, content_type: &str) {
        match self
            .defaults
            .iter_mut()
            .find(|(e, _)| e.eq_ignore_ascii_case(ext))
        {
            Some(entry) => entry.1 = content_type.to_owned(),
            None => self.defaults.push((ext.to_owned(), content_type.to_owned())),
        }
    }

    /// Adds or replaces a part override.
    pub fn set_override(&mut self, part: PartName, content_type: &str) {
        match self.overrides.iter_mut().find(|(n, _)| *n == part) {
            Some(entry) => entry.1 = content_type.to_owned(),
            None => self.overrides.push((part, content_type.to_owned())),
        }
    }

    /// Removes a part override.
    pub fn remove_override(&mut self, part: &PartName) -> Option<String> {
        let i = self.overrides.iter().position(|(n, _)| n == part)?;
        Some(self.overrides.remove(i).1)
    }

    /// Extension defaults in document order.
    pub fn defaults(&self) -> impl Iterator<Item = (&str, &str)> {
        self.defaults.iter().map(|(e, t)| (e.as_str(), t.as_str()))
    }

    /// Part overrides in document order.
    pub fn overrides(&self) -> impl Iterator<Item = (&PartName, &str)> {
        self.overrides.iter().map(|(n, t)| (n, t.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="PNG" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/my%20dir/a.xml" ContentType="text/x"/></Types>"#;

    fn pn(s: &str) -> PartName {
        PartName::new(s).unwrap()
    }

    #[test]
    fn parses_defaults_and_overrides() {
        let t = ContentTypes::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(t.content_type(&pn("/word/document.xml")), Some(ct::WML_DOCUMENT));
        assert_eq!(t.content_type(&pn("/WORD/Document.xml")), Some(ct::WML_DOCUMENT));
        assert_eq!(t.content_type(&pn("/word/styles.xml")), Some(ct::XML));
        assert_eq!(
            t.content_type(&pn("/media/a.png")),
            Some("image/png"),
            "extension lookup is case-insensitive"
        );
        assert_eq!(
            t.content_type(&pn("/my dir/a.xml")),
            Some("text/x"),
            "override names are percent-decoded"
        );
        assert_eq!(t.content_type(&pn("/noext")), None);
        assert_eq!(t.content_type(&pn("/a.bin")), None);
        assert_eq!(t.defaults().count(), 3);
        assert_eq!(t.overrides().count(), 2);
    }

    #[test]
    fn round_trips() {
        let t = ContentTypes::parse(SAMPLE.as_bytes()).unwrap();
        let xml = t.to_xml();
        assert!(xml.starts_with("<?xml"));
        assert!(
            xml.contains(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#)
        );
        assert_eq!(ContentTypes::parse(xml.as_bytes()).unwrap(), t);
    }

    #[test]
    fn editing() {
        let mut t = ContentTypes::new();
        assert_eq!(t.default_for("RELS"), Some(ct::RELATIONSHIPS));
        t.set_default("png", "image/png");
        t.set_default("PNG", "image/x-png");
        assert_eq!(t.defaults().count(), 3);
        assert_eq!(t.default_for("png"), Some("image/x-png"));
        t.set_override(pn("/a.xml"), "a");
        t.set_override(pn("/A.xml"), "b");
        assert_eq!(t.overrides().count(), 1);
        assert_eq!(t.content_type(&pn("/a.xml")), Some("b"));
        assert_eq!(t.remove_override(&pn("/a.xml")).as_deref(), Some("b"));
        assert_eq!(t.remove_override(&pn("/a.xml")), None);
        assert_eq!(t.content_type(&pn("/a.xml")), Some(ct::XML));
        assert_eq!(ContentTypes::empty().defaults().count(), 0);
    }

    #[test]
    fn rejects_wrong_root() {
        assert!(ContentTypes::parse(b"<Other/>").is_err());
        assert!(ContentTypes::parse(b"not xml").is_err());
    }
}
