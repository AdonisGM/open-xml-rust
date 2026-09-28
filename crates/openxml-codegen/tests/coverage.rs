//! Completeness of the generated model.
//!
//! This test resolves the ECMA-376 Transitional schemas independently of the
//! generator (groups, attribute groups, extensions, references) and checks
//! that the generated IR — which is what the emitter prints — contains every
//! complex type with exactly its attributes and child elements, every simple
//! type with every enumeration value, and every global element.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use openxml_codegen::ir::{ContentDef, FieldKind, SimpleKind};
use openxml_codegen::lower::{lower, module_name, ns_by_ident};
use openxml_codegen::registry::Registry;
use openxml_codegen::spec::SpecIndex;
use openxml_codegen::xsd::{
    AttrItem, ComplexType, Content, ElementRef, Particle, QName, SimpleBody, XML_NS, XS,
};

type Name = (String, String);

struct Expect<'a> {
    reg: &'a Registry,
}

impl Expect<'_> {
    fn target(&self, si: usize) -> &str {
        &self.reg.schemas[si].target_ns
    }

    fn particle(&self, si: usize, p: &Particle, out: &mut BTreeSet<Name>) {
        if p.occurs().max == Some(0) {
            return;
        }
        match p {
            Particle::Element {
                decl: ElementRef::Local(e),
                ..
            } => {
                let ns = if e.qualified {
                    self.target(si).to_owned()
                } else {
                    String::new()
                };
                out.insert((ns, e.name.clone()));
            }
            Particle::Element {
                decl: ElementRef::Ref(q),
                ..
            } => {
                out.insert((q.ns.clone(), q.name.clone()));
            }
            Particle::Sequence { items, .. }
            | Particle::Choice { items, .. }
            | Particle::All { items, .. } => {
                for i in items {
                    self.particle(si, i, out);
                }
            }
            Particle::Group { name, .. } => {
                let (gsi, g) = self.reg.group(name).expect("group");
                if let Some(gp) = &g.particle {
                    self.particle(gsi, gp, out);
                }
            }
            Particle::Any { .. } => {}
        }
    }

    fn elements(&self, si: usize, ct: &ComplexType, out: &mut BTreeSet<Name>) {
        match &ct.content {
            Content::Particle(p) => self.particle(si, p, out),
            Content::Extension { base, particle } => {
                if let Some((bsi, b)) = self.reg.complex(base) {
                    self.elements(bsi, b, out);
                }
                if let Some(p) = particle {
                    self.particle(si, p, out);
                }
            }
            Content::Restriction {
                particle: Some(p), ..
            } => self.particle(si, p, out),
            _ => {}
        }
    }

    fn attr_items(&self, si: usize, items: &[AttrItem], out: &mut BTreeSet<Name>) {
        for item in items {
            match item {
                AttrItem::Local(a) => {
                    let key = (
                        if a.qualified {
                            self.target(si).to_owned()
                        } else {
                            String::new()
                        },
                        a.name.clone(),
                    );
                    if a.prohibited {
                        out.remove(&key);
                    } else {
                        out.insert(key);
                    }
                }
                AttrItem::Ref { name, prohibited, .. } => {
                    let key = (name.ns.clone(), name.name.clone());
                    if *prohibited {
                        out.remove(&key);
                    } else {
                        out.insert(key);
                    }
                }
                AttrItem::Group(q) => {
                    let (gsi, g) = self.reg.attribute_group(q).expect("attribute group");
                    self.attr_items(gsi, &g.items, out);
                }
                AttrItem::AnyAttribute => {}
            }
        }
    }

    fn attributes(&self, si: usize, ct: &ComplexType, out: &mut BTreeSet<Name>) {
        match &ct.content {
            Content::Extension { base, .. }
            | Content::Restriction { base, .. }
            | Content::Simple { base } => {
                if let Some((bsi, b)) = self.reg.complex(base) {
                    self.attributes(bsi, b, out);
                }
            }
            _ => {}
        }
        self.attr_items(si, &ct.attributes, out);
    }

    fn is_mixed(&self, ct: &ComplexType) -> bool {
        ct.mixed
            || matches!(&ct.content, Content::Extension { base, .. }
                if self.reg.complex(base).is_some_and(|(_, b)| self.is_mixed(b)))
    }
}

fn uri(ident: &str) -> String {
    match ident {
        "NONE" => String::new(),
        "XML" => XML_NS.to_owned(),
        other => ns_by_ident(other).uri().to_owned(),
    }
}

#[test]
fn every_type_element_attribute_and_value_is_generated() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/transitional");
    let schemas = openxml_codegen::load_schemas(&root).unwrap();
    let reg = Registry::new(schemas);
    let krate = lower(&reg, &SpecIndex::empty());
    let expect = Expect { reg: &reg };

    let modules: HashMap<&str, usize> = krate
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.as_str(), i))
        .collect();
    let mut problems = Vec::new();
    let (mut types, mut elements, mut attributes, mut simple, mut values, mut globals) = (0, 0, 0, 0, 0, 0);

    for (si, schema) in reg.schemas.iter().enumerate() {
        let module = &krate.modules[modules[module_name(&schema.file).as_str()]];

        // Complex types: exact attribute and child-element sets.
        for ct in &schema.complex_types {
            types += 1;
            let Some(def) = module.complex_types.iter().find(|c| c.name == ct.name) else {
                problems.push(format!("{}: complex type {} not generated", schema.file, ct.name));
                continue;
            };
            let mut want_attrs = BTreeSet::new();
            expect.attributes(si, ct, &mut want_attrs);
            let got_attrs: BTreeSet<Name> = def.attrs.iter().map(|a| (uri(&a.ns), a.local.clone())).collect();
            attributes += want_attrs.len();
            for missing in want_attrs.difference(&got_attrs) {
                problems.push(format!(
                    "{}: {} lacks attribute {missing:?}",
                    schema.file, ct.name
                ));
            }
            for extra in got_attrs.difference(&want_attrs) {
                problems.push(format!(
                    "{}: {} has unexpected attribute {extra:?}",
                    schema.file, ct.name
                ));
            }

            let mut want_elems = BTreeSet::new();
            expect.elements(si, ct, &mut want_elems);
            elements += want_elems.len();
            let got_elems: BTreeSet<Name> = match &def.content {
                ContentDef::Fields(fields) => fields
                    .iter()
                    .flat_map(|f| match &f.kind {
                        FieldKind::Element { elem, .. } => vec![(uri(&elem.ns), elem.local.clone())],
                        FieldKind::Choice { elems, .. } => {
                            elems.iter().map(|e| (uri(&e.ns), e.local.clone())).collect()
                        }
                        FieldKind::Any { .. } => vec![],
                    })
                    .collect(),
                ContentDef::Mixed => {
                    // Mixed content is kept as raw nodes; only wildcard content qualifies.
                    assert!(expect.is_mixed(ct), "{} is not mixed", ct.name);
                    want_elems.clone()
                }
                _ => BTreeSet::new(),
            };
            for missing in want_elems.difference(&got_elems) {
                problems.push(format!(
                    "{}: {} lacks child element {missing:?}",
                    schema.file, ct.name
                ));
            }
            for extra in got_elems.difference(&want_elems) {
                problems.push(format!(
                    "{}: {} has unexpected child element {extra:?}",
                    schema.file, ct.name
                ));
            }
        }

        // Simple types: every enumeration value.
        for st in &schema.simple_types {
            simple += 1;
            let Some(def) = module.simple_types.iter().find(|s| s.name == st.name) else {
                problems.push(format!("{}: simple type {} not generated", schema.file, st.name));
                continue;
            };
            if let SimpleBody::Restriction { enumerations, .. } = &st.body {
                if !enumerations.is_empty() {
                    values += enumerations.len();
                    match &def.kind {
                        SimpleKind::Enum(v) => {
                            let got: Vec<&str> = v.iter().map(|e| e.value.as_str()).collect();
                            let want: Vec<&str> = enumerations.iter().map(String::as_str).collect();
                            if got != want {
                                problems
                                    .push(format!("{}: {} values {got:?} != {want:?}", schema.file, st.name));
                            }
                        }
                        other => problems.push(format!(
                            "{}: {} is {other:?}, expected an enum",
                            schema.file, st.name
                        )),
                    }
                }
            }
        }

        // Global elements: every one with a complex type is a document root definition.
        for e in &schema.elements {
            let complex = match &e.type_name {
                Some(q) if q.ns == XS => false,
                Some(q) => reg.complex(&QName::new(&q.ns, &q.name)).is_some(),
                None => e.inline_complex.is_some(),
            };
            if complex {
                globals += 1;
                if !module.globals.iter().any(|g| g.local == e.name) {
                    problems.push(format!(
                        "{}: global element {} not generated",
                        schema.file, e.name
                    ));
                }
            }
        }
    }

    println!(
        "{types} complex types, {elements} child-element slots, {attributes} attributes, \
         {simple} simple types, {values} enumeration values, {globals} root elements checked"
    );
    assert!(types > 1400 && attributes > 3000 && values > 3000);
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
