//! Typed cache of the secondary parts that belong to worksheets: drawings,
//! comments, legacy VML drawings and tables.
//!
//! Parts are parsed on first use and written back on save only if they were
//! changed, so untouched parts stay byte-identical like every other part.

use std::borrow::Cow;
use std::collections::BTreeMap;

use openxml_core::part::read_part;
use openxml_core::{Error, Result};
use openxml_opc::known::content_types as ct;
use openxml_opc::{Package, PartName};
use openxml_schema::{dml_spreadsheet_drawing as xdr, sml};
use openxml_xml::RawElement;

/// A typed secondary part.
#[derive(Debug, Clone)]
pub(crate) enum SideValue {
    Drawing(Box<xdr::CT_Drawing>),
    Comments(Box<sml::CT_Comments>),
    Vml(Box<RawElement>),
    Table(Box<sml::CT_Table>),
}

impl SideValue {
    fn default_content_type(&self) -> &'static str {
        match self {
            SideValue::Drawing(_) => ct::DRAWING,
            SideValue::Comments(_) => ct::SML_COMMENTS,
            SideValue::Vml(_) => ct::VML_DRAWING,
            SideValue::Table(_) => ct::SML_TABLE,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            SideValue::Drawing(d) => xdr::elements::WS_DR.to_bytes(d),
            SideValue::Comments(c) => sml::elements::COMMENTS.to_bytes(c),
            SideValue::Vml(v) => v.to_xml().into_bytes(),
            SideValue::Table(t) => sml::elements::TABLE.to_bytes(t),
        }
    }
}

#[derive(Debug)]
struct Entry {
    value: SideValue,
    dirty: bool,
}

/// Parsed secondary parts, keyed by part name.
#[derive(Debug, Default)]
pub(crate) struct SideParts {
    entries: BTreeMap<PartName, Entry>,
}

fn parse_vml(pkg: &Package, name: &PartName) -> Result<RawElement> {
    let part = pkg
        .part(name)
        .ok_or_else(|| Error::MissingPart(name.to_string()))?;
    RawElement::parse_bytes(part.data())
        .or_else(|first| {
            // Some producers write HTML-style `<br>` in VML text boxes.
            let text = openxml_xml::decode_xml_bytes(part.data()).map_err(|_| first.clone())?;
            let fixed = text.replace("<br>", "<br/>");
            if fixed == text {
                return Err(first);
            }
            RawElement::parse(&fixed).map_err(|_| first)
        })
        .map_err(|source| Error::Xml {
            part: name.to_string(),
            source,
        })
}

macro_rules! accessors {
    ($get:ident, $peek:ident, $variant:ident, $ty:ty, $load:expr) => {
        /// The part, parsed on first use and marked as changed.
        pub fn $get(&mut self, pkg: &Package, name: &PartName) -> Result<&mut $ty> {
            if !self.entries.contains_key(name) {
                let value: $ty = $load(pkg, name)?;
                self.entries.insert(
                    name.clone(),
                    Entry {
                        value: SideValue::$variant(Box::new(value)),
                        dirty: false,
                    },
                );
            }
            let entry = self.entries.get_mut(name).expect("inserted above");
            entry.dirty = true;
            match &mut entry.value {
                SideValue::$variant(v) => Ok(v),
                _ => Err(Error::InvalidDocument(format!(
                    "part {name} is not a {}",
                    stringify!($variant)
                ))),
            }
        }

        /// The part for reading: the cached value, else a fresh parse.
        pub fn $peek<'s>(&'s self, pkg: &Package, name: &PartName) -> Result<Cow<'s, $ty>> {
            match self.entries.get(name) {
                Some(Entry {
                    value: SideValue::$variant(v),
                    ..
                }) => Ok(Cow::Borrowed(&**v)),
                Some(_) => Err(Error::InvalidDocument(format!(
                    "part {name} is not a {}",
                    stringify!($variant)
                ))),
                None => $load(pkg, name).map(Cow::Owned),
            }
        }
    };
}

impl SideParts {
    accessors!(drawing, peek_drawing, Drawing, xdr::CT_Drawing, |p, n| read_part(
        p,
        n,
        &xdr::elements::WS_DR
    ));
    accessors!(comments, peek_comments, Comments, sml::CT_Comments, |p, n| {
        read_part(p, n, &sml::elements::COMMENTS)
    });
    accessors!(vml, peek_vml, Vml, RawElement, parse_vml);
    accessors!(table, peek_table, Table, sml::CT_Table, |p, n| read_part(
        p,
        n,
        &sml::elements::TABLE
    ));

    /// Adds a new part to the package and the cache.
    pub fn create(&mut self, pkg: &mut Package, name: &PartName, value: SideValue) -> Result<()> {
        pkg.add_part(name.clone(), value.default_content_type(), value.to_bytes())?;
        self.entries.insert(name.clone(), Entry { value, dirty: false });
        Ok(())
    }

    /// Forgets a cached part (after it was removed from the package).
    pub fn forget(&mut self, name: &PartName) {
        self.entries.remove(name);
    }

    /// Whether a part is cached.
    #[cfg(test)]
    pub fn is_cached(&self, name: &PartName) -> bool {
        self.entries.contains_key(name)
    }

    /// Writes the changed parts into the package.
    pub fn flush(&mut self, pkg: &mut Package) -> Result<()> {
        for (name, entry) in &mut self.entries {
            if !entry.dirty {
                continue;
            }
            if !pkg.contains(name) {
                // Removed through the package in the meantime.
                entry.dirty = false;
                continue;
            }
            let content_type = pkg
                .part(name)
                .map_or(entry.value.default_content_type(), |p| p.content_type())
                .to_owned();
            pkg.set_part(name.clone(), &content_type, entry.value.to_bytes())?;
            entry.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pn(s: &str) -> PartName {
        PartName::new(s).unwrap()
    }

    #[test]
    fn parts_are_parsed_once_and_written_only_when_changed() {
        let mut pkg = Package::new();
        let mut side = SideParts::default();
        let name = pn("/xl/comments1.xml");
        side.create(
            &mut pkg,
            &name,
            SideValue::Comments(Box::new(sml::CT_Comments {
                authors: Some(Box::default()),
                comment_list: Some(Box::default()),
                ..Default::default()
            })),
        )
        .unwrap();
        let before = pkg.part(&name).unwrap().data().to_vec();
        side.flush(&mut pkg).unwrap();
        assert_eq!(
            pkg.part(&name).unwrap().data(),
            before.as_slice(),
            "clean parts are not rewritten"
        );
        side.comments(&pkg, &name)
            .unwrap()
            .authors
            .as_mut()
            .unwrap()
            .author
            .push("Ann".into());
        side.flush(&mut pkg).unwrap();
        let written = String::from_utf8(pkg.part(&name).unwrap().data().to_vec()).unwrap();
        assert!(written.contains("<author>Ann</author>"), "{written}");
        assert_eq!(pkg.part(&name).unwrap().content_type(), ct::SML_COMMENTS);
        assert!(side.is_cached(&name));
        let peeked = side.peek_comments(&pkg, &name).unwrap();
        assert_eq!(peeked.authors.as_ref().unwrap().author, ["Ann"]);
        assert!(side.peek_table(&pkg, &name).is_err(), "wrong kind");
        assert!(side.drawing(&pkg, &name).is_err(), "wrong kind");
        side.forget(&name);
        assert!(!side.is_cached(&name));
        // Uncached parts are parsed from the package.
        let fresh = side.peek_comments(&pkg, &name).unwrap();
        assert!(matches!(fresh, Cow::Owned(_)));
        assert!(side.peek_drawing(&pkg, &pn("/missing.xml")).is_err());
    }

    #[test]
    fn vml_parts_round_trip_as_raw_xml() {
        let mut pkg = Package::new();
        let name = pn("/xl/drawings/vmlDrawing1.vml");
        let xml = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml"><v:shape id="a"/></xml>"#;
        pkg.add_part(name.clone(), ct::VML_DRAWING, xml.as_bytes().to_vec())
            .unwrap();
        let mut side = SideParts::default();
        let vml = side.vml(&pkg, &name).unwrap();
        vml.set_attr(openxml_xml::Ns::NONE, "x", "1");
        side.flush(&mut pkg).unwrap();
        let out = String::from_utf8(pkg.part(&name).unwrap().data().to_vec()).unwrap();
        assert_eq!(
            out,
            r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" x="1"><v:shape id="a"/></xml>"#
        );
        pkg.remove_part(&name);
        side.vml(&pkg, &name).unwrap();
        side.flush(&mut pkg).unwrap();
        assert!(
            !pkg.contains(&name),
            "parts removed from the package stay removed"
        );
    }
}
