//! Namespace-aware XML writer.
//!
//! The writer keeps track of the namespace bindings in scope. Elements and
//! attributes are named by [`Ns`] (or by URI for foreign namespaces); prefixes
//! are chosen automatically, reusing existing bindings and declaring new ones
//! on demand. Declarations captured from a source document (see
//! [`crate::RawAttribute::namespace_declaration`]) are re-emitted as-is so
//! that prefixes referenced from attribute values (e.g. `mc:Ignorable`) stay
//! valid.

use std::ops::Range;

use crate::ns::Ns;
use crate::raw::{RawAttribute, RawName};
use crate::value::XmlValue;

/// The XML declaration written at the top of every OOXML part.
pub const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

#[derive(Debug, Clone)]
struct Binding {
    prefix: String,
    ns: Ns,
    /// URI for [`Ns::OTHER`] bindings.
    uri: Option<String>,
}

impl Binding {
    fn matches(&self, ns: Ns, uri: Option<&str>) -> bool {
        if self.ns != ns {
            return false;
        }
        ns != Ns::OTHER || self.uri.as_deref() == uri
    }

    fn uri(&self) -> &str {
        match &self.uri {
            Some(u) => u,
            None => self.ns.uri(),
        }
    }
}

#[derive(Debug)]
struct Frame {
    name: Range<usize>,
    bindings_len: usize,
}

/// Streaming XML writer producing a `String`.
#[derive(Debug, Default)]
pub struct XmlWriter {
    out: String,
    open: bool,
    frames: Vec<Frame>,
    names: String,
    bindings: Vec<Binding>,
    predeclare: Vec<Ns>,
    scratch: String,
}

impl XmlWriter {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a writer whose output starts with the standard OOXML XML declaration.
    pub fn with_declaration() -> Self {
        let mut w = Self::new();
        w.out.push_str(XML_DECLARATION);
        w
    }

    /// Requests namespace declarations for `namespaces` on the next element that
    /// is started (typically the root). Namespaces that are already bound are
    /// skipped.
    pub fn predeclare(&mut self, namespaces: &[Ns]) {
        self.predeclare.extend_from_slice(namespaces);
    }

    /// Number of currently open elements.
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Returns the XML written so far.
    pub fn finish(self) -> String {
        debug_assert!(self.frames.is_empty(), "unclosed elements");
        self.out
    }

    fn close_start_tag(&mut self) {
        if self.open {
            self.out.push('>');
            self.open = false;
        }
    }

    /// Finds the prefix of a visible (non-shadowed) binding for the namespace.
    fn visible_prefix(&self, ns: Ns, uri: Option<&str>, allow_default: bool) -> Option<&str> {
        for (i, b) in self.bindings.iter().enumerate().rev() {
            if !b.matches(ns, uri) || (!allow_default && b.prefix.is_empty()) {
                continue;
            }
            let shadowed = self.bindings[i + 1..]
                .iter()
                .any(|later| later.prefix == b.prefix);
            if !shadowed {
                return Some(&b.prefix);
            }
        }
        None
    }

    /// The binding currently visible for `prefix`, if any.
    fn binding_of_prefix(&self, prefix: &str) -> Option<&Binding> {
        self.bindings.iter().rev().find(|b| b.prefix == prefix)
    }

    /// Chooses a prefix that can be bound on the current element without
    /// conflicting with bindings in scope.
    fn fresh_prefix(&self, wanted: &str) -> String {
        let usable =
            |p: &str| !p.is_empty() && p != "xml" && p != "xmlns" && self.binding_of_prefix(p).is_none();
        if usable(wanted) {
            return wanted.to_owned();
        }
        let base = if wanted.is_empty() || wanted == "xml" || wanted == "xmlns" {
            "ns"
        } else {
            wanted
        };
        (1..)
            .map(|i| format!("{base}{i}"))
            .find(|p| usable(p))
            .expect("an unused prefix exists")
    }

    fn push_binding(&mut self, prefix: String, ns: Ns, uri: Option<&str>) {
        self.bindings.push(Binding {
            prefix,
            ns,
            uri: if ns == Ns::OTHER {
                uri.map(str::to_owned)
            } else {
                None
            },
        });
    }

    fn write_binding(&mut self, index: usize) {
        let b = &self.bindings[index];
        self.out.push_str(" xmlns");
        if !b.prefix.is_empty() {
            self.out.push(':');
            self.out.push_str(&b.prefix);
        }
        self.out.push_str("=\"");
        escape_attr(&mut self.out, b.uri());
        self.out.push('"');
    }

    fn element_prefix(&mut self, ns: Ns, uri: Option<&str>, hint: Option<&str>) -> String {
        if ns == Ns::NONE {
            // Unqualified element: make sure no default namespace applies.
            if let Some(b) = self.binding_of_prefix("")
                && b.ns != Ns::NONE
            {
                self.push_binding(String::new(), Ns::NONE, None);
            }
            return String::new();
        }
        if ns == Ns::XML {
            return "xml".into();
        }
        if let Some(p) = self.visible_prefix(ns, uri, true) {
            return p.to_owned();
        }
        let wanted = hint.unwrap_or(ns.prefix());
        let prefix = if wanted.is_empty() {
            // The element was unprefixed in its source: bind the default namespace.
            String::new()
        } else {
            self.fresh_prefix(wanted)
        };
        self.push_binding(prefix.clone(), ns, uri);
        prefix
    }

    fn attribute_prefix(&mut self, ns: Ns, uri: Option<&str>, hint: Option<&str>) -> Option<String> {
        match ns {
            Ns::NONE => return None,
            Ns::XML => return Some("xml".into()),
            _ => {}
        }
        if let Some(p) = self.visible_prefix(ns, uri, false) {
            return Some(p.to_owned());
        }
        let wanted = hint.filter(|h| !h.is_empty()).unwrap_or(ns.prefix());
        let prefix = self.fresh_prefix(wanted);
        self.push_binding(prefix.clone(), ns, uri);
        let idx = self.bindings.len() - 1;
        self.write_binding(idx);
        Some(prefix)
    }

    fn open_element(
        &mut self,
        ns: Ns,
        uri: Option<&str>,
        hint: Option<&str>,
        local: &str,
        attrs: &[RawAttribute],
    ) {
        self.close_start_tag();
        let start = self.bindings.len();
        // Explicit declarations captured from the source document.
        for a in attrs.iter().filter(|a| a.is_namespace_declaration()) {
            let prefix: &str = &a.name.local;
            let declared_here = self.bindings[start..].iter().any(|b| b.prefix == prefix);
            let target = Ns::classify(&a.value);
            let redundant = self
                .binding_of_prefix(prefix)
                .is_some_and(|b| b.matches(target, Some(&a.value)));
            if declared_here || redundant || prefix == "xml" || prefix == "xmlns" {
                continue;
            }
            self.push_binding(prefix.to_owned(), target, Some(&a.value));
        }
        // Declarations requested for the root element of a part.
        for ns in std::mem::take(&mut self.predeclare) {
            if !ns.is_known() || ns == Ns::XML || ns == Ns::XMLNS {
                continue;
            }
            if self.visible_prefix(ns, None, true).is_some() {
                continue;
            }
            let default_free = !self.bindings[start..].iter().any(|b| b.prefix.is_empty())
                && self.binding_of_prefix("").is_none();
            let prefix = if ns.default_on_root() && default_free {
                String::new()
            } else if self.binding_of_prefix(ns.prefix()).is_none() {
                ns.prefix().to_owned()
            } else {
                continue;
            };
            self.push_binding(prefix, ns, None);
        }
        let prefix = self.element_prefix(ns, uri, hint);
        self.out.push('<');
        let name_start = self.names.len();
        if !prefix.is_empty() {
            self.names.push_str(&prefix);
            self.names.push(':');
        }
        self.names.push_str(local);
        self.out.push_str(&self.names[name_start..]);
        for i in start..self.bindings.len() {
            self.write_binding(i);
        }
        self.frames.push(Frame {
            name: name_start..self.names.len(),
            bindings_len: start,
        });
        self.open = true;
    }

    /// Starts an element in a known namespace (or [`Ns::NONE`]).
    pub fn start(&mut self, ns: Ns, local: &str) {
        self.open_element(ns, None, None, local, &[]);
    }

    /// Starts an element and applies the namespace declarations found in
    /// `extra_attrs`. The remaining extra attributes must be written with
    /// [`XmlWriter::attrs_raw`] before any content.
    pub fn start_with(&mut self, ns: Ns, local: &str, extra_attrs: &[RawAttribute]) {
        self.open_element(ns, None, None, local, extra_attrs);
    }

    /// Starts an element named by a [`RawName`], applying the namespace
    /// declarations found in `attrs`.
    pub fn start_raw(&mut self, name: &RawName, attrs: &[RawAttribute]) {
        let uri = name.uri.as_deref();
        self.open_element(name.ns, uri, name.prefix.as_deref(), &name.local, attrs);
    }

    fn attr_name(&mut self, ns: Ns, uri: Option<&str>, hint: Option<&str>, local: &str) {
        debug_assert!(self.open, "attributes must be written before content");
        let prefix = self.attribute_prefix(ns, uri, hint);
        self.out.push(' ');
        if let Some(p) = prefix {
            self.out.push_str(&p);
            self.out.push(':');
        }
        self.out.push_str(local);
        self.out.push_str("=\"");
    }

    /// Writes an attribute of the current element.
    pub fn attr(&mut self, ns: Ns, local: &str, value: &str) {
        self.attr_name(ns, None, None, local);
        escape_attr(&mut self.out, value);
        self.out.push('"');
    }

    /// Writes an attribute whose value is a schema simple type.
    pub fn attr_value<T: XmlValue>(&mut self, ns: Ns, local: &str, value: &T) {
        self.attr_name(ns, None, None, local);
        self.scratch.clear();
        value.write_xml(&mut self.scratch);
        escape_attr(&mut self.out, &self.scratch);
        self.out.push('"');
    }

    /// Writes a raw attribute. Namespace declarations are ignored here (they are
    /// handled when the element is started).
    pub fn attr_raw(&mut self, a: &RawAttribute) {
        if a.is_namespace_declaration() {
            return;
        }
        let n = &a.name;
        self.attr_name(n.ns, n.uri.as_deref(), n.prefix.as_deref(), &n.local);
        escape_attr(&mut self.out, &a.value);
        self.out.push('"');
    }

    /// Writes all non-declaration attributes of `attrs`.
    pub fn attrs_raw(&mut self, attrs: &[RawAttribute]) {
        for a in attrs {
            self.attr_raw(a);
        }
    }

    /// Writes character data.
    pub fn text(&mut self, s: &str) {
        self.close_start_tag();
        escape_text(&mut self.out, s);
    }

    /// Writes a simple-type value as character data.
    pub fn text_value<T: XmlValue>(&mut self, value: &T) {
        self.close_start_tag();
        self.scratch.clear();
        value.write_xml(&mut self.scratch);
        escape_text(&mut self.out, &self.scratch);
    }

    /// Writes a complete element whose content is a simple-type value. Adds
    /// `xml:space="preserve"` when the text has leading or trailing
    /// whitespace or contains line breaks or tabs, so that consumers keep it.
    pub fn simple_element<T: XmlValue>(&mut self, ns: Ns, local: &str, value: &T) {
        self.scratch.clear();
        value.write_xml(&mut self.scratch);
        self.start(ns, local);
        if needs_space_preserve(&self.scratch) {
            self.attr(Ns::XML, "space", "preserve");
        }
        if !self.scratch.is_empty() {
            self.close_start_tag();
            escape_text(&mut self.out, &self.scratch);
        }
        self.end();
    }

    /// Ends the innermost open element.
    pub fn end(&mut self) {
        let frame = self.frames.pop().expect("end() without matching start");
        if self.open {
            self.out.push_str("/>");
            self.open = false;
        } else {
            self.out.push_str("</");
            self.out.push_str(&self.names[frame.name.clone()]);
            self.out.push('>');
        }
        self.names.truncate(frame.name.start);
        self.bindings.truncate(frame.bindings_len);
    }

    /// Writes a complete element containing only text.
    pub fn text_element(&mut self, ns: Ns, local: &str, text: &str) {
        self.start(ns, local);
        if !text.is_empty() {
            self.text(text);
        }
        self.end();
    }
}

/// Whether text needs `xml:space="preserve"` to survive whitespace handling of consumers.
pub fn needs_space_preserve(s: &str) -> bool {
    s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) || s.contains(['\n', '\t'])
}

fn is_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Escapes character data. Characters that XML 1.0 cannot represent are dropped.
pub fn escape_text(out: &mut String, s: &str) {
    let mut last = 0;
    for (i, c) in s.char_indices() {
        let rep = match c {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '\r' => "&#xD;",
            c if !is_xml_char(c) => "",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + c.len_utf8();
    }
    out.push_str(&s[last..]);
}

/// Escapes an attribute value so that it survives attribute-value normalization.
/// Characters that XML 1.0 cannot represent are dropped.
pub fn escape_attr(out: &mut String, s: &str) {
    let mut last = 0;
    for (i, c) in s.char_indices() {
        let rep = match c {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\t' => "&#x9;",
            '\n' => "&#xA;",
            '\r' => "&#xD;",
            c if !is_xml_char(c) => "",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + c.len_utf8();
    }
    out.push_str(&s[last..]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::XmlReader;

    #[test]
    fn writes_prefixed_elements_with_declarations() {
        let mut w = XmlWriter::new();
        w.start(Ns::W, "p");
        w.attr(Ns::W, "rsidR", "00AB");
        w.start(Ns::W, "r");
        w.text_element(Ns::W, "t", "a < b & c > d");
        w.end();
        w.end();
        assert_eq!(
            w.finish(),
            r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:rsidR="00AB"><w:r><w:t>a &lt; b &amp; c &gt; d</w:t></w:r></w:p>"#
        );
    }

    #[test]
    fn predeclares_root_namespaces_and_default_namespace() {
        let mut w = XmlWriter::with_declaration();
        w.predeclare(&[Ns::X, Ns::R, Ns::MC]);
        w.start(Ns::X, "worksheet");
        w.start(Ns::X, "sheetData");
        w.end();
        w.start(Ns::X, "drawing");
        w.attr(Ns::R, "id", "rId1");
        w.end();
        w.end();
        let xml = w.finish();
        assert_eq!(
            xml,
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
                r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
                r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#,
                r#"<sheetData/><drawing r:id="rId1"/></worksheet>"#
            )
        );
    }

    #[test]
    fn attributes_never_use_the_default_namespace() {
        let mut w = XmlWriter::new();
        w.predeclare(&[Ns::X]);
        w.start(Ns::X, "a");
        w.attr(Ns::X, "b", "1");
        w.end();
        assert_eq!(
            w.finish(),
            r#"<a xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" x:b="1"/>"#
        );
    }

    #[test]
    fn declarations_are_scoped_to_elements() {
        let mut w = XmlWriter::new();
        w.start(Ns::NONE, "root");
        w.start(Ns::A, "x");
        w.end();
        w.start(Ns::A, "y");
        w.end();
        w.end();
        let a = Ns::A.uri();
        assert_eq!(
            w.finish(),
            format!(r#"<root><a:x xmlns:a="{a}"/><a:y xmlns:a="{a}"/></root>"#)
        );
    }

    #[test]
    fn conflicting_prefixes_get_fresh_names() {
        let mut w = XmlWriter::new();
        let decl = RawAttribute::namespace_declaration("w", "urn:not-word");
        w.start_raw(
            &RawName::with_uri("urn:not-word", Some("w"), "root"),
            std::slice::from_ref(&decl),
        );
        w.start(Ns::W, "p");
        w.end();
        w.end();
        let xml = w.finish();
        assert_eq!(
            xml,
            format!(
                r#"<w:root xmlns:w="urn:not-word"><w1:p xmlns:w1="{}"/></w:root>"#,
                Ns::W.uri()
            )
        );
        // The output must be well-formed and resolve to the right namespaces.
        let mut r = XmlReader::new(&xml);
        let root = r.root().unwrap();
        assert_eq!(root.ns(), Ns::OTHER);
        let p = r.next_child().unwrap().unwrap();
        assert_eq!(p.ns(), Ns::W);
    }

    #[test]
    fn duplicate_and_redundant_declarations_are_dropped() {
        let mut w = XmlWriter::new();
        let decls = [
            RawAttribute::namespace_declaration("w", Ns::W.uri()),
            RawAttribute::namespace_declaration("w", Ns::W.uri()),
        ];
        w.start_with(Ns::W, "document", &decls);
        w.start_with(Ns::W, "body", &decls[..1]);
        w.end();
        w.end();
        assert_eq!(
            w.finish(),
            format!(r#"<w:document xmlns:w="{}"><w:body/></w:document>"#, Ns::W.uri())
        );
    }

    #[test]
    fn escapes_attribute_whitespace_and_quotes() {
        let mut w = XmlWriter::new();
        w.start(Ns::NONE, "a");
        w.attr(Ns::NONE, "v", "x\t\"y\"\n\r&<>");
        w.end();
        let xml = w.finish();
        assert_eq!(xml, r#"<a v="x&#x9;&quot;y&quot;&#xA;&#xD;&amp;&lt;&gt;"/>"#);
        let mut r = XmlReader::new(&xml);
        let a = r.root().unwrap();
        assert_eq!(a.attr(Ns::NONE, "v").as_deref(), Some("x\t\"y\"\n\r&<>"));
    }

    #[test]
    fn text_round_trips_carriage_returns_and_drops_invalid_chars() {
        let mut w = XmlWriter::new();
        w.text_element(Ns::NONE, "t", "a\r\nb\u{1}c\u{FFFF}");
        let xml = w.finish();
        assert_eq!(xml, "<t>a&#xD;\nbc</t>");
        let mut r = XmlReader::new(&xml);
        let t = r.root().unwrap();
        assert_eq!(r.read_text(&t).unwrap(), "a\r\nbc");
    }

    #[test]
    fn xml_namespace_is_never_declared() {
        let mut w = XmlWriter::new();
        w.start(Ns::W, "t");
        w.attr(Ns::XML, "space", "preserve");
        w.text(" x ");
        w.end();
        assert_eq!(
            w.finish(),
            format!(r#"<w:t xmlns:w="{}" xml:space="preserve"> x </w:t>"#, Ns::W.uri())
        );
    }

    #[test]
    fn values_are_formatted_through_xml_value() {
        let mut w = XmlWriter::new();
        w.start(Ns::NONE, "n");
        w.attr_value(Ns::NONE, "b", &true);
        w.attr_value(Ns::NONE, "i", &-42i32);
        w.text_value(&1.5f64);
        w.end();
        assert_eq!(w.finish(), r#"<n b="true" i="-42">1.5</n>"#);
    }

    #[test]
    fn simple_elements_preserve_significant_whitespace() {
        let mut w = XmlWriter::new();
        w.start(Ns::NONE, "r");
        w.simple_element(Ns::NONE, "t", &String::from("plain"));
        w.simple_element(Ns::NONE, "t", &String::from(" lead"));
        w.simple_element(Ns::NONE, "t", &String::from("two\nlines"));
        w.simple_element(Ns::NONE, "t", &String::new());
        w.simple_element(Ns::NONE, "n", &42u32);
        w.end();
        assert_eq!(
            w.finish(),
            r#"<r><t>plain</t><t xml:space="preserve"> lead</t><t xml:space="preserve">two
lines</t><t/><n>42</n></r>"#
        );
        assert!(needs_space_preserve("a\tb"));
        assert!(!needs_space_preserve("a b"));
    }

    #[test]
    fn depth_tracks_open_elements() {
        let mut w = XmlWriter::new();
        assert_eq!(w.depth(), 0);
        w.start(Ns::NONE, "a");
        w.start(Ns::NONE, "b");
        assert_eq!(w.depth(), 2);
        w.end();
        w.end();
        assert_eq!(w.depth(), 0);
    }
}
