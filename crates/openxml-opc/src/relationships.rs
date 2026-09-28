//! Relationships parts (ECMA-376 Part 2 §6.5).

use openxml_xml::{Ns, RawElement, XmlWriter};

use crate::error::{Error, Result};
use crate::known::canonical_relationship_type;

/// Whether a relationship targets a part inside the package or an external resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TargetMode {
    /// The target is a part in the package (the default).
    #[default]
    Internal,
    /// The target is an external resource (e.g. a hyperlink).
    External,
}

/// A single relationship.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Relationship {
    /// Identifier, unique within its relationships part (e.g. `rId1`).
    pub id: String,
    /// Relationship type URI (Strict types are mapped to Transitional on read).
    pub rel_type: String,
    /// Target URI as written (relative to the source part for internal targets).
    pub target: String,
    /// Internal or external target.
    pub target_mode: TargetMode,
}

impl Relationship {
    /// Whether the target is external.
    pub fn is_external(&self) -> bool {
        self.target_mode == TargetMode::External
    }
}

/// The relationships of one source (a part or the package itself).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relationships {
    items: Vec<Relationship>,
}

impl Relationships {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a relationships part.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = RawElement::parse_bytes(bytes)?;
        if !root.name.is(Ns::PR, "Relationships") {
            return Err(Error::Xml(openxml_xml::Error::UnexpectedRoot {
                expected: format!("{{{}}}Relationships", Ns::PR.uri()),
                found: format!("{{{}}}{}", root.name.uri(), root.name.local),
            }));
        }
        let mut rels = Relationships::new();
        for el in root.elements().filter(|e| e.name.is(Ns::PR, "Relationship")) {
            let (Some(id), Some(ty)) = (el.attr(Ns::NONE, "Id"), el.attr(Ns::NONE, "Type")) else {
                // Unusable entries are dropped, as Office applications do when repairing.
                continue;
            };
            let target = el.attr(Ns::NONE, "Target").unwrap_or("");
            let target_mode = match el.attr(Ns::NONE, "TargetMode") {
                Some("External") => TargetMode::External,
                _ => TargetMode::Internal,
            };
            if rels.get(id).is_some() {
                // Duplicate identifiers are invalid; keep the first occurrence.
                continue;
            }
            rels.items.push(Relationship {
                id: id.to_owned(),
                rel_type: canonical_relationship_type(ty).into_owned(),
                target: target.to_owned(),
                target_mode,
            });
        }
        Ok(rels)
    }

    /// Serializes the relationships part.
    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::with_declaration();
        w.predeclare(&[Ns::PR]);
        w.start(Ns::PR, "Relationships");
        for r in &self.items {
            w.start(Ns::PR, "Relationship");
            w.attr(Ns::NONE, "Id", &r.id);
            w.attr(Ns::NONE, "Type", &r.rel_type);
            w.attr(Ns::NONE, "Target", &r.target);
            if r.is_external() {
                w.attr(Ns::NONE, "TargetMode", "External");
            }
            w.end();
        }
        w.end();
        w.finish()
    }

    /// Number of relationships.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no relationships.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterates in document order.
    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.items.iter()
    }

    /// Looks up a relationship by identifier.
    pub fn get(&self, id: &str) -> Option<&Relationship> {
        self.items.iter().find(|r| r.id == id)
    }

    /// Relationships of a given type.
    pub fn by_type<'a>(&'a self, rel_type: &'a str) -> impl Iterator<Item = &'a Relationship> + 'a {
        self.items.iter().filter(move |r| r.rel_type == rel_type)
    }

    /// First relationship of a given type.
    pub fn first_by_type(&self, rel_type: &str) -> Option<&Relationship> {
        self.items.iter().find(|r| r.rel_type == rel_type)
    }

    /// Returns an identifier of the form `rIdN` that is not in use.
    pub fn next_id(&self) -> String {
        let max = self
            .items
            .iter()
            .filter_map(|r| r.id.strip_prefix("rId")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let mut n = max + 1;
        loop {
            let id = format!("rId{n}");
            if self.get(&id).is_none() {
                return id;
            }
            n += 1;
        }
    }

    /// Adds a relationship with a fresh identifier and returns the identifier.
    pub fn add(&mut self, rel_type: &str, target: &str, target_mode: TargetMode) -> String {
        let id = self.next_id();
        self.items.push(Relationship {
            id: id.clone(),
            rel_type: rel_type.to_owned(),
            target: target.to_owned(),
            target_mode,
        });
        id
    }

    /// Inserts a relationship with an explicit identifier.
    pub fn insert(&mut self, rel: Relationship) -> Result<()> {
        if rel.id.is_empty() || self.get(&rel.id).is_some() {
            return Err(Error::InvalidRelationship(format!(
                "duplicate or empty id {:?}",
                rel.id
            )));
        }
        self.items.push(rel);
        Ok(())
    }

    /// Removes a relationship by identifier.
    pub fn remove(&mut self, id: &str) -> Option<Relationship> {
        let i = self.items.iter().position(|r| r.id == id)?;
        Some(self.items.remove(i))
    }

    /// Keeps only the relationships for which `f` returns `true`.
    pub fn retain(&mut self, f: impl FnMut(&Relationship) -> bool) {
        self.items.retain(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::known::rel_types;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/?a=1&amp;b=2" TargetMode="External"/></Relationships>"#;

    #[test]
    fn parses_relationships() {
        let rels = Relationships::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(rels.len(), 4);
        let main = rels.first_by_type(rel_types::OFFICE_DOCUMENT).unwrap();
        assert_eq!(main.id, "rId1", "Strict relationship type is canonicalised");
        assert_eq!(main.target, "word/document.xml");
        let link = rels.get("rId9").unwrap();
        assert!(link.is_external());
        assert_eq!(link.target, "https://example.com/?a=1&b=2");
        assert_eq!(rels.by_type(rel_types::HYPERLINK).count(), 1);
    }

    #[test]
    fn round_trips() {
        let rels = Relationships::parse(SAMPLE.as_bytes()).unwrap();
        let xml = rels.to_xml();
        assert!(xml.contains(r#"TargetMode="External""#));
        assert_eq!(Relationships::parse(xml.as_bytes()).unwrap(), rels);
    }

    #[test]
    fn identifiers() {
        let mut rels = Relationships::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(rels.next_id(), "rId10");
        let id = rels.add(rel_types::STYLES, "styles.xml", TargetMode::Internal);
        assert_eq!(id, "rId10");
        assert!(
            rels.insert(Relationship {
                id: "rId10".into(),
                rel_type: "x".into(),
                target: "y".into(),
                target_mode: TargetMode::Internal
            })
            .is_err()
        );
        assert!(
            rels.insert(Relationship {
                id: "custom".into(),
                rel_type: "x".into(),
                target: "y".into(),
                target_mode: TargetMode::Internal
            })
            .is_ok()
        );
        assert_eq!(rels.remove("custom").unwrap().target, "y");
        assert!(rels.remove("custom").is_none());
        rels.retain(|r| !r.is_external());
        assert_eq!(rels.len(), 4);
        let mut empty = Relationships::new();
        assert!(empty.is_empty());
        assert_eq!(empty.add("t", "a", TargetMode::External), "rId1");
        assert_eq!(empty.iter().count(), 1);
    }

    #[test]
    fn duplicate_ids_keep_first_and_incomplete_entries_are_skipped() {
        let xml = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="a" Type="t1" Target="x"/><Relationship Id="a" Type="t2" Target="y"/></Relationships>"#;
        let rels = Relationships::parse(xml.as_bytes()).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels.get("a").unwrap().rel_type, "t1");
        let bad = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="x"/><Relationship Id="b" Type="t" Target="y"/></Relationships>"#;
        let rels = Relationships::parse(bad.as_bytes()).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels.get("b").unwrap().target, "y");
        assert!(Relationships::parse(b"<Types/>").is_err());
    }
}
