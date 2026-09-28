//! Reading and writing typed XML parts of a package.

use openxml_opc::{Package, PartName};
use openxml_xml::{ElementDef, XmlRead, XmlWrite};

use crate::error::{Error, Result};

/// Parses a part with the given root element definition.
pub fn read_part<T: XmlRead>(pkg: &Package, name: &PartName, def: &ElementDef<T>) -> Result<T> {
    let part = pkg
        .part(name)
        .ok_or_else(|| Error::MissingPart(name.to_string()))?;
    def.parse_bytes(part.data()).map_err(|source| Error::Xml {
        part: name.to_string(),
        source,
    })
}

/// Serializes `value` into a part, creating the part or replacing its data
/// (existing relationships are kept).
pub fn write_part<T: XmlWrite>(
    pkg: &mut Package,
    name: &PartName,
    content_type: &str,
    def: &ElementDef<T>,
    value: &T,
) -> Result<()> {
    pkg.set_part(name.clone(), content_type, def.to_bytes(value))?;
    Ok(())
}

/// Resolves the first relationship of `rel_type` from `source` and parses its target.
///
/// Returns `Ok(None)` when there is no such relationship or the target part is absent.
pub fn read_related<T: XmlRead>(
    pkg: &Package,
    source: Option<&PartName>,
    rel_type: &str,
    def: &ElementDef<T>,
) -> Result<Option<(PartName, T)>> {
    match pkg.related_part(source, rel_type) {
        Some(name) if pkg.contains(&name) => {
            let value = read_part(pkg, &name, def)?;
            Ok(Some((name, value)))
        }
        _ => Ok(None),
    }
}

/// Adds a part at the first free name matching `pattern` (containing `{}`)
/// and relates it from `source`. Returns the part name and relationship id.
pub fn add_related_part(
    pkg: &mut Package,
    source: Option<&PartName>,
    rel_type: &str,
    pattern: &str,
    content_type: &str,
    data: Vec<u8>,
) -> Result<(PartName, String)> {
    let name = pkg.next_part_name(pattern)?;
    pkg.add_part(name.clone(), content_type, data)?;
    let id = pkg.add_relationship(source, rel_type, &name)?;
    Ok((name, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxml_opc::known::{content_types as ct, rel_types};
    use openxml_schema::wml;

    #[test]
    fn typed_parts_round_trip_through_a_package() {
        let mut pkg = Package::new();
        let name = PartName::new("/word/document.xml").unwrap();
        let doc = wml::CT_Document {
            body: Some(Box::default()),
            ..Default::default()
        };
        write_part(&mut pkg, &name, ct::WML_DOCUMENT, &wml::elements::DOCUMENT, &doc).unwrap();
        pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &name)
            .unwrap();
        let mut back = read_part(&pkg, &name, &wml::elements::DOCUMENT).unwrap();
        back.extra_attrs.clear();
        assert_eq!(back, doc);
        let (found, _) = read_related(&pkg, None, rel_types::OFFICE_DOCUMENT, &wml::elements::DOCUMENT)
            .unwrap()
            .unwrap();
        assert_eq!(found, name);
        assert!(
            read_related(&pkg, None, rel_types::STYLES, &wml::elements::STYLES)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn errors_name_the_part() {
        let mut pkg = Package::new();
        let name = PartName::new("/bad.xml").unwrap();
        assert!(matches!(
            read_part(&pkg, &name, &wml::elements::DOCUMENT),
            Err(Error::MissingPart(_))
        ));
        pkg.add_part(name.clone(), ct::XML, b"<nope/>".to_vec()).unwrap();
        let err = read_part(&pkg, &name, &wml::elements::DOCUMENT).unwrap_err();
        assert!(err.to_string().starts_with("/bad.xml: "), "{err}");
    }

    #[test]
    fn related_parts_get_fresh_names() {
        let mut pkg = Package::new();
        let doc = PartName::new("/word/document.xml").unwrap();
        pkg.add_part(doc.clone(), ct::WML_DOCUMENT, vec![]).unwrap();
        let (a, id_a) = add_related_part(
            &mut pkg,
            Some(&doc),
            rel_types::IMAGE,
            "/word/media/image{}.png",
            ct::PNG,
            vec![1],
        )
        .unwrap();
        let (b, id_b) = add_related_part(
            &mut pkg,
            Some(&doc),
            rel_types::IMAGE,
            "/word/media/image{}.png",
            ct::PNG,
            vec![2],
        )
        .unwrap();
        assert_eq!(a.as_str(), "/word/media/image1.png");
        assert_eq!(b.as_str(), "/word/media/image2.png");
        assert_ne!(id_a, id_b);
        assert_eq!(pkg.relationship_target(Some(&doc), &id_b), Some(b));
    }
}
