//! Global symbol tables over a set of parsed schemas.

use std::collections::HashMap;

use crate::xsd::{Attribute, AttributeGroup, ComplexType, Element, Group, QName, Schema, SimpleType};

/// Index of the definitions of a schema set, keyed by expanded name.
pub struct Registry {
    /// The schemas, in load order.
    pub schemas: Vec<Schema>,
    simple: HashMap<QName, (usize, usize)>,
    complex: HashMap<QName, (usize, usize)>,
    groups: HashMap<QName, (usize, usize)>,
    attribute_groups: HashMap<QName, (usize, usize)>,
    elements: HashMap<QName, (usize, usize)>,
    attributes: HashMap<QName, (usize, usize)>,
}

impl Registry {
    /// Indexes the schemas. Later duplicates are ignored.
    pub fn new(schemas: Vec<Schema>) -> Self {
        let mut r = Registry {
            schemas,
            simple: HashMap::new(),
            complex: HashMap::new(),
            groups: HashMap::new(),
            attribute_groups: HashMap::new(),
            elements: HashMap::new(),
            attributes: HashMap::new(),
        };
        for (si, s) in r.schemas.iter().enumerate() {
            let ns = &s.target_ns;
            let key = |name: &str| QName::new(ns, name);
            for (i, t) in s.simple_types.iter().enumerate() {
                r.simple.entry(key(&t.name)).or_insert((si, i));
            }
            for (i, t) in s.complex_types.iter().enumerate() {
                r.complex.entry(key(&t.name)).or_insert((si, i));
            }
            for (i, g) in s.groups.iter().enumerate() {
                r.groups.entry(key(&g.name)).or_insert((si, i));
            }
            for (i, g) in s.attribute_groups.iter().enumerate() {
                r.attribute_groups.entry(key(&g.name)).or_insert((si, i));
            }
            for (i, e) in s.elements.iter().enumerate() {
                r.elements.entry(key(&e.name)).or_insert((si, i));
            }
            for (i, a) in s.attributes.iter().enumerate() {
                r.attributes.entry(key(&a.name)).or_insert((si, i));
            }
        }
        r
    }

    /// A named simple type and the index of its schema.
    pub fn simple(&self, q: &QName) -> Option<(usize, &SimpleType)> {
        self.simple
            .get(q)
            .map(|&(s, i)| (s, &self.schemas[s].simple_types[i]))
    }

    /// A named complex type and the index of its schema.
    pub fn complex(&self, q: &QName) -> Option<(usize, &ComplexType)> {
        self.complex
            .get(q)
            .map(|&(s, i)| (s, &self.schemas[s].complex_types[i]))
    }

    /// A model group and the index of its schema.
    pub fn group(&self, q: &QName) -> Option<(usize, &Group)> {
        self.groups.get(q).map(|&(s, i)| (s, &self.schemas[s].groups[i]))
    }

    /// An attribute group and the index of its schema.
    pub fn attribute_group(&self, q: &QName) -> Option<(usize, &AttributeGroup)> {
        self.attribute_groups
            .get(q)
            .map(|&(s, i)| (s, &self.schemas[s].attribute_groups[i]))
    }

    /// A global element and the index of its schema.
    pub fn element(&self, q: &QName) -> Option<(usize, &Element)> {
        self.elements
            .get(q)
            .map(|&(s, i)| (s, &self.schemas[s].elements[i]))
    }

    /// A global attribute and the index of its schema.
    pub fn attribute(&self, q: &QName) -> Option<(usize, &Attribute)> {
        self.attributes
            .get(q)
            .map(|&(s, i)| (s, &self.schemas[s].attributes[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xsd::parse_schema;

    #[test]
    fn indexes_definitions_by_expanded_name() {
        let a = parse_schema(
            "a.xsd",
            r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a">
                <xsd:simpleType name="ST"><xsd:restriction base="xsd:int"/></xsd:simpleType>
                <xsd:complexType name="CT"/>
                <xsd:group name="G"><xsd:sequence/></xsd:group>
                <xsd:attributeGroup name="AG"/>
                <xsd:element name="e" type="CT"/>
                <xsd:attribute name="at" type="xsd:int"/>
            </xsd:schema>"#,
        )
        .unwrap();
        let b = parse_schema(
            "b.xsd",
            r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:b">
                <xsd:complexType name="CT"/></xsd:schema>"#,
        )
        .unwrap();
        let r = Registry::new(vec![a, b]);
        assert_eq!(r.simple(&QName::new("urn:a", "ST")).unwrap().0, 0);
        assert_eq!(r.complex(&QName::new("urn:b", "CT")).unwrap().0, 1);
        assert!(r.complex(&QName::new("urn:c", "CT")).is_none());
        assert!(r.group(&QName::new("urn:a", "G")).is_some());
        assert!(r.attribute_group(&QName::new("urn:a", "AG")).is_some());
        assert_eq!(r.element(&QName::new("urn:a", "e")).unwrap().1.name, "e");
        assert!(r.attribute(&QName::new("urn:a", "at")).is_some());
    }
}
