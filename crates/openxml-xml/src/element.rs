//! Traits implemented by schema types and helpers used by generated code.

use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::ns::Ns;
use crate::raw::{ExtraChild, RawElement, RawNode};
use crate::reader::{StartTag, XmlReader, decode_xml_bytes};
use crate::value::XmlValue;
use crate::writer::XmlWriter;

/// Deserialization of an element's attributes and content.
///
/// A complex type does not know the name of the element it is used for (many
/// elements share one type), so the caller has already consumed the start tag
/// and passes it in. The implementation must consume the element's content up
/// to and including its end tag.
pub trait XmlRead: Sized {
    /// Reads the element opened by `tag`.
    fn read_xml(r: &mut XmlReader<'_>, tag: &StartTag<'_>) -> Result<Self>;
}

/// Serialization of a value as an element with the given name.
pub trait XmlWrite {
    /// Writes the value as element `ns:local`.
    fn write_xml(&self, w: &mut XmlWriter, ns: Ns, local: &str);
}

impl<T: XmlRead> XmlRead for Box<T> {
    fn read_xml(r: &mut XmlReader<'_>, tag: &StartTag<'_>) -> Result<Self> {
        T::read_xml(r, tag).map(Box::new)
    }
}

impl<T: XmlWrite + ?Sized> XmlWrite for Box<T> {
    fn write_xml(&self, w: &mut XmlWriter, ns: Ns, local: &str) {
        (**self).write_xml(w, ns, local)
    }
}

impl XmlRead for RawElement {
    fn read_xml(r: &mut XmlReader<'_>, tag: &StartTag<'_>) -> Result<Self> {
        RawElement::read(r, tag)
    }
}

impl XmlWrite for RawElement {
    /// Writes the raw element under the requested name (its own name is ignored).
    fn write_xml(&self, w: &mut XmlWriter, ns: Ns, local: &str) {
        let mut renamed = self.clone();
        renamed.name = crate::RawName::new(ns, local);
        renamed.write(w);
    }
}

/// Describes a global (root-capable) element of a schema: its name, content
/// type and the namespaces to declare when it is written as a document root.
pub struct ElementDef<T> {
    ns: Ns,
    local: &'static str,
    namespaces: &'static [Ns],
    _type: PhantomData<fn() -> T>,
}

impl<T> Clone for ElementDef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ElementDef<T> {}

impl<T> std::fmt::Debug for ElementDef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ElementDef({}:{})", self.ns.prefix(), self.local)
    }
}

impl<T> ElementDef<T> {
    /// Creates a definition (used by generated code).
    pub const fn new(ns: Ns, local: &'static str, namespaces: &'static [Ns]) -> Self {
        ElementDef {
            ns,
            local,
            namespaces,
            _type: PhantomData,
        }
    }

    /// Namespace of the element.
    pub fn ns(&self) -> Ns {
        self.ns
    }

    /// Local name of the element.
    pub fn local(&self) -> &'static str {
        self.local
    }

    /// Namespaces declared on the root when a document is written.
    pub fn namespaces(&self) -> &'static [Ns] {
        self.namespaces
    }
}

impl<T: XmlRead> ElementDef<T> {
    /// Parses a document whose root must be this element.
    pub fn parse(&self, xml: &str) -> Result<T> {
        let mut r = XmlReader::new(xml);
        let root = r.root()?;
        if root.ns() != self.ns || root.local() != self.local {
            return Err(Error::UnexpectedRoot {
                expected: format!("{{{}}}{}", self.ns.uri(), self.local),
                found: root.clark_name(),
            });
        }
        T::read_xml(&mut r, &root)
    }

    /// Parses document bytes (UTF-8 or UTF-16) whose root must be this element.
    pub fn parse_bytes(&self, bytes: &[u8]) -> Result<T> {
        self.parse(&decode_xml_bytes(bytes)?)
    }
}

impl<T: XmlWrite> ElementDef<T> {
    /// Serializes a value as a complete XML document with this element as root.
    pub fn to_xml(&self, value: &T) -> String {
        let mut w = XmlWriter::with_declaration();
        w.predeclare(self.namespaces);
        value.write_xml(&mut w, self.ns, self.local);
        w.finish()
    }

    /// Serializes a value as UTF-8 document bytes.
    pub fn to_bytes(&self, value: &T) -> Vec<u8> {
        self.to_xml(value).into_bytes()
    }
}

/// Parses a document into `T` without checking the root element's name.
pub fn read_document<T: XmlRead>(xml: &str) -> Result<T> {
    let mut r = XmlReader::new(xml);
    let root = r.root()?;
    T::read_xml(&mut r, &root)
}

/// Runtime support for generated code. Not part of the stable API.
#[doc(hidden)]
pub mod rt {
    use super::*;
    use crate::raw::RawAttribute;

    /// Reads an element whose content is a simple type.
    ///
    /// If the text is not a valid value, or the element carries attributes the
    /// schema does not allow, the element is returned as raw XML instead so
    /// that it can be preserved.
    pub fn read_simple<T: XmlValue>(
        r: &mut XmlReader<'_>,
        tag: &StartTag<'_>,
    ) -> Result<Result<T, RawElement>> {
        let attrs: Vec<RawAttribute> = tag.attributes().map(|a| a.to_raw()).collect();
        if tag.is_empty() {
            if attrs.is_empty()
                && let Some(v) = T::parse_xml("")
            {
                return Ok(Ok(v));
            }
            return Ok(Err(RawElement {
                name: tag.raw_name(),
                attributes: attrs,
                children: Vec::new(),
            }));
        }
        if !attrs.is_empty() {
            return RawElement::read(r, tag).map(Err);
        }
        let text = r.read_text(tag)?;
        match T::parse_xml(&text) {
            Some(v) => Ok(Ok(v)),
            None => Ok(Err(RawElement {
                name: tag.raw_name(),
                attributes: attrs,
                children: vec![RawNode::Text(text.into_owned())],
            })),
        }
    }

    /// Writes an element whose content is a simple type.
    pub fn write_simple<T: XmlValue>(w: &mut XmlWriter, ns: Ns, local: &str, value: &T) {
        w.start(ns, local);
        w.text_value(value);
        w.end();
    }

    /// Writes an optional attribute.
    #[inline]
    pub fn write_attr<T: XmlValue>(w: &mut XmlWriter, ns: Ns, local: &str, value: &Option<T>) {
        if let Some(v) = value {
            w.attr_value(ns, local, v);
        }
    }

    /// Writes the extra children anchored before field `anchor`.
    #[inline]
    pub fn write_extras(w: &mut XmlWriter, extras: &[ExtraChild], anchor: u16) {
        if extras.is_empty() {
            return;
        }
        for e in extras.iter().filter(|e| e.anchor == anchor) {
            e.element.write(w);
        }
    }

    /// Stores an unknown child element.
    #[inline]
    pub fn push_extra(extras: &mut Vec<ExtraChild>, anchor: u16, element: RawElement) {
        extras.push(ExtraChild { anchor, element });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::RawAttribute;

    /// A hand-written type in the shape the generator produces.
    #[derive(Debug, Default, PartialEq)]
    struct Item {
        id: Option<u32>,
        name: Option<String>,
        extra_attrs: Vec<RawAttribute>,
        extra_children: Vec<ExtraChild>,
    }

    impl XmlRead for Item {
        fn read_xml(r: &mut XmlReader<'_>, tag: &StartTag<'_>) -> Result<Self> {
            let mut this = Item::default();
            for a in tag.attributes() {
                if let (Ns::NONE, "id") = (a.ns, a.local)
                    && let Some(v) = XmlValue::parse_xml(&a.value)
                {
                    this.id = Some(v);
                    continue;
                }
                this.extra_attrs.push(a.to_raw());
            }
            if !tag.is_empty() {
                let mut cur = None::<u16>;
                while let Some(child) = r.next_child()? {
                    match (child.ns(), child.local()) {
                        (Ns::NONE, "name") if this.name.is_none() => {
                            match rt::read_simple::<String>(r, &child)? {
                                Ok(v) => this.name = Some(v),
                                Err(raw) => rt::push_extra(&mut this.extra_children, 0, raw),
                            }
                            cur = Some(0);
                        }
                        _ => {
                            let raw = RawElement::read(r, &child)?;
                            rt::push_extra(&mut this.extra_children, cur.map_or(0, |c| c + 1), raw);
                        }
                    }
                }
            }
            Ok(this)
        }
    }

    impl XmlWrite for Item {
        fn write_xml(&self, w: &mut XmlWriter, ns: Ns, local: &str) {
            w.start_with(ns, local, &self.extra_attrs);
            rt::write_attr(w, Ns::NONE, "id", &self.id);
            w.attrs_raw(&self.extra_attrs);
            rt::write_extras(w, &self.extra_children, 0);
            if let Some(v) = &self.name {
                rt::write_simple(w, Ns::NONE, "name", v);
            }
            rt::write_extras(w, &self.extra_children, 1);
            w.end();
        }
    }

    const ITEM: ElementDef<Item> = ElementDef::new(Ns::NONE, "item", &[]);

    #[test]
    fn typed_round_trip_preserves_unknown_content_positions() {
        let xml = r#"<item id="7" x="y"><pre/><name>n</name><post>t</post></item>"#;
        let item = ITEM.parse(xml).unwrap();
        assert_eq!(item.id, Some(7));
        assert_eq!(item.name.as_deref(), Some("n"));
        assert_eq!(item.extra_attrs.len(), 1);
        assert_eq!(
            item.extra_children.iter().map(|e| e.anchor).collect::<Vec<_>>(),
            [0, 1]
        );
        let out = ITEM.to_xml(&item);
        assert_eq!(out, format!("{}{}", crate::XML_DECLARATION, xml));
    }

    #[test]
    fn invalid_values_are_preserved_as_raw() {
        let xml = r#"<item id="seven"><name a="1">n</name></item>"#;
        let item = ITEM.parse(xml).unwrap();
        assert_eq!(item.id, None);
        assert_eq!(item.extra_attrs[0].value, "seven");
        assert_eq!(
            item.name, None,
            "an attribute on a simple element forces raw capture"
        );
        assert_eq!(item.extra_children.len(), 1);
        let out = ITEM.to_xml(&item);
        assert!(out.ends_with(xml), "{out}");
    }

    #[test]
    fn root_name_is_checked() {
        let err = ITEM.parse("<other/>").unwrap_err();
        assert!(matches!(err, Error::UnexpectedRoot { .. }));
        assert!(err.to_string().contains("{}item"));
        assert!(read_document::<Item>("<other id='1'/>").unwrap().id == Some(1));
        assert_eq!(ITEM.parse_bytes(b"\xEF\xBB\xBF<item/>").unwrap(), Item::default());
        assert_eq!(
            ITEM.to_bytes(&Item::default()),
            format!("{}<item/>", crate::XML_DECLARATION).into_bytes()
        );
        assert_eq!(ITEM.local(), "item");
        assert_eq!(ITEM.ns(), Ns::NONE);
        assert!(ITEM.namespaces().is_empty());
        assert_eq!(format!("{ITEM:?}"), "ElementDef(:item)");
    }

    #[test]
    fn simple_elements() {
        let mut r = XmlReader::new("<r><n>12</n><n>x</n><n/><n></n></r>");
        let root = r.root().unwrap();
        let mut results = Vec::new();
        while let Some(c) = r.next_child().unwrap() {
            results.push(rt::read_simple::<u32>(&mut r, &c).unwrap().ok());
        }
        assert_eq!(results, [Some(12), None, None, None]);
        let _ = root;
        let mut r = XmlReader::new("<r><s/></r>");
        r.root().unwrap();
        let s = r.next_child().unwrap().unwrap();
        assert_eq!(rt::read_simple::<String>(&mut r, &s).unwrap(), Ok(String::new()));
    }

    #[test]
    fn boxed_and_raw_impls() {
        let mut r = XmlReader::new(r#"<item id="3"/>"#);
        let root = r.root().unwrap();
        let b: Box<Item> = XmlRead::read_xml(&mut r, &root).unwrap();
        assert_eq!(b.id, Some(3));
        let mut w = XmlWriter::new();
        b.write_xml(&mut w, Ns::NONE, "renamed");
        assert_eq!(w.finish(), r#"<renamed id="3"/>"#);

        let raw = RawElement::parse(r#"<a k="v"><b/></a>"#).unwrap();
        let mut w = XmlWriter::new();
        raw.write_xml(&mut w, Ns::NONE, "z");
        assert_eq!(w.finish(), r#"<z k="v"><b/></z>"#);
        let typed: Item = RawElement::parse(r#"<item id="9"/>"#)
            .unwrap()
            .to_typed()
            .unwrap();
        assert_eq!(typed.id, Some(9));
        let back = RawElement::from_typed(&typed, Ns::NONE, "item");
        assert_eq!(back.attr(Ns::NONE, "id"), Some("9"));
    }
}
