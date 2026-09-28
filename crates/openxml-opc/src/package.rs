//! In-memory model of an OPC package backed by a ZIP container.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use crate::content_types::{CONTENT_TYPES_ITEM, ContentTypes};
use crate::core_properties::CoreProperties;
use crate::error::{Error, Result};
use crate::known::{content_types as ct, rel_types};
use crate::part_name::PartName;
use crate::relationships::{Relationship, Relationships, TargetMode};

/// Content type used for parts whose type cannot be determined.
pub const FALLBACK_CONTENT_TYPE: &str = "application/octet-stream";

/// A part: content type, bytes and outgoing relationships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    content_type: String,
    data: Vec<u8>,
    relationships: Relationships,
}

impl Part {
    /// Creates a part.
    pub fn new(content_type: impl Into<String>, data: Vec<u8>) -> Self {
        Part {
            content_type: content_type.into(),
            data,
            relationships: Relationships::new(),
        }
    }

    /// Content type of the part.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// Changes the content type.
    pub fn set_content_type(&mut self, content_type: impl Into<String>) {
        self.content_type = content_type.into();
    }

    /// The part's bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replaces the part's bytes.
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    /// Relationships whose source is this part.
    pub fn relationships(&self) -> &Relationships {
        &self.relationships
    }

    /// Mutable relationships whose source is this part.
    pub fn relationships_mut(&mut self) -> &mut Relationships {
        &mut self.relationships
    }
}

/// An Open Packaging Conventions package (ECMA-376 Part 2).
///
/// Parts are kept as bytes; higher layers parse the XML parts they understand
/// and leave the rest untouched, so saving a package preserves unknown parts.
///
/// Two packages are equal when they have the same parts (names, content
/// types, bytes, relationships) and package relationships; how content types
/// are spelled in `[Content_Types].xml` (defaults vs. overrides) is ignored.
#[derive(Debug, Clone)]
pub struct Package {
    parts: BTreeMap<PartName, Part>,
    relationships: Relationships,
    content_types: ContentTypes,
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts && self.relationships == other.relationships
    }
}

impl Eq for Package {}

impl Default for Package {
    fn default() -> Self {
        Package {
            parts: BTreeMap::new(),
            relationships: Relationships::new(),
            content_types: ContentTypes::new(),
        }
    }
}

impl Package {
    /// Creates an empty package.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads a package from a seekable reader.
    pub fn open<R: Read + Seek>(reader: R) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(reader)?;
        let mut content_types = None;
        let mut items: Vec<(String, Vec<u8>)> = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().to_owned();
            let mut data = Vec::with_capacity(file.size().min(1 << 26) as usize);
            file.read_to_end(&mut data)?;
            if name.eq_ignore_ascii_case(CONTENT_TYPES_ITEM) {
                content_types = Some(ContentTypes::parse(&data)?);
            } else if !name.starts_with("[trash]") {
                items.push((name, data));
            }
        }
        let content_types = content_types.ok_or(Error::MissingContentTypes)?;
        let mut pkg = Package {
            parts: BTreeMap::new(),
            relationships: Relationships::new(),
            content_types,
        };
        let mut rels_parts: Vec<(PartName, Vec<u8>)> = Vec::new();
        for (name, data) in items {
            let Ok(part_name) = PartName::from_zip_name(&name) else {
                // Items that cannot be named as parts (e.g. interleaved pieces) are skipped.
                continue;
            };
            if part_name.is_rels_part() {
                rels_parts.push((part_name, data));
                continue;
            }
            let ty = pkg
                .content_types
                .content_type(&part_name)
                .unwrap_or(FALLBACK_CONTENT_TYPE)
                .to_owned();
            pkg.parts.insert(part_name, Part::new(ty, data));
        }
        for (rels_name, data) in rels_parts {
            match rels_name.rels_source() {
                Some(None) => pkg.relationships = Relationships::parse(&data)?,
                Some(Some(source)) if pkg.parts.contains_key(&source) => {
                    let rels = Relationships::parse(&data)?;
                    pkg.parts.get_mut(&source).expect("checked").relationships = rels;
                }
                _ => {
                    // Orphaned relationships part: keep it as an ordinary part.
                    pkg.parts.insert(rels_name, Part::new(ct::RELATIONSHIPS, data));
                }
            }
        }
        Ok(pkg)
    }

    /// Reads a package from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Package::open(Cursor::new(bytes))
    }

    /// Reads a package from a file.
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        Package::open(std::io::BufReader::new(file))
    }

    /// Builds the content types table for the current parts.
    pub fn build_content_types(&self) -> ContentTypes {
        let mut table = ContentTypes::empty();
        for (ext, ty) in self.content_types.defaults() {
            table.set_default(ext, ty);
        }
        if table.default_for("rels").is_none() {
            table.set_default("rels", ct::RELATIONSHIPS);
        }
        if table.default_for("xml").is_none() {
            table.set_default("xml", ct::XML);
        }
        // Extensions without a default get one when all their parts agree.
        let mut by_ext: HashMap<String, Option<&str>> = HashMap::new();
        for (name, part) in &self.parts {
            if let Some(ext) = name.extension()
                && table.default_for(ext).is_none()
            {
                let entry = by_ext
                    .entry(ext.to_ascii_lowercase())
                    .or_insert(Some(part.content_type()));
                if *entry != Some(part.content_type()) {
                    *entry = None;
                }
            }
        }
        let mut new_defaults: Vec<_> = by_ext.into_iter().filter_map(|(e, t)| Some((e, t?))).collect();
        new_defaults.sort();
        for (ext, ty) in new_defaults {
            table.set_default(&ext, ty);
        }
        for (name, part) in &self.parts {
            let by_default = name.extension().and_then(|e| table.default_for(e));
            if by_default != Some(part.content_type()) {
                table.set_override(name.clone(), part.content_type());
            }
        }
        table
    }

    /// Writes the package as a ZIP container.
    pub fn save<W: Write + Seek>(&self, writer: W) -> Result<W> {
        let mut zip = zip::ZipWriter::new(writer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        zip.start_file(CONTENT_TYPES_ITEM, options)?;
        zip.write_all(self.build_content_types().to_xml().as_bytes())?;
        if !self.relationships.is_empty() {
            zip.start_file("_rels/.rels", options)?;
            zip.write_all(self.relationships.to_xml().as_bytes())?;
        }
        for (name, part) in &self.parts {
            zip.start_file(name.zip_name(), options)?;
            zip.write_all(&part.data)?;
            if !part.relationships.is_empty() {
                zip.start_file(name.rels_part_name().zip_name(), options)?;
                zip.write_all(part.relationships.to_xml().as_bytes())?;
            }
        }
        Ok(zip.finish()?)
    }

    /// Serializes the package to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.save(Cursor::new(Vec::new()))?.into_inner())
    }

    /// Writes the package to a file.
    pub fn save_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = std::fs::File::create(path)?;
        let mut w = self.save(std::io::BufWriter::new(file))?;
        w.flush()?;
        Ok(())
    }

    // ----- parts -------------------------------------------------------------

    /// Iterates over all parts (relationships parts are not listed).
    pub fn parts(&self) -> impl Iterator<Item = (&PartName, &Part)> {
        self.parts.iter()
    }

    /// Number of parts.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Looks up a part.
    pub fn part(&self, name: &PartName) -> Option<&Part> {
        self.parts.get(name)
    }

    /// Looks up a part mutably.
    pub fn part_mut(&mut self, name: &PartName) -> Option<&mut Part> {
        self.parts.get_mut(name)
    }

    /// Whether a part exists.
    pub fn contains(&self, name: &PartName) -> bool {
        self.parts.contains_key(name)
    }

    /// Adds a new part; fails if the name is taken.
    pub fn add_part(&mut self, name: PartName, content_type: &str, data: Vec<u8>) -> Result<()> {
        if name.is_rels_part() {
            return Err(Error::InvalidPartName {
                name: name.to_string(),
                reason: "relationships parts are managed by the package",
            });
        }
        if self.parts.contains_key(&name) {
            return Err(Error::DuplicatePart(name.to_string()));
        }
        self.parts.insert(name, Part::new(content_type, data));
        Ok(())
    }

    /// Adds a part or replaces the data and content type of an existing one
    /// (its relationships are kept).
    pub fn set_part(&mut self, name: PartName, content_type: &str, data: Vec<u8>) -> Result<()> {
        match self.parts.get_mut(&name) {
            Some(part) => {
                part.content_type = content_type.to_owned();
                part.data = data;
                Ok(())
            }
            None => self.add_part(name, content_type, data),
        }
    }

    /// Replaces the bytes of an existing part.
    pub fn set_part_data(&mut self, name: &PartName, data: Vec<u8>) -> Result<()> {
        let part = self
            .parts
            .get_mut(name)
            .ok_or_else(|| Error::MissingPart(name.to_string()))?;
        part.data = data;
        Ok(())
    }

    /// Removes a part together with every internal relationship that targets it.
    pub fn remove_part(&mut self, name: &PartName) -> Option<Part> {
        let removed = self.parts.remove(name)?;
        let targets = |source: Option<&PartName>, r: &Relationship| {
            !r.is_external() && PartName::resolve(source, &r.target).is_ok_and(|t| &t == name)
        };
        self.relationships.retain(|r| !targets(None, r));
        let names: Vec<PartName> = self.parts.keys().cloned().collect();
        for source in names {
            let part = self.parts.get_mut(&source).expect("listed");
            part.relationships.retain(|r| !targets(Some(&source), r));
        }
        Some(removed)
    }

    /// Returns a part name built from `pattern` (containing `{}`) with the
    /// smallest positive number that is not in use, e.g. `/word/media/image{}.png`.
    pub fn next_part_name(&self, pattern: &str) -> Result<PartName> {
        (1u32..)
            .map(|n| PartName::new(pattern.replacen("{}", &n.to_string(), 1)))
            .find(|p| p.as_ref().map_or(true, |p| !self.parts.contains_key(p)))
            .expect("unbounded search")
    }

    // ----- relationships -----------------------------------------------------

    /// Package-level relationships.
    pub fn package_relationships(&self) -> &Relationships {
        &self.relationships
    }

    /// Mutable package-level relationships.
    pub fn package_relationships_mut(&mut self) -> &mut Relationships {
        &mut self.relationships
    }

    /// Relationships of a source (`None` = package). `None` if the part is missing.
    pub fn relationships(&self, source: Option<&PartName>) -> Option<&Relationships> {
        match source {
            None => Some(&self.relationships),
            Some(p) => self.parts.get(p).map(|p| &p.relationships),
        }
    }

    /// Mutable relationships of a source (`None` = package).
    pub fn relationships_mut(&mut self, source: Option<&PartName>) -> Option<&mut Relationships> {
        match source {
            None => Some(&mut self.relationships),
            Some(p) => self.parts.get_mut(p).map(|p| &mut p.relationships),
        }
    }

    /// Adds an internal relationship from `source` to `target` and returns its id.
    pub fn add_relationship(
        &mut self,
        source: Option<&PartName>,
        rel_type: &str,
        target: &PartName,
    ) -> Result<String> {
        let reference = target.relative_to(source);
        let rels = self
            .relationships_mut(source)
            .ok_or_else(|| Error::MissingPart(source.map(|s| s.to_string()).unwrap_or_default()))?;
        Ok(rels.add(rel_type, &reference, TargetMode::Internal))
    }

    /// Adds an external relationship (e.g. a hyperlink) and returns its id.
    pub fn add_external_relationship(
        &mut self,
        source: Option<&PartName>,
        rel_type: &str,
        uri: &str,
    ) -> Result<String> {
        let rels = self
            .relationships_mut(source)
            .ok_or_else(|| Error::MissingPart(source.map(|s| s.to_string()).unwrap_or_default()))?;
        Ok(rels.add(rel_type, uri, TargetMode::External))
    }

    /// Resolves the internal target of relationship `id` of `source`.
    pub fn relationship_target(&self, source: Option<&PartName>, id: &str) -> Option<PartName> {
        let rel = self.relationships(source)?.get(id)?;
        if rel.is_external() {
            return None;
        }
        PartName::resolve(source, &rel.target).ok()
    }

    /// Internal targets of all relationships of `rel_type` from `source`.
    pub fn related_parts(&self, source: Option<&PartName>, rel_type: &str) -> Vec<PartName> {
        self.relationships(source)
            .map(|rels| {
                rels.by_type(rel_type)
                    .filter(|r| !r.is_external())
                    .filter_map(|r| PartName::resolve(source, &r.target).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Internal target of the first relationship of `rel_type` from `source`.
    pub fn related_part(&self, source: Option<&PartName>, rel_type: &str) -> Option<PartName> {
        self.related_parts(source, rel_type).into_iter().next()
    }

    /// The main document part (target of the package `officeDocument` relationship).
    pub fn main_part(&self) -> Option<PartName> {
        self.related_part(None, rel_types::OFFICE_DOCUMENT)
    }

    // ----- core properties ---------------------------------------------------

    fn core_properties_part(&self) -> Option<PartName> {
        self.related_part(None, rel_types::CORE_PROPERTIES)
            .or_else(|| self.related_part(None, rel_types::CORE_PROPERTIES_LEGACY))
    }

    /// Reads the core properties (empty if the package has none).
    pub fn core_properties(&self) -> Result<CoreProperties> {
        match self.core_properties_part().and_then(|p| self.parts.get(&p)) {
            Some(part) => CoreProperties::parse(part.data()),
            None => Ok(CoreProperties::default()),
        }
    }

    /// Writes the core properties, creating `/docProps/core.xml` if needed.
    pub fn set_core_properties(&mut self, props: &CoreProperties) -> Result<()> {
        let data = props.to_xml().into_bytes();
        match self.core_properties_part() {
            Some(name) if self.parts.contains_key(&name) => self.set_part_data(&name, data),
            _ => {
                let name = PartName::new("/docProps/core.xml")?;
                self.set_part(name.clone(), ct::CORE_PROPERTIES, data)?;
                self.relationships
                    .retain(|r| r.rel_type != rel_types::CORE_PROPERTIES);
                self.add_relationship(None, rel_types::CORE_PROPERTIES, &name)?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pn(s: &str) -> PartName {
        PartName::new(s).unwrap()
    }

    fn sample() -> Package {
        let mut pkg = Package::new();
        let doc = pn("/word/document.xml");
        pkg.add_part(doc.clone(), ct::WML_DOCUMENT, b"<w:document/>".to_vec())
            .unwrap();
        pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &doc)
            .unwrap();
        let img = pn("/word/media/image1.png");
        pkg.add_part(img.clone(), ct::PNG, vec![0x89, b'P', b'N', b'G'])
            .unwrap();
        pkg.add_relationship(Some(&doc), rel_types::IMAGE, &img).unwrap();
        pkg.add_external_relationship(Some(&doc), rel_types::HYPERLINK, "https://example.com")
            .unwrap();
        pkg
    }

    #[test]
    fn save_and_reopen_preserves_everything() {
        let pkg = sample();
        let bytes = pkg.to_bytes().unwrap();
        let back = Package::from_bytes(&bytes).unwrap();
        assert_eq!(back.part_count(), 2);
        let doc = back.main_part().unwrap();
        assert_eq!(doc, pn("/word/document.xml"));
        assert_eq!(back.part(&doc).unwrap().content_type(), ct::WML_DOCUMENT);
        assert_eq!(back.part(&doc).unwrap().data(), b"<w:document/>");
        let images = back.related_parts(Some(&doc), rel_types::IMAGE);
        assert_eq!(images, vec![pn("/word/media/image1.png")]);
        assert_eq!(back.part(&images[0]).unwrap().content_type(), "image/png");
        let links: Vec<_> = back
            .relationships(Some(&doc))
            .unwrap()
            .by_type(rel_types::HYPERLINK)
            .collect();
        assert_eq!(links[0].target, "https://example.com");
        assert!(links[0].is_external());
        // Saving is deterministic.
        assert_eq!(back.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn content_types_use_defaults_and_overrides() {
        let pkg = sample();
        let table = pkg.build_content_types();
        assert_eq!(table.default_for("png"), Some("image/png"));
        assert_eq!(table.default_for("rels"), Some(ct::RELATIONSHIPS));
        let overrides: Vec<_> = table
            .overrides()
            .map(|(n, t)| (n.as_str().to_owned(), t.to_owned()))
            .collect();
        assert_eq!(
            overrides,
            vec![("/word/document.xml".to_owned(), ct::WML_DOCUMENT.to_owned())]
        );
    }

    #[test]
    fn conflicting_extension_types_use_overrides() {
        let mut pkg = Package::new();
        pkg.add_part(pn("/a/x.bin"), "type/one", vec![]).unwrap();
        pkg.add_part(pn("/a/y.bin"), "type/two", vec![]).unwrap();
        let table = pkg.build_content_types();
        assert_eq!(table.default_for("bin"), None);
        assert_eq!(table.overrides().count(), 2);
        let back = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
        assert_eq!(back.part(&pn("/a/x.bin")).unwrap().content_type(), "type/one");
        assert_eq!(back.part(&pn("/a/y.bin")).unwrap().content_type(), "type/two");
    }

    #[test]
    fn part_management() {
        let mut pkg = sample();
        let doc = pn("/word/document.xml");
        assert!(matches!(
            pkg.add_part(doc.clone(), "x", vec![]),
            Err(Error::DuplicatePart(_))
        ));
        assert!(
            pkg.add_part(pn("/word/_rels/document.xml.rels"), "x", vec![])
                .is_err()
        );
        pkg.set_part(doc.clone(), ct::WML_TEMPLATE, b"new".to_vec())
            .unwrap();
        assert_eq!(pkg.part(&doc).unwrap().content_type(), ct::WML_TEMPLATE);
        assert_eq!(
            pkg.part(&doc).unwrap().relationships().len(),
            2,
            "relationships survive set_part"
        );
        pkg.set_part_data(&doc, b"again".to_vec()).unwrap();
        assert_eq!(pkg.part(&doc).unwrap().data(), b"again");
        assert!(pkg.set_part_data(&pn("/missing.xml"), vec![]).is_err());
        assert!(pkg.contains(&doc));
        pkg.part_mut(&doc).unwrap().set_content_type(ct::WML_DOCUMENT);
        assert_eq!(
            pkg.next_part_name("/word/media/image{}.png").unwrap(),
            pn("/word/media/image2.png")
        );
        assert_eq!(
            pkg.next_part_name("/word/header{}.xml").unwrap(),
            pn("/word/header1.xml")
        );
        assert_eq!(pkg.parts().count(), 2);
    }

    #[test]
    fn removing_a_part_removes_incoming_relationships() {
        let mut pkg = sample();
        let doc = pn("/word/document.xml");
        let img = pn("/word/media/image1.png");
        assert!(pkg.remove_part(&img).is_some());
        assert!(pkg.remove_part(&img).is_none());
        let rels = pkg.relationships(Some(&doc)).unwrap();
        assert_eq!(rels.len(), 1, "only the external hyperlink remains");
        pkg.remove_part(&doc).unwrap();
        assert!(pkg.package_relationships().is_empty());
        assert!(pkg.main_part().is_none());
    }

    #[test]
    fn relationship_lookup() {
        let mut pkg = sample();
        let doc = pn("/word/document.xml");
        let rels = pkg.relationships(Some(&doc)).unwrap();
        let img_id = rels.first_by_type(rel_types::IMAGE).unwrap().id.clone();
        let link_id = rels.first_by_type(rel_types::HYPERLINK).unwrap().id.clone();
        assert_eq!(
            pkg.relationship_target(Some(&doc), &img_id),
            Some(pn("/word/media/image1.png"))
        );
        assert_eq!(
            pkg.relationship_target(Some(&doc), &link_id),
            None,
            "external targets are not parts"
        );
        assert_eq!(pkg.relationship_target(Some(&doc), "nope"), None);
        assert!(pkg.relationships(Some(&pn("/none.xml"))).is_none());
        assert!(pkg.add_relationship(Some(&pn("/none.xml")), "t", &doc).is_err());
        assert!(
            pkg.add_external_relationship(Some(&pn("/none.xml")), "t", "x")
                .is_err()
        );
        pkg.package_relationships_mut()
            .add("t", "x", TargetMode::External);
        assert_eq!(pkg.package_relationships().len(), 2);
        assert!(pkg.relationships_mut(None).is_some());
    }

    #[test]
    fn core_properties_are_created_and_updated() {
        let mut pkg = Package::new();
        assert_eq!(pkg.core_properties().unwrap(), CoreProperties::default());
        let props = CoreProperties {
            title: Some("Hello".into()),
            ..Default::default()
        };
        pkg.set_core_properties(&props).unwrap();
        assert_eq!(pkg.core_properties().unwrap(), props);
        let props2 = CoreProperties {
            creator: Some("Me".into()),
            ..props
        };
        pkg.set_core_properties(&props2).unwrap();
        let back = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
        assert_eq!(back.core_properties().unwrap(), props2);
        assert_eq!(back.package_relationships().len(), 1);
        assert_eq!(
            back.part(&pn("/docProps/core.xml")).unwrap().content_type(),
            ct::CORE_PROPERTIES
        );
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, opts).unwrap();
            } else {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap().into_inner()
    }

    const TYPES: &[u8] = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#;

    #[test]
    fn opening_tolerates_unusual_content() {
        let bytes = zip_of(&[
            ("[content_types].xml", TYPES),
            ("dir/", b""),
            ("a.xml", b"<a/>"),
            ("data.bin", b"\0"),
            ("[trash]/0000.dat", b"junk"),
            (
                "orphan/_rels/missing.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
            ),
        ]);
        let pkg = Package::open(Cursor::new(bytes)).unwrap();
        assert_eq!(pkg.part(&pn("/a.xml")).unwrap().content_type(), ct::XML);
        assert_eq!(
            pkg.part(&pn("/data.bin")).unwrap().content_type(),
            FALLBACK_CONTENT_TYPE
        );
        assert!(
            pkg.part(&pn("/orphan/_rels/missing.xml.rels")).is_some(),
            "orphaned rels are kept"
        );
        assert_eq!(pkg.part_count(), 3);
    }

    #[test]
    fn opening_requires_content_types() {
        let bytes = zip_of(&[("a.xml", b"<a/>")]);
        assert!(matches!(
            Package::from_bytes(&bytes),
            Err(Error::MissingContentTypes)
        ));
        assert!(matches!(
            Package::from_bytes(b"not a zip"),
            Err(Error::Zip(_) | Error::Io(_))
        ));
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("openxml-opc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pkg.zip");
        let pkg = sample();
        pkg.save_path(&path).unwrap();
        let back = Package::open_path(&path).unwrap();
        assert_eq!(back, Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Package::open_path(dir.join("missing.zip")).is_err());
    }
}
