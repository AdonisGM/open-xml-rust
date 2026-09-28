//! Schema-less XML nodes.
//!
//! Content that the typed object model does not describe (extension
//! elements, markup-compatibility wrappers, wildcard `xsd:any` content,
//! unknown attributes) is captured as raw nodes so that documents survive a
//! read/write round trip without losing data.

use crate::element::{XmlRead, XmlWrite};
use crate::error::{Error, Result};
use crate::ns::Ns;
use crate::reader::{Event, StartTag, XmlReader, decode_xml_bytes};
use crate::writer::XmlWriter;

/// A qualified XML name that remembers how it was written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawName {
    /// Namespace identifier.
    pub ns: Ns,
    /// Namespace URI when `ns` is [`Ns::OTHER`].
    pub uri: Option<Box<str>>,
    /// Prefix used in the source document, reused when writing if possible.
    pub prefix: Option<Box<str>>,
    /// Local name.
    pub local: Box<str>,
}

impl RawName {
    /// Creates a name in a known namespace (or [`Ns::NONE`]).
    pub fn new(ns: Ns, local: &str) -> Self {
        RawName {
            ns,
            uri: None,
            prefix: None,
            local: local.into(),
        }
    }

    /// Creates a name in an arbitrary namespace URI.
    pub fn with_uri(uri: &str, prefix: Option<&str>, local: &str) -> Self {
        let ns = Ns::classify(uri);
        RawName {
            ns,
            uri: (ns == Ns::OTHER).then(|| uri.into()),
            prefix: prefix.map(Into::into),
            local: local.into(),
        }
    }

    /// The namespace URI (canonical for known namespaces, empty for none).
    pub fn uri(&self) -> &str {
        match &self.uri {
            Some(u) => u,
            None => self.ns.uri(),
        }
    }

    /// Whether this name has the given namespace and local name.
    pub fn is(&self, ns: Ns, local: &str) -> bool {
        self.ns == ns && &*self.local == local
    }
}

/// An attribute captured without schema knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawAttribute {
    /// Attribute name. Namespace declarations use [`Ns::XMLNS`] with the declared
    /// prefix as local name (empty for the default namespace).
    pub name: RawName,
    /// Attribute value (unescaped).
    pub value: String,
}

impl RawAttribute {
    /// Creates an attribute in a known namespace (or [`Ns::NONE`]).
    pub fn new(ns: Ns, local: &str, value: impl Into<String>) -> Self {
        RawAttribute {
            name: RawName::new(ns, local),
            value: value.into(),
        }
    }

    /// Creates a namespace declaration (`xmlns:prefix="uri"`, or `xmlns="uri"` for an empty prefix).
    pub fn namespace_declaration(prefix: &str, uri: &str) -> Self {
        RawAttribute {
            name: RawName {
                ns: Ns::XMLNS,
                uri: None,
                prefix: Some("xmlns".into()),
                local: prefix.into(),
            },
            value: uri.into(),
        }
    }

    /// Whether this attribute is a namespace declaration.
    pub fn is_namespace_declaration(&self) -> bool {
        self.name.ns == Ns::XMLNS
    }
}

/// A node inside a [`RawElement`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RawNode {
    /// A child element.
    Element(RawElement),
    /// Character data.
    Text(String),
}

/// An element captured without schema knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawElement {
    /// Element name.
    pub name: RawName,
    /// Attributes, including namespace declarations.
    pub attributes: Vec<RawAttribute>,
    /// Child nodes in document order.
    pub children: Vec<RawNode>,
}

impl RawElement {
    /// Creates an empty element.
    pub fn new(ns: Ns, local: &str) -> Self {
        RawElement {
            name: RawName::new(ns, local),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Reads the element opened by `tag`, including all of its content.
    pub fn read(r: &mut XmlReader<'_>, tag: &StartTag<'_>) -> Result<Self> {
        let mut el = RawElement {
            name: tag.raw_name(),
            attributes: tag.attributes().map(|a| a.to_raw()).collect(),
            children: Vec::new(),
        };
        if !tag.is_empty() {
            loop {
                match r.next_event()? {
                    Event::Start(child) => {
                        el.children.push(RawNode::Element(RawElement::read(r, &child)?));
                    }
                    Event::Text(t) => el.children.push(RawNode::Text(t.into_owned())),
                    Event::End => break,
                    Event::Eof => return Err(Error::UnexpectedEof),
                }
            }
        }
        Ok(el)
    }

    /// Parses a standalone XML document into a raw element tree.
    pub fn parse(xml: &str) -> Result<Self> {
        let mut r = XmlReader::new(xml);
        let root = r.root()?;
        RawElement::read(&mut r, &root)
    }

    /// Parses XML bytes (UTF-8 or UTF-16) into a raw element tree.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self> {
        RawElement::parse(&decode_xml_bytes(bytes)?)
    }

    /// Writes the element (and its subtree).
    pub fn write(&self, w: &mut XmlWriter) {
        w.start_raw(&self.name, &self.attributes);
        w.attrs_raw(&self.attributes);
        for child in &self.children {
            match child {
                RawNode::Element(e) => e.write(w),
                RawNode::Text(t) => w.text(t),
            }
        }
        w.end();
    }

    /// Serializes the element as a standalone XML fragment (no declaration).
    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::new();
        self.write(&mut w);
        w.finish()
    }

    /// Converts a typed value into a raw element named `ns:local`.
    pub fn from_typed<T: XmlWrite + ?Sized>(value: &T, ns: Ns, local: &str) -> Self {
        let mut w = XmlWriter::new();
        value.write_xml(&mut w, ns, local);
        RawElement::parse(&w.finish()).expect("the writer produces well-formed XML")
    }

    /// Interprets this element as a typed value.
    pub fn to_typed<T: XmlRead>(&self) -> Result<T> {
        let xml = self.to_xml();
        let mut r = XmlReader::new(&xml);
        let root = r.root()?;
        T::read_xml(&mut r, &root)
    }

    /// Value of the attribute with the given name.
    pub fn attr(&self, ns: Ns, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.is(ns, local))
            .map(|a| a.value.as_str())
    }

    /// Sets (or replaces) an attribute value.
    pub fn set_attr(&mut self, ns: Ns, local: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attributes.iter_mut().find(|a| a.name.is(ns, local)) {
            Some(a) => a.value = value,
            None => self.attributes.push(RawAttribute::new(ns, local, value)),
        }
    }

    /// Iterates over child elements.
    pub fn elements(&self) -> impl Iterator<Item = &RawElement> {
        self.children.iter().filter_map(|c| match c {
            RawNode::Element(e) => Some(e),
            RawNode::Text(_) => None,
        })
    }

    /// First child element with the given name.
    pub fn child(&self, ns: Ns, local: &str) -> Option<&RawElement> {
        self.elements().find(|e| e.name.is(ns, local))
    }

    /// Concatenated character data of this element and all descendants.
    pub fn text(&self) -> String {
        fn walk(e: &RawElement, out: &mut String) {
            for c in &e.children {
                match c {
                    RawNode::Element(e) => walk(e, out),
                    RawNode::Text(t) => out.push_str(t),
                }
            }
        }
        let mut out = String::new();
        walk(self, &mut out);
        out
    }

    /// Visits this element and all descendant elements in document order.
    pub fn descendants(&self) -> Vec<&RawElement> {
        let mut out = vec![self];
        let mut i = 0;
        while i < out.len() {
            let e = out[i];
            out.extend(e.elements());
            i += 1;
        }
        out
    }
}

/// A raw element that appeared among the children of a typed element at a
/// position the schema does not describe.
///
/// The position is recorded relative to the typed fields: the element is
/// written immediately before item `index` of field `anchor` (for fields
/// holding a single value `index` is `0`, i.e. before the field). When
/// `index` is at least the number of items of a repeated field, the element
/// is written after that field's last item. `anchor` equal to the number of
/// fields means "after all fields". This keeps markup-compatibility wrappers
/// such as `mc:AlternateContent` in their original position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtraChild {
    /// Index of the field the element is attached to.
    pub anchor: u16,
    /// Item index within a repeated field.
    pub index: u32,
    /// The captured element.
    pub element: RawElement,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialize_round_trip() {
        let xml = r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wps="urn:wps"><mc:Choice Requires="wps"><wps:shape a="1">text &amp; more</wps:shape></mc:Choice><mc:Fallback/></mc:AlternateContent>"#;
        let el = RawElement::parse(xml).unwrap();
        assert!(el.name.is(Ns::MC, "AlternateContent"));
        let choice = el.child(Ns::MC, "Choice").unwrap();
        assert_eq!(choice.attr(Ns::NONE, "Requires"), Some("wps"));
        let shape = choice.elements().next().unwrap();
        assert_eq!(shape.name.ns, Ns::OTHER);
        assert_eq!(shape.name.uri(), "urn:wps");
        assert_eq!(shape.text(), "text & more");
        let out = el.to_xml();
        assert_eq!(out, xml);
        assert_eq!(RawElement::parse(&out).unwrap(), el);
    }

    #[test]
    fn writes_declarations_for_unbound_namespaces() {
        let mut el = RawElement::new(Ns::A, "blip");
        el.set_attr(Ns::R, "embed", "rId1");
        el.children.push(RawNode::Element(RawElement {
            name: RawName::with_uri("urn:x", Some("q"), "ext"),
            attributes: vec![],
            children: vec![RawNode::Text("<>".into())],
        }));
        let xml = el.to_xml();
        assert_eq!(
            xml,
            r#"<a:blip xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="rId1"><q:ext xmlns:q="urn:x">&lt;&gt;</q:ext></a:blip>"#
        );
        let back = RawElement::parse(&xml).unwrap();
        assert_eq!(back.attr(Ns::R, "embed"), Some("rId1"));
        assert_eq!(back.child_text_for_test(), "<>");
    }

    impl RawElement {
        fn child_text_for_test(&self) -> String {
            self.elements().next().unwrap().text()
        }
    }

    #[test]
    fn set_attr_replaces_existing() {
        let mut el = RawElement::new(Ns::NONE, "a");
        el.set_attr(Ns::NONE, "x", "1");
        el.set_attr(Ns::NONE, "x", "2");
        assert_eq!(el.attributes.len(), 1);
        assert_eq!(el.attr(Ns::NONE, "x"), Some("2"));
        assert_eq!(el.to_xml(), r#"<a x="2"/>"#);
    }

    #[test]
    fn descendants_are_breadth_first_and_complete() {
        let el = RawElement::parse("<a><b><d/></b><c/></a>").unwrap();
        let names: Vec<_> = el
            .descendants()
            .iter()
            .map(|e| e.name.local.to_string())
            .collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
    }

    #[test]
    fn parse_bytes_and_errors() {
        assert!(RawElement::parse_bytes(b"\xEF\xBB\xBF<a/>").is_ok());
        assert!(RawElement::parse("<a>").is_err());
        assert!(RawElement::parse("").is_err());
    }

    #[test]
    fn namespace_declaration_attribute() {
        let d = RawAttribute::namespace_declaration("w14", "urn:w14");
        assert!(d.is_namespace_declaration());
        let mut el = RawElement::new(Ns::NONE, "root");
        el.attributes.push(d);
        el.set_attr(Ns::MC, "Ignorable", "w14");
        let xml = el.to_xml();
        assert_eq!(
            xml,
            r#"<root xmlns:w14="urn:w14" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14"/>"#
        );
    }

    #[test]
    fn preserves_default_namespace_prefixing() {
        let xml = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;
        let el = RawElement::parse(xml).unwrap();
        assert_eq!(el.name.ns, Ns::X);
        assert_eq!(el.to_xml(), xml);
    }

    #[test]
    fn unqualified_child_inside_default_namespace_resets_it() {
        let mut root = RawElement::parse(r#"<a xmlns="urn:a"/>"#).unwrap();
        root.children
            .push(RawNode::Element(RawElement::new(Ns::NONE, "b")));
        let xml = root.to_xml();
        assert_eq!(xml, r#"<a xmlns="urn:a"><b xmlns=""/></a>"#);
        let back = RawElement::parse(&xml).unwrap();
        assert_eq!(back.elements().next().unwrap().name.ns, Ns::NONE);
    }
}
