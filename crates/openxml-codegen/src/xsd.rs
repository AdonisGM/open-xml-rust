//! A parser for the subset of W3C XML Schema used by ECMA-376.
//!
//! The parser produces a syntax-level model: references (`type=`, `ref=`,
//! `base=`) are resolved to expanded names but not to their definitions.
//! Resolution happens in [`crate::registry`].

use std::fmt;

use roxmltree::{Document, Node};

/// The XML Schema namespace.
pub const XS: &str = "http://www.w3.org/2001/XMLSchema";
/// The XML namespace (`xml:` attributes).
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// An expanded name.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    /// Namespace URI (empty for no namespace).
    pub ns: String,
    /// Local name.
    pub name: String,
}

impl QName {
    /// Creates a name.
    pub fn new(ns: &str, name: &str) -> Self {
        QName {
            ns: ns.to_owned(),
            name: name.to_owned(),
        }
    }
}

impl fmt::Debug for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}{}", self.ns, self.name)
    }
}

/// `minOccurs` / `maxOccurs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurs {
    /// Minimum number of occurrences.
    pub min: u32,
    /// Maximum number of occurrences; `None` means unbounded.
    pub max: Option<u32>,
}

impl Occurs {
    /// Exactly once.
    pub const ONE: Occurs = Occurs { min: 1, max: Some(1) };

    /// Whether more than one occurrence is allowed.
    pub fn repeats(self) -> bool {
        self.max.is_none_or(|m| m > 1)
    }

    /// Composition of nested occurrence constraints.
    pub fn times(self, other: Occurs) -> Occurs {
        Occurs {
            min: self.min.saturating_mul(other.min),
            max: match (self.max, other.max) {
                (Some(a), Some(b)) => Some(a.saturating_mul(b)),
                (Some(0), _) | (_, Some(0)) => Some(0),
                _ => None,
            },
        }
    }
}

/// A simple type definition body.
#[derive(Clone, Debug, PartialEq)]
pub enum SimpleBody {
    /// `xsd:restriction`.
    Restriction {
        /// Named base type.
        base: Option<QName>,
        /// Anonymous base type.
        inline_base: Option<Box<SimpleBody>>,
        /// Enumerated values, in schema order.
        enumerations: Vec<String>,
    },
    /// `xsd:union`.
    Union {
        /// Named member types.
        members: Vec<QName>,
        /// Anonymous member types.
        inline: Vec<SimpleBody>,
    },
    /// `xsd:list`.
    List {
        /// Named item type.
        item: Option<QName>,
        /// Anonymous item type.
        inline: Option<Box<SimpleBody>>,
    },
}

/// A named simple type.
#[derive(Clone, Debug, PartialEq)]
pub struct SimpleType {
    /// Type name.
    pub name: String,
    /// Definition.
    pub body: SimpleBody,
}

/// An attribute declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// Local name.
    pub name: String,
    /// Named type.
    pub type_name: Option<QName>,
    /// Anonymous type.
    pub inline_type: Option<SimpleBody>,
    /// `use="required"`.
    pub required: bool,
    /// `use="prohibited"`.
    pub prohibited: bool,
    /// `default=`.
    pub default: Option<String>,
    /// `fixed=`.
    pub fixed: Option<String>,
    /// Whether the attribute name is namespace-qualified.
    pub qualified: bool,
}

/// An entry of an attribute list.
#[derive(Clone, Debug, PartialEq)]
pub enum AttrItem {
    /// A locally declared attribute.
    Local(Attribute),
    /// A reference to a global attribute.
    Ref {
        /// Referenced attribute.
        name: QName,
        /// `use="required"`.
        required: bool,
        /// `use="prohibited"`.
        prohibited: bool,
        /// `default=`.
        default: Option<String>,
    },
    /// A reference to an attribute group.
    Group(QName),
    /// `xsd:anyAttribute`.
    AnyAttribute,
}

/// An element declaration (global or local).
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// Local name.
    pub name: String,
    /// Named type (absent means `xsd:anyType` unless an inline type is given).
    pub type_name: Option<QName>,
    /// Anonymous complex type.
    pub inline_complex: Option<Box<ComplexType>>,
    /// Anonymous simple type.
    pub inline_simple: Option<SimpleBody>,
    /// Whether the element name is namespace-qualified.
    pub qualified: bool,
}

/// Reference from a particle to an element declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementRef {
    /// A local declaration.
    Local(Box<Element>),
    /// A reference to a global declaration.
    Ref(QName),
}

/// A content-model particle.
#[derive(Clone, Debug, PartialEq)]
pub enum Particle {
    /// An element.
    Element {
        /// The declaration.
        decl: ElementRef,
        /// Occurrence constraint.
        occurs: Occurs,
    },
    /// `xsd:any`.
    Any {
        /// The `namespace` constraint (e.g. `##any`, `##other`, URI list).
        namespace: String,
        /// Occurrence constraint.
        occurs: Occurs,
    },
    /// `xsd:sequence`.
    Sequence {
        /// Members.
        items: Vec<Particle>,
        /// Occurrence constraint.
        occurs: Occurs,
    },
    /// `xsd:choice`.
    Choice {
        /// Alternatives.
        items: Vec<Particle>,
        /// Occurrence constraint.
        occurs: Occurs,
    },
    /// `xsd:all`.
    All {
        /// Members.
        items: Vec<Particle>,
        /// Occurrence constraint.
        occurs: Occurs,
    },
    /// Reference to a model group.
    Group {
        /// Referenced group.
        name: QName,
        /// Occurrence constraint.
        occurs: Occurs,
    },
}

impl Particle {
    /// Occurrence constraint of the particle.
    pub fn occurs(&self) -> Occurs {
        match self {
            Particle::Element { occurs, .. }
            | Particle::Any { occurs, .. }
            | Particle::Sequence { occurs, .. }
            | Particle::Choice { occurs, .. }
            | Particle::All { occurs, .. }
            | Particle::Group { occurs, .. } => *occurs,
        }
    }
}

/// Content of a complex type.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// No content (attributes only).
    Empty,
    /// Element content.
    Particle(Particle),
    /// `xsd:simpleContent` with the given base type.
    Simple {
        /// Base (simple or complex-with-simple-content) type.
        base: QName,
    },
    /// `xsd:complexContent/xsd:extension`.
    Extension {
        /// Base complex type.
        base: QName,
        /// Content appended to the base content.
        particle: Option<Particle>,
    },
    /// `xsd:complexContent/xsd:restriction`.
    Restriction {
        /// Base complex type.
        base: QName,
        /// Replacement content.
        particle: Option<Particle>,
    },
}

/// A complex type.
#[derive(Clone, Debug, PartialEq)]
pub struct ComplexType {
    /// Type name (empty for anonymous types).
    pub name: String,
    /// `mixed="true"`.
    pub mixed: bool,
    /// Content model.
    pub content: Content,
    /// Attributes (including those of an extension/restriction).
    pub attributes: Vec<AttrItem>,
}

/// A named model group.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Group name.
    pub name: String,
    /// The group's particle.
    pub particle: Option<Particle>,
}

/// A named attribute group.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeGroup {
    /// Group name.
    pub name: String,
    /// Members.
    pub items: Vec<AttrItem>,
}

/// One schema document.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Schema {
    /// File name the schema was loaded from.
    pub file: String,
    /// `targetNamespace`.
    pub target_ns: String,
    /// Named simple types.
    pub simple_types: Vec<SimpleType>,
    /// Named complex types.
    pub complex_types: Vec<ComplexType>,
    /// Model groups.
    pub groups: Vec<Group>,
    /// Attribute groups.
    pub attribute_groups: Vec<AttributeGroup>,
    /// Global elements.
    pub elements: Vec<Element>,
    /// Global attributes.
    pub attributes: Vec<Attribute>,
}

/// Parse failure.
#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

fn err<T>(node: Node<'_, '_>, msg: &str) -> Result<T> {
    Err(ParseError(format!(
        "{msg} at <{}> (byte {})",
        node.tag_name().name(),
        node.range().start
    )))
}

struct Ctx {
    target_ns: String,
    element_form_qualified: bool,
    attribute_form_qualified: bool,
}

fn is_xs(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(XS) && node.tag_name().name() == name
}

fn xs_children<'a, 'i>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(|c| c.is_element() && c.tag_name().namespace() == Some(XS))
}

fn qname(node: Node<'_, '_>, value: &str) -> Result<QName> {
    let (prefix, local) = match value.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, value),
    };
    let ns = match prefix {
        Some("xml") => XML_NS.to_owned(),
        Some(p) => match node.lookup_namespace_uri(Some(p)) {
            Some(u) => u.to_owned(),
            None => return err(node, &format!("undeclared prefix {p}")),
        },
        None => node.lookup_namespace_uri(None).unwrap_or("").to_owned(),
    };
    Ok(QName {
        ns,
        name: local.to_owned(),
    })
}

fn attr_qname(node: Node<'_, '_>, name: &str) -> Result<Option<QName>> {
    node.attribute(name).map(|v| qname(node, v)).transpose()
}

fn occurs(node: Node<'_, '_>) -> Result<Occurs> {
    let min = match node.attribute("minOccurs") {
        Some(v) => v.parse().or_else(|_| err(node, "bad minOccurs"))?,
        None => 1,
    };
    let max = match node.attribute("maxOccurs") {
        Some("unbounded") => None,
        Some(v) => Some(v.parse().or_else(|_| err(node, "bad maxOccurs"))?),
        None => Some(1),
    };
    Ok(Occurs { min, max })
}

fn parse_simple_body(node: Node<'_, '_>) -> Result<SimpleBody> {
    for child in xs_children(node) {
        match child.tag_name().name() {
            "restriction" => {
                let base = attr_qname(child, "base")?;
                let inline_base = xs_children(child)
                    .find(|c| is_xs(*c, "simpleType"))
                    .map(parse_simple_body)
                    .transpose()?
                    .map(Box::new);
                let enumerations = xs_children(child)
                    .filter(|c| is_xs(*c, "enumeration"))
                    .map(|c| c.attribute("value").unwrap_or("").to_owned())
                    .collect();
                return Ok(SimpleBody::Restriction {
                    base,
                    inline_base,
                    enumerations,
                });
            }
            "union" => {
                let members = match child.attribute("memberTypes") {
                    Some(list) => list
                        .split_whitespace()
                        .map(|m| qname(child, m))
                        .collect::<Result<_>>()?,
                    None => Vec::new(),
                };
                let inline = xs_children(child)
                    .filter(|c| is_xs(*c, "simpleType"))
                    .map(parse_simple_body)
                    .collect::<Result<_>>()?;
                return Ok(SimpleBody::Union { members, inline });
            }
            "list" => {
                let item = attr_qname(child, "itemType")?;
                let inline = xs_children(child)
                    .find(|c| is_xs(*c, "simpleType"))
                    .map(parse_simple_body)
                    .transpose()?
                    .map(Box::new);
                return Ok(SimpleBody::List { item, inline });
            }
            "annotation" => {}
            _ => return err(child, "unsupported simple type construct"),
        }
    }
    err(node, "empty simple type")
}

fn parse_attribute(ctx: &Ctx, node: Node<'_, '_>, global: bool) -> Result<AttrItem> {
    let usage = node.attribute("use").unwrap_or("optional");
    let required = usage == "required";
    let prohibited = usage == "prohibited";
    let default = node.attribute("default").map(str::to_owned);
    if let Some(r) = node.attribute("ref") {
        return Ok(AttrItem::Ref {
            name: qname(node, r)?,
            required,
            prohibited,
            default,
        });
    }
    let Some(name) = node.attribute("name") else {
        return err(node, "attribute without name or ref");
    };
    let qualified = global
        || match node.attribute("form") {
            Some(f) => f == "qualified",
            None => ctx.attribute_form_qualified,
        };
    let inline_type = xs_children(node)
        .find(|c| is_xs(*c, "simpleType"))
        .map(parse_simple_body)
        .transpose()?;
    Ok(AttrItem::Local(Attribute {
        name: name.to_owned(),
        type_name: attr_qname(node, "type")?,
        inline_type,
        required,
        prohibited,
        default,
        fixed: node.attribute("fixed").map(str::to_owned),
        qualified,
    }))
}

fn parse_attr_items(ctx: &Ctx, node: Node<'_, '_>) -> Result<Vec<AttrItem>> {
    let mut items = Vec::new();
    for c in xs_children(node) {
        match c.tag_name().name() {
            "attribute" => items.push(parse_attribute(ctx, c, false)?),
            "attributeGroup" => match c.attribute("ref") {
                Some(r) => items.push(AttrItem::Group(qname(c, r)?)),
                None => return err(c, "nested attribute group definition"),
            },
            "anyAttribute" => items.push(AttrItem::AnyAttribute),
            _ => {}
        }
    }
    Ok(items)
}

fn parse_element(ctx: &Ctx, node: Node<'_, '_>, global: bool) -> Result<Element> {
    let Some(name) = node.attribute("name") else {
        return err(node, "element without name");
    };
    let qualified = global
        || match node.attribute("form") {
            Some(f) => f == "qualified",
            None => ctx.element_form_qualified,
        };
    let inline_complex = xs_children(node)
        .find(|c| is_xs(*c, "complexType"))
        .map(|c| parse_complex(ctx, c, ""))
        .transpose()?
        .map(Box::new);
    let inline_simple = xs_children(node)
        .find(|c| is_xs(*c, "simpleType"))
        .map(parse_simple_body)
        .transpose()?;
    Ok(Element {
        name: name.to_owned(),
        type_name: attr_qname(node, "type")?,
        inline_complex,
        inline_simple,
        qualified,
    })
}

fn parse_particle(ctx: &Ctx, node: Node<'_, '_>) -> Result<Option<Particle>> {
    let occ = occurs(node)?;
    let items = |n: Node<'_, '_>| -> Result<Vec<Particle>> {
        let mut v = Vec::new();
        for c in xs_children(n) {
            if let Some(p) = parse_particle(ctx, c)? {
                v.push(p);
            }
        }
        Ok(v)
    };
    Ok(Some(match node.tag_name().name() {
        "element" => {
            let decl = match node.attribute("ref") {
                Some(r) => ElementRef::Ref(qname(node, r)?),
                None => ElementRef::Local(Box::new(parse_element(ctx, node, false)?)),
            };
            Particle::Element { decl, occurs: occ }
        }
        "any" => Particle::Any {
            namespace: node.attribute("namespace").unwrap_or("##any").to_owned(),
            occurs: occ,
        },
        "sequence" => Particle::Sequence {
            items: items(node)?,
            occurs: occ,
        },
        "choice" => Particle::Choice {
            items: items(node)?,
            occurs: occ,
        },
        "all" => Particle::All {
            items: items(node)?,
            occurs: occ,
        },
        "group" => match node.attribute("ref") {
            Some(r) => Particle::Group {
                name: qname(node, r)?,
                occurs: occ,
            },
            None => return err(node, "nested group definition"),
        },
        "annotation" | "attribute" | "attributeGroup" | "anyAttribute" => return Ok(None),
        other => return err(node, &format!("unexpected particle {other}")),
    }))
}

fn first_particle(ctx: &Ctx, node: Node<'_, '_>) -> Result<Option<Particle>> {
    for c in xs_children(node) {
        if matches!(c.tag_name().name(), "sequence" | "choice" | "all" | "group") {
            return parse_particle(ctx, c);
        }
    }
    Ok(None)
}

fn parse_complex(ctx: &Ctx, node: Node<'_, '_>, name: &str) -> Result<ComplexType> {
    let mixed = node.attribute("mixed") == Some("true");
    let mut attributes = parse_attr_items(ctx, node)?;
    let mut content = match first_particle(ctx, node)? {
        Some(p) => Content::Particle(p),
        None => Content::Empty,
    };
    for c in xs_children(node) {
        match c.tag_name().name() {
            "simpleContent" => {
                let Some(inner) = xs_children(c).find(|n| is_xs(*n, "extension") || is_xs(*n, "restriction"))
                else {
                    return err(c, "simpleContent without derivation");
                };
                let Some(base) = attr_qname(inner, "base")? else {
                    return err(inner, "derivation without base");
                };
                attributes.extend(parse_attr_items(ctx, inner)?);
                content = Content::Simple { base };
            }
            "complexContent" => {
                let mixed_cc = c.attribute("mixed") == Some("true");
                let Some(inner) = xs_children(c).find(|n| is_xs(*n, "extension") || is_xs(*n, "restriction"))
                else {
                    return err(c, "complexContent without derivation");
                };
                let Some(base) = attr_qname(inner, "base")? else {
                    return err(inner, "derivation without base");
                };
                attributes.extend(parse_attr_items(ctx, inner)?);
                let particle = first_particle(ctx, inner)?;
                content = if is_xs(inner, "extension") {
                    Content::Extension { base, particle }
                } else {
                    Content::Restriction { base, particle }
                };
                if mixed_cc {
                    return Ok(ComplexType {
                        name: name.to_owned(),
                        mixed: true,
                        content,
                        attributes,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(ComplexType {
        name: name.to_owned(),
        mixed,
        content,
        attributes,
    })
}

/// Parses one schema document.
pub fn parse_schema(file: &str, text: &str) -> Result<Schema> {
    let doc = Document::parse(text).map_err(|e| ParseError(format!("{file}: {e}")))?;
    let root = doc.root_element();
    if !is_xs(root, "schema") {
        return Err(ParseError(format!("{file}: root is not xsd:schema")));
    }
    let ctx = Ctx {
        target_ns: root.attribute("targetNamespace").unwrap_or("").to_owned(),
        element_form_qualified: root.attribute("elementFormDefault") == Some("qualified"),
        attribute_form_qualified: root.attribute("attributeFormDefault") == Some("qualified"),
    };
    let mut schema = Schema {
        file: file.to_owned(),
        target_ns: ctx.target_ns.clone(),
        ..Default::default()
    };
    for node in xs_children(root) {
        let name = node.attribute("name").unwrap_or("").to_owned();
        match node.tag_name().name() {
            "simpleType" => schema.simple_types.push(SimpleType {
                name,
                body: parse_simple_body(node)?,
            }),
            "complexType" => schema.complex_types.push(parse_complex(&ctx, node, &name)?),
            "group" => schema.groups.push(Group {
                name,
                particle: first_particle(&ctx, node)?,
            }),
            "attributeGroup" => schema.attribute_groups.push(AttributeGroup {
                name,
                items: parse_attr_items(&ctx, node)?,
            }),
            "element" => schema.elements.push(parse_element(&ctx, node, true)?),
            "attribute" => match parse_attribute(&ctx, node, true)? {
                AttrItem::Local(a) => schema.attributes.push(a),
                _ => return err(node, "global attribute reference"),
            },
            "import" | "include" | "annotation" | "notation" => {}
            other => return err(node, &format!("unsupported top-level construct {other}")),
        }
    }
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r###"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns="urn:t" xmlns:s="urn:s"
        targetNamespace="urn:t" elementFormDefault="qualified" attributeFormDefault="qualified">
      <xsd:simpleType name="ST_Jc"><xsd:restriction base="xsd:string">
        <xsd:enumeration value="left"/><xsd:enumeration value="right"/></xsd:restriction></xsd:simpleType>
      <xsd:simpleType name="ST_U"><xsd:union memberTypes="xsd:boolean s:ST_X"/></xsd:simpleType>
      <xsd:simpleType name="ST_L"><xsd:list itemType="xsd:int"/></xsd:simpleType>
      <xsd:complexType name="CT_P">
        <xsd:sequence>
          <xsd:element name="pPr" type="CT_PPr" minOccurs="0"/>
          <xsd:group ref="EG_Content" minOccurs="0" maxOccurs="unbounded"/>
          <xsd:any namespace="##other" processContents="lax" minOccurs="0"/>
        </xsd:sequence>
        <xsd:attribute name="val" type="ST_Jc" use="required"/>
        <xsd:attribute name="loc" form="unqualified" type="xsd:string" default="x"/>
        <xsd:attribute ref="s:id"/>
        <xsd:attributeGroup ref="AG_Common"/>
      </xsd:complexType>
      <xsd:complexType name="CT_Ext">
        <xsd:complexContent><xsd:extension base="CT_P">
          <xsd:sequence><xsd:element ref="other" maxOccurs="3"/></xsd:sequence>
          <xsd:attribute name="more" type="xsd:int"/>
        </xsd:extension></xsd:complexContent>
      </xsd:complexType>
      <xsd:complexType name="CT_Text"><xsd:simpleContent><xsd:extension base="xsd:string">
        <xsd:attribute ref="xml:space"/></xsd:extension></xsd:simpleContent></xsd:complexType>
      <xsd:group name="EG_Content"><xsd:choice><xsd:element name="r" type="CT_R"/><xsd:element name="t" type="xsd:string"/></xsd:choice></xsd:group>
      <xsd:attributeGroup name="AG_Common"><xsd:attribute name="c" type="xsd:boolean"/></xsd:attributeGroup>
      <xsd:element name="p" type="CT_P"/>
      <xsd:attribute name="g" type="xsd:int"/>
    </xsd:schema>"###;

    #[test]
    fn parses_all_constructs() {
        let s = parse_schema("t.xsd", SAMPLE).unwrap();
        assert_eq!(s.target_ns, "urn:t");
        assert_eq!(s.simple_types.len(), 3);
        match &s.simple_types[0].body {
            SimpleBody::Restriction {
                base, enumerations, ..
            } => {
                assert_eq!(base.as_ref().unwrap(), &QName::new(XS, "string"));
                assert_eq!(enumerations, &["left", "right"]);
            }
            other => panic!("{other:?}"),
        }
        match &s.simple_types[1].body {
            SimpleBody::Union { members, .. } => {
                assert_eq!(members, &[QName::new(XS, "boolean"), QName::new("urn:s", "ST_X")]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&s.simple_types[2].body, SimpleBody::List { item: Some(q), .. } if q.name == "int"));

        let p = &s.complex_types[0];
        let Content::Particle(Particle::Sequence { items, occurs }) = &p.content else {
            panic!()
        };
        assert_eq!(*occurs, Occurs::ONE);
        assert_eq!(items.len(), 3);
        assert!(
            matches!(&items[1], Particle::Group { name, occurs } if name.name == "EG_Content" && occurs.max.is_none())
        );
        assert!(matches!(&items[2], Particle::Any { namespace, .. } if namespace == "##other"));
        assert_eq!(p.attributes.len(), 4);
        let AttrItem::Local(val) = &p.attributes[0] else {
            panic!()
        };
        assert!(val.required && val.qualified);
        let AttrItem::Local(loc) = &p.attributes[1] else {
            panic!()
        };
        assert!(!loc.qualified);
        assert_eq!(loc.default.as_deref(), Some("x"));
        assert!(matches!(&p.attributes[2], AttrItem::Ref { name, .. } if name == &QName::new("urn:s", "id")));
        assert!(matches!(&p.attributes[3], AttrItem::Group(g) if g.name == "AG_Common"));

        let ext = &s.complex_types[1];
        let Content::Extension {
            base,
            particle: Some(Particle::Sequence { items, .. }),
        } = &ext.content
        else {
            panic!("{:?}", ext.content)
        };
        assert_eq!(base.name, "CT_P");
        assert!(
            matches!(&items[0], Particle::Element { decl: ElementRef::Ref(q), occurs } if q.name == "other" && occurs.max == Some(3))
        );
        assert_eq!(ext.attributes.len(), 1);

        let text = &s.complex_types[2];
        assert!(matches!(&text.content, Content::Simple { base } if base.name == "string"));
        assert!(matches!(&text.attributes[0], AttrItem::Ref { name, .. } if name.ns == XML_NS));

        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.attribute_groups.len(), 1);
        assert_eq!(s.elements[0].name, "p");
        assert_eq!(s.attributes[0].name, "g");
        assert!(
            s.attributes[0].qualified,
            "global attributes are always qualified"
        );
    }

    #[test]
    fn occurs_arithmetic() {
        let opt = Occurs { min: 0, max: Some(1) };
        let many = Occurs { min: 1, max: None };
        assert!(!opt.repeats());
        assert!(many.repeats());
        assert!(Occurs { min: 0, max: Some(2) }.repeats());
        assert_eq!(opt.times(many), Occurs { min: 0, max: None });
        assert_eq!(Occurs::ONE.times(opt), opt);
        assert_eq!(Occurs { min: 0, max: Some(0) }.times(many).max, Some(0));
    }

    #[test]
    fn reports_errors() {
        assert!(parse_schema("x", "<notxsd/>").is_err());
        assert!(parse_schema("x", "not xml").is_err());
        let bad = r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"><xsd:element name="a" type="q:T"/></xsd:schema>"#;
        let e = parse_schema("x", bad).unwrap_err();
        assert!(e.to_string().contains("undeclared prefix q"), "{e}");
    }

    #[test]
    fn parses_every_vendored_schema() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        for sub in ["transitional", "strict"] {
            let mut count = 0;
            for entry in std::fs::read_dir(dir.join(sub)).unwrap() {
                let path = entry.unwrap().path();
                let text = std::fs::read_to_string(&path).unwrap();
                let schema = parse_schema(&path.display().to_string(), &text)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(!schema.target_ns.is_empty());
                count += 1;
            }
            assert!(count >= 21, "{sub}: {count}");
        }
    }
}
