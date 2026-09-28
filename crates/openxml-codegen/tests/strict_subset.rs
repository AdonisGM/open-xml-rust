//! The object model is generated from the Transitional schemas and Strict
//! documents are read into it. This test checks the premise: every type,
//! element and attribute defined by the Strict schemas has a counterpart in
//! the Transitional schemas (after mapping Strict namespaces).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use openxml_codegen::load_schemas;
use openxml_codegen::xsd::{AttrItem, Content, ElementRef, Particle, Schema};
use openxml_xml::Ns;

fn canonical(uri: &str) -> String {
    Ns::from_uri(uri)
        .map(|n| n.uri().to_owned())
        .unwrap_or_else(|| uri.to_owned())
}

fn particle_elements(p: &Particle, out: &mut BTreeSet<String>) {
    match p {
        Particle::Element { decl, .. } => {
            out.insert(match decl {
                ElementRef::Local(e) => e.name.clone(),
                ElementRef::Ref(q) => q.name.clone(),
            });
        }
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } | Particle::All { items, .. } => {
            for i in items {
                particle_elements(i, out);
            }
        }
        Particle::Any { .. } | Particle::Group { .. } => {}
    }
}

struct Summary {
    types: HashMap<(String, String), (BTreeSet<String>, BTreeSet<String>)>,
    elements: BTreeSet<(String, String)>,
    simple: BTreeSet<(String, String)>,
}

fn summarize(schemas: &[Schema]) -> Summary {
    let mut s = Summary {
        types: HashMap::new(),
        elements: BTreeSet::new(),
        simple: BTreeSet::new(),
    };
    for schema in schemas {
        let ns = canonical(&schema.target_ns);
        for ct in &schema.complex_types {
            let mut elems = BTreeSet::new();
            let mut attrs = BTreeSet::new();
            match &ct.content {
                Content::Particle(p) => particle_elements(p, &mut elems),
                Content::Extension {
                    particle: Some(p), ..
                }
                | Content::Restriction {
                    particle: Some(p), ..
                } => particle_elements(p, &mut elems),
                _ => {}
            }
            for a in &ct.attributes {
                match a {
                    AttrItem::Local(a) => {
                        attrs.insert(a.name.clone());
                    }
                    AttrItem::Ref { name, .. } => {
                        attrs.insert(name.name.clone());
                    }
                    _ => {}
                }
            }
            s.types.insert((ns.clone(), ct.name.clone()), (elems, attrs));
        }
        for g in &schema.groups {
            let mut elems = BTreeSet::new();
            if let Some(p) = &g.particle {
                particle_elements(p, &mut elems);
            }
            s.types
                .insert((ns.clone(), g.name.clone()), (elems, BTreeSet::new()));
        }
        for e in &schema.elements {
            s.elements.insert((ns.clone(), e.name.clone()));
        }
        for t in &schema.simple_types {
            s.simple.insert((ns.clone(), t.name.clone()));
        }
    }
    s
}

#[test]
fn strict_is_a_subset_of_transitional() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let strict = summarize(&load_schemas(&root.join("strict")).unwrap());
    let transitional = summarize(&load_schemas(&root.join("transitional")).unwrap());

    let mut problems = Vec::new();
    for (key, (elems, attrs)) in &strict.types {
        match transitional.types.get(key) {
            None => problems.push(format!("type/group {key:?} missing")),
            Some((t_elems, t_attrs)) => {
                for e in elems.difference(t_elems) {
                    problems.push(format!("{key:?}: element {e} missing"));
                }
                for a in attrs.difference(t_attrs) {
                    problems.push(format!("{key:?}: attribute {a} missing"));
                }
            }
        }
    }
    for e in strict.elements.difference(&transitional.elements) {
        problems.push(format!("global element {e:?} missing"));
    }
    for t in strict.simple.difference(&transitional.simple) {
        problems.push(format!("simple type {t:?} missing"));
    }
    assert!(
        strict.types.len() > 1000,
        "sanity: {} strict types",
        strict.types.len()
    );
    assert!(
        problems.is_empty(),
        "{} differences:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
