//! Namespace-aware pull reader tailored to generated deserializers.

use std::borrow::Cow;
use std::ops::Range;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event as QEvent};
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;

use crate::error::{Error, Result};
use crate::ns::Ns;
use crate::raw::{RawAttribute, RawName};

/// Maximum element nesting accepted by [`XmlReader`].
///
/// Generated readers recurse once per element level; the limit protects the
/// stack against hostile or corrupt input.
pub const MAX_DEPTH: usize = 256;

/// Decodes the bytes of an XML part into text.
///
/// OOXML parts are UTF-8 or UTF-16 (ECMA-376 Part 2 §8.1.4). A byte order mark
/// is removed; UTF-16 input is transcoded.
pub fn decode_xml_bytes(bytes: &[u8]) -> Result<Cow<'_, str>> {
    fn utf16(bytes: &[u8], le: bool) -> Result<Cow<'static, str>> {
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::Encoding("odd number of bytes in UTF-16 input".into()));
        }
        let units = bytes.as_chunks::<2>().0.iter().map(|c| {
            if le {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        });
        char::decode_utf16(units)
            .collect::<std::result::Result<String, _>>()
            .map(Cow::Owned)
            .map_err(|e| Error::Encoding(e.to_string()))
    }
    let utf8 = |b| {
        std::str::from_utf8(b)
            .map(Cow::Borrowed)
            .map_err(|e| Error::Encoding(e.to_string()))
    };
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => utf8(rest),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [b'<', 0, ..] => utf16(bytes, true),
        [0, b'<', ..] => utf16(bytes, false),
        _ => utf8(bytes),
    }
}

/// Returns the namespace and local name of a document's root element.
pub fn root_name(xml: &str) -> Result<(Ns, String)> {
    let mut r = XmlReader::new(xml);
    let root = r.root()?;
    Ok((root.ns(), root.local().to_owned()))
}

/// An event returned by [`XmlReader::next_event`].
#[derive(Debug)]
pub enum Event<'i> {
    /// Start of an element (also used for empty elements, see [`StartTag::is_empty`]).
    Start(StartTag<'i>),
    /// End of the current element.
    End,
    /// Character data, with entity references resolved and adjacent pieces merged.
    Text(Cow<'i, str>),
    /// End of input.
    Eof,
}

#[derive(Debug)]
enum Slot {
    Borrowed(Range<usize>),
    Owned(String),
}

#[derive(Debug)]
struct AttrSlot {
    ns: Ns,
    uri: Option<Box<str>>,
    /// Range of the qualified name inside the raw attribute text.
    key: Range<usize>,
    /// Offset of the local name inside the raw attribute text.
    local_start: usize,
    value: Slot,
}

/// A resolved start tag.
#[derive(Debug)]
pub struct StartTag<'i> {
    raw: BytesStart<'i>,
    ns: Ns,
    uri: Option<Box<str>>,
    attrs: Vec<AttrSlot>,
    empty: bool,
}

/// A resolved attribute of a [`StartTag`].
///
/// Namespace declarations are reported as attributes in the [`Ns::XMLNS`]
/// namespace whose local name is the declared prefix (empty for the default
/// namespace) and whose value is the namespace URI (Strict URIs are mapped to
/// their Transitional equivalents).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr<'a> {
    /// Namespace of the attribute name.
    pub ns: Ns,
    /// URI of the namespace when `ns` is [`Ns::OTHER`].
    pub uri: Option<&'a str>,
    /// Prefix as written in the document.
    pub prefix: Option<&'a str>,
    /// Local name.
    pub local: &'a str,
    /// Normalized attribute value.
    pub value: Cow<'a, str>,
}

impl Attr<'_> {
    /// Converts the attribute into an owned [`RawAttribute`].
    pub fn to_raw(&self) -> RawAttribute {
        RawAttribute {
            name: RawName {
                ns: self.ns,
                uri: self.uri.map(Into::into),
                prefix: self.prefix.map(Into::into),
                local: self.local.into(),
            },
            value: self.value.clone().into_owned(),
        }
    }
}

fn offset_in(base: &str, sub: &str) -> Option<Range<usize>> {
    let b = base.as_ptr() as usize;
    let s = sub.as_ptr() as usize;
    if s >= b && s + sub.len() <= b + base.len() {
        Some(s - b..s - b + sub.len())
    } else {
        None
    }
}

impl<'i> StartTag<'i> {
    /// Namespace of the element.
    pub fn ns(&self) -> Ns {
        self.ns
    }

    /// Local name of the element.
    pub fn local(&self) -> &str {
        let name = self.raw.name().0;
        match name.find(':') {
            Some(i) => &name[i + 1..],
            None => name,
        }
    }

    /// Prefix of the element name as written in the document.
    pub fn prefix(&self) -> Option<&str> {
        let name = self.raw.name().0;
        name.find(':').map(|i| &name[..i])
    }

    /// Namespace URI of the element (canonical Transitional URI for known namespaces).
    pub fn uri(&self) -> &str {
        match &self.uri {
            Some(u) => u,
            None => self.ns.uri(),
        }
    }

    /// `true` for an empty-element tag (`<a/>`), which has no content and no end tag.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// Number of attributes (including namespace declarations).
    pub fn attribute_count(&self) -> usize {
        self.attrs.len()
    }

    /// Iterates over the attributes, including namespace declarations.
    pub fn attributes(&self) -> impl Iterator<Item = Attr<'_>> + '_ {
        let text = self.raw.attributes_raw();
        self.attrs.iter().map(move |slot| {
            let key = &text[slot.key.clone()];
            let local = &text[slot.local_start..slot.key.end];
            let prefix = if slot.local_start > slot.key.start {
                Some(&key[..slot.local_start - slot.key.start - 1])
            } else {
                None
            };
            let (prefix, local) = if slot.ns == Ns::XMLNS {
                // `xmlns:p` declares prefix `p`; `xmlns` declares the default namespace.
                (Some("xmlns"), if prefix.is_some() { local } else { "" })
            } else {
                (prefix, local)
            };
            let value = match &slot.value {
                Slot::Borrowed(r) => Cow::Borrowed(&text[r.clone()]),
                Slot::Owned(s) => Cow::Borrowed(s.as_str()),
            };
            Attr {
                ns: slot.ns,
                uri: slot.uri.as_deref(),
                prefix,
                local,
                value,
            }
        })
    }

    /// Returns the value of the attribute with the given namespace and local name.
    pub fn attr(&self, ns: Ns, local: &str) -> Option<Cow<'_, str>> {
        self.attributes()
            .find(|a| a.ns == ns && a.local == local)
            .map(|a| a.value)
    }

    /// The element name as an owned [`RawName`].
    pub fn raw_name(&self) -> RawName {
        RawName {
            ns: self.ns,
            uri: self.uri.clone(),
            prefix: self.prefix().map(Into::into),
            local: self.local().into(),
        }
    }

    /// Clark notation (`{uri}local`) of the element name, for diagnostics.
    pub fn clark_name(&self) -> String {
        format!("{{{}}}{}", self.uri(), self.local())
    }
}

/// Streaming reader over an in-memory XML document.
///
/// The reader resolves namespaces, merges adjacent character data and skips
/// comments, processing instructions and the XML declaration.
pub struct XmlReader<'i> {
    inner: NsReader<&'i [u8]>,
    pending: Option<Event<'i>>,
    depth: usize,
}

impl<'i> XmlReader<'i> {
    /// Creates a reader over XML text.
    pub fn new(xml: &'i str) -> Self {
        let mut inner = NsReader::from_str(xml);
        let config = inner.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = false;
        config.check_end_names = true;
        XmlReader {
            inner,
            pending: None,
            depth: 0,
        }
    }

    /// Current element depth (number of open elements).
    pub fn depth(&self) -> usize {
        self.depth
    }

    fn resolve_start(&self, raw: BytesStart<'i>, empty: bool) -> Result<StartTag<'i>> {
        let resolver = self.inner.resolver();
        let (res, _) = resolver.resolve_element(raw.name());
        let (ns, uri) = classify(res)?;
        let mut attrs = Vec::new();
        let text = raw.attributes_raw();
        for attr in raw.attributes() {
            let attr = attr?;
            let key = attr.key.0;
            let key_range =
                offset_in(text, key).ok_or_else(|| Error::Syntax("attribute name outside of tag".into()))?;
            let colon = key.find(':');
            let local_start = key_range.start + colon.map_or(0, |i| i + 1);
            let (ns, uri, value) = match attr.key.as_namespace_binding() {
                Some(_) => {
                    // Normalise Strict namespace URIs to their Transitional form.
                    let uri_text = attr.normalized_value(XmlVersion::Implicit1_0)?;
                    let canonical = match Ns::from_uri(&uri_text) {
                        Some(known) if known.uri() != uri_text => Slot::Owned(known.uri().into()),
                        _ => slot_for(text, uri_text),
                    };
                    (Ns::XMLNS, None, canonical)
                }
                None => {
                    let (res, _) = resolver.resolve_attribute(QName(key));
                    let (ns, uri) = match res {
                        ResolveResult::Unbound => (Ns::NONE, None),
                        other => classify(other)?,
                    };
                    let value = attr.normalized_value(XmlVersion::Implicit1_0)?;
                    (ns, uri, slot_for(text, value))
                }
            };
            attrs.push(AttrSlot {
                ns,
                uri,
                key: key_range,
                local_start,
                value,
            });
        }
        Ok(StartTag {
            raw,
            ns,
            uri,
            attrs,
            empty,
        })
    }

    fn read_raw(&mut self) -> Result<Option<Event<'i>>> {
        loop {
            let ev = self.inner.read_event()?;
            return Ok(Some(match ev {
                QEvent::Start(e) => {
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        return Err(Error::TooDeep);
                    }
                    Event::Start(self.resolve_start(e, false)?)
                }
                QEvent::Empty(e) => {
                    if self.depth + 1 > MAX_DEPTH {
                        return Err(Error::TooDeep);
                    }
                    Event::Start(self.resolve_start(e, true)?)
                }
                QEvent::End(_) => {
                    self.depth = self.depth.saturating_sub(1);
                    Event::End
                }
                QEvent::Text(t) => Event::Text(t.xml10_content()),
                QEvent::CData(c) => Event::Text(c.xml10_content()),
                QEvent::GeneralRef(r) => {
                    let resolved = match r.resolve_char_ref()? {
                        Some(ch) => Cow::Owned(ch.to_string()),
                        None => match quick_xml::escape::resolve_predefined_entity(&r) {
                            Some(s) => Cow::Borrowed(s),
                            None => {
                                return Err(Error::Syntax(format!("undefined entity reference &{};", &*r)));
                            }
                        },
                    };
                    Event::Text(resolved)
                }
                QEvent::Eof => Event::Eof,
                QEvent::Comment(_) | QEvent::PI(_) | QEvent::Decl(_) | QEvent::DocType(_) => {
                    continue;
                }
            }));
        }
    }

    /// Returns the next event.
    pub fn next_event(&mut self) -> Result<Event<'i>> {
        if let Some(ev) = self.pending.take() {
            return Ok(ev);
        }
        let first = match self.read_raw()? {
            Some(ev) => ev,
            None => return Ok(Event::Eof),
        };
        let Event::Text(mut text) = first else {
            return Ok(first);
        };
        // Merge adjacent character data (text, CDATA and entity references).
        loop {
            match self.read_raw()? {
                Some(Event::Text(more)) => text.to_mut().push_str(&more),
                Some(other) => {
                    self.pending = Some(other);
                    return Ok(Event::Text(text));
                }
                None => return Ok(Event::Text(text)),
            }
        }
    }

    /// Returns the root element's start tag, skipping the prolog.
    pub fn root(&mut self) -> Result<StartTag<'i>> {
        loop {
            match self.next_event()? {
                Event::Start(tag) => return Ok(tag),
                Event::Text(t) if t.trim().is_empty() => continue,
                Event::Text(_) => return Err(Error::Syntax("text before the root element".into())),
                Event::End => return Err(Error::Syntax("unexpected end tag".into())),
                Event::Eof => return Err(Error::NoRootElement),
            }
        }
    }

    /// Returns the next child element of the current element, or `None` when
    /// the current element ends. Character data is skipped.
    ///
    /// Callers must fully consume every returned child (by reading it or
    /// calling [`XmlReader::skip`]) before asking for the next one.
    pub fn next_child(&mut self) -> Result<Option<StartTag<'i>>> {
        loop {
            match self.next_event()? {
                Event::Start(tag) => return Ok(Some(tag)),
                Event::End => return Ok(None),
                Event::Text(_) => continue,
                Event::Eof => return Err(Error::UnexpectedEof),
            }
        }
    }

    /// Skips the remaining content of the element opened by `tag`.
    pub fn skip(&mut self, tag: &StartTag<'_>) -> Result<()> {
        if tag.is_empty() {
            return Ok(());
        }
        let mut level = 1usize;
        loop {
            match self.next_event()? {
                Event::Start(t) => {
                    if !t.is_empty() {
                        level += 1;
                    }
                }
                Event::End => {
                    level -= 1;
                    if level == 0 {
                        return Ok(());
                    }
                }
                Event::Text(_) => {}
                Event::Eof => return Err(Error::UnexpectedEof),
            }
        }
    }

    /// Reads the character content of the element opened by `tag` up to its end
    /// tag. Nested elements are skipped (their text is ignored).
    pub fn read_text(&mut self, tag: &StartTag<'_>) -> Result<Cow<'i, str>> {
        if tag.is_empty() {
            return Ok(Cow::Borrowed(""));
        }
        let mut out: Option<Cow<'i, str>> = None;
        loop {
            match self.next_event()? {
                Event::Text(t) => match &mut out {
                    None => out = Some(t),
                    Some(acc) => acc.to_mut().push_str(&t),
                },
                Event::Start(child) => self.skip(&child)?,
                Event::End => return Ok(out.unwrap_or(Cow::Borrowed(""))),
                Event::Eof => return Err(Error::UnexpectedEof),
            }
        }
    }
}

fn slot_for(text: &str, value: Cow<'_, str>) -> Slot {
    match &value {
        Cow::Borrowed(s) => match offset_in(text, s) {
            Some(r) => Slot::Borrowed(r),
            None => Slot::Owned((*s).to_owned()),
        },
        Cow::Owned(s) => Slot::Owned(s.clone()),
    }
}

fn classify(res: ResolveResult<'_>) -> Result<(Ns, Option<Box<str>>)> {
    match res {
        ResolveResult::Unbound => Ok((Ns::NONE, None)),
        ResolveResult::Bound(ns) => {
            let uri = ns.0;
            match Ns::from_uri(uri) {
                Some(known) => Ok((known, None)),
                None => Ok((Ns::OTHER, Some(uri.into()))),
            }
        }
        ResolveResult::Unknown(prefix) => {
            Err(Error::Syntax(format!("undeclared namespace prefix `{prefix}`")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn collect(xml: &str) -> Vec<String> {
        let mut r = XmlReader::new(xml);
        let mut out = Vec::new();
        loop {
            match r.next_event().unwrap() {
                Event::Start(t) => out.push(format!(
                    "start {:?} {}{}",
                    t.ns(),
                    t.local(),
                    if t.is_empty() { " (empty)" } else { "" }
                )),
                Event::End => out.push("end".into()),
                Event::Text(t) => out.push(format!("text {t:?}")),
                Event::Eof => break,
            }
        }
        out
    }

    #[test]
    fn resolves_prefixed_and_default_namespaces() {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{W}"><body xmlns="{W}"><w:p/></body></w:document>"#
        );
        assert_eq!(
            collect(&xml),
            vec![
                "start Ns::W document",
                "start Ns::W body",
                "start Ns::W p (empty)",
                "end",
                "end",
            ]
        );
    }

    #[test]
    fn strict_namespace_maps_to_transitional() {
        let xml = r#"<w:p xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main" w:rsidR="00AB"/>"#;
        let mut r = XmlReader::new(xml);
        let tag = r.root().unwrap();
        assert_eq!(tag.ns(), Ns::W);
        assert_eq!(tag.uri(), W);
        let attrs: Vec<_> = tag.attributes().collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].ns, Ns::XMLNS);
        assert_eq!(attrs[0].local, "w");
        assert_eq!(attrs[0].value, W, "declaration URI is canonicalised");
        assert_eq!(attrs[1].ns, Ns::W);
        assert_eq!(attrs[1].local, "rsidR");
        assert_eq!(attrs[1].prefix, Some("w"));
        assert_eq!(attrs[1].value, "00AB");
    }

    #[test]
    fn unknown_namespaces_keep_their_uri() {
        let xml = r#"<x:a xmlns:x="urn:test" x:b="1" c="2"/>"#;
        let mut r = XmlReader::new(xml);
        let tag = r.root().unwrap();
        assert_eq!(tag.ns(), Ns::OTHER);
        assert_eq!(tag.uri(), "urn:test");
        assert_eq!(tag.prefix(), Some("x"));
        let attrs: Vec<_> = tag.attributes().collect();
        assert_eq!(attrs[1].ns, Ns::OTHER);
        assert_eq!(attrs[1].uri, Some("urn:test"));
        assert_eq!(attrs[2].ns, Ns::NONE, "unprefixed attributes have no namespace");
        assert_eq!(tag.attr(Ns::NONE, "c").as_deref(), Some("2"));
        assert_eq!(tag.clark_name(), "{urn:test}a");
    }

    #[test]
    fn default_namespace_declaration_is_reported() {
        let xml = r#"<a xmlns="urn:test"/>"#;
        let mut r = XmlReader::new(xml);
        let tag = r.root().unwrap();
        let a: Vec<_> = tag.attributes().collect();
        assert_eq!(a[0].ns, Ns::XMLNS);
        assert_eq!(a[0].local, "");
        assert_eq!(a[0].value, "urn:test");
        let raw = a[0].to_raw();
        assert_eq!(raw.name.ns, Ns::XMLNS);
        assert_eq!(raw.value, "urn:test");
    }

    #[test]
    fn merges_text_entities_and_cdata() {
        let xml = "<a>x &amp; y &#65;&#x42;<![CDATA[<c>]]>z</a>";
        assert_eq!(
            collect(xml),
            vec!["start Ns::NONE a", "text \"x & y AB<c>z\"", "end"]
        );
    }

    #[test]
    fn normalises_line_endings_and_attribute_whitespace() {
        let xml = "<a v=\"1\t2&#10;3\">l1\r\nl2</a>";
        let mut r = XmlReader::new(xml);
        let tag = r.root().unwrap();
        assert_eq!(tag.attr(Ns::NONE, "v").as_deref(), Some("1 2\n3"));
        assert_eq!(r.read_text(&tag).unwrap(), "l1\nl2");
    }

    #[test]
    fn skips_comments_and_processing_instructions() {
        let xml = "<?xml version='1.0'?><!-- c --><a><?pi x?>t<!-- c -->u</a>";
        assert_eq!(collect(xml), vec!["start Ns::NONE a", "text \"tu\"", "end"]);
    }

    #[test]
    fn next_child_and_skip() {
        let xml = "<r> <a><deep><er/></deep></a> text <b/> </r>";
        let mut r = XmlReader::new(xml);
        let root = r.root().unwrap();
        assert_eq!(root.local(), "r");
        let a = r.next_child().unwrap().unwrap();
        assert_eq!(a.local(), "a");
        r.skip(&a).unwrap();
        let b = r.next_child().unwrap().unwrap();
        assert_eq!(b.local(), "b");
        assert!(b.is_empty());
        r.skip(&b).unwrap();
        assert!(r.next_child().unwrap().is_none());
        assert!(matches!(r.next_event().unwrap(), Event::Eof));
    }

    #[test]
    fn read_text_ignores_nested_elements() {
        let xml = "<t>a<b>ignored</b>c</t>";
        let mut r = XmlReader::new(xml);
        let t = r.root().unwrap();
        assert_eq!(r.read_text(&t).unwrap(), "ac");
    }

    #[test]
    fn read_text_of_empty_element() {
        let mut r = XmlReader::new("<t/>");
        let t = r.root().unwrap();
        assert_eq!(r.read_text(&t).unwrap(), "");
    }

    #[test]
    fn reports_errors() {
        assert_eq!(XmlReader::new("").root().unwrap_err(), Error::NoRootElement);
        assert_eq!(XmlReader::new("   ").root().unwrap_err(), Error::NoRootElement);
        assert!(matches!(
            XmlReader::new("<a><b></a>").root().and_then(|_| {
                let mut r = XmlReader::new("<a><b></a>");
                let a = r.root()?;
                r.skip(&a)
            }),
            Err(Error::Syntax(_))
        ));
        let mut r = XmlReader::new("<p:a/>");
        assert!(matches!(r.root(), Err(Error::Syntax(msg)) if msg.contains("undeclared")));
        let mut r = XmlReader::new("<a>&bogus;</a>");
        let a = r.root().unwrap();
        assert!(matches!(r.read_text(&a), Err(Error::Syntax(msg)) if msg.contains("bogus")));
        let mut r = XmlReader::new("<a><b>");
        let a = r.root().unwrap();
        assert!(r.skip(&a).is_err());
    }

    #[test]
    fn depth_limit_is_enforced() {
        let depth = MAX_DEPTH + 5;
        let xml = "<a>".repeat(depth) + &"</a>".repeat(depth);
        let mut r = XmlReader::new(&xml);
        let root = r.root().unwrap();
        assert_eq!(r.skip(&root).unwrap_err(), Error::TooDeep);
    }

    #[test]
    fn xml_prefix_is_predeclared() {
        let mut r = XmlReader::new(r#"<t xml:space="preserve"> a </t>"#);
        let t = r.root().unwrap();
        assert_eq!(t.attr(Ns::XML, "space").as_deref(), Some("preserve"));
        assert_eq!(r.read_text(&t).unwrap(), " a ");
    }

    #[test]
    fn decodes_encodings() {
        assert_eq!(decode_xml_bytes(b"<a/>").unwrap(), "<a/>");
        assert_eq!(decode_xml_bytes(b"\xEF\xBB\xBF<a/>").unwrap(), "<a/>");
        let le: Vec<u8> = "\u{feff}<a>é</a>"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_xml_bytes(&le).unwrap(), "<a>é</a>");
        let be: Vec<u8> = "\u{feff}<a/>"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(decode_xml_bytes(&be).unwrap(), "<a/>");
        let le_nobom: Vec<u8> = "<a/>".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_xml_bytes(&le_nobom).unwrap(), "<a/>");
        assert!(decode_xml_bytes(b"\xFF\xFE<").is_err());
        assert!(decode_xml_bytes(b"<a>\xC3</a>").is_err());
    }

    #[test]
    fn root_name_of_document() {
        let xml = format!(r#"<?xml version="1.0"?><!-- c --><w:document xmlns:w="{W}"/>"#);
        assert_eq!(root_name(&xml).unwrap(), (Ns::W, "document".to_owned()));
        assert!(root_name("").is_err());
    }

    #[test]
    fn raw_name_of_start_tag() {
        let xml = format!(r#"<w:p xmlns:w="{W}"/>"#);
        let mut r = XmlReader::new(&xml);
        let t = r.root().unwrap();
        let n = t.raw_name();
        assert_eq!(n.ns, Ns::W);
        assert_eq!(n.prefix.as_deref(), Some("w"));
        assert_eq!(&*n.local, "p");
        assert_eq!(t.attribute_count(), 1);
    }
}
