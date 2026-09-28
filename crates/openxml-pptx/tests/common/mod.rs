//! Helpers shared by the integration tests.

#![allow(dead_code)]

use std::collections::HashSet;

use openxml_opc::{FALLBACK_CONTENT_TYPE, Package, PartName};
use openxml_pptx::Presentation;
use openxml_schema::pml;

/// Asserts that every XML part of the package validates against the ECMA-376 schemas.
pub fn assert_schema_valid(pkg: &Package) {
    let failures = openxml_testkit::validate_package(pkg);
    assert!(
        failures.is_empty(),
        "schema validation failed:\n{}",
        failures
            .iter()
            .map(|(p, e)| format!("--- {p}\n{e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Asserts package-level consistency: every internal relationship target
/// exists and every part has a real content type.
pub fn assert_package_consistent(pkg: &Package) {
    let mut sources: Vec<(Option<PartName>, &openxml_opc::Relationships)> =
        vec![(None, pkg.package_relationships())];
    for (name, part) in pkg.parts() {
        sources.push((Some(name.clone()), part.relationships()));
        assert_ne!(
            part.content_type(),
            FALLBACK_CONTENT_TYPE,
            "{name} has no content type"
        );
    }
    for (source, rels) in sources {
        let mut ids = HashSet::new();
        for r in rels.iter() {
            assert!(
                ids.insert(r.id.clone()),
                "duplicate relationship id {} in {source:?}",
                r.id
            );
            // External targets and same-part fragment links (`#slide3`, empty hyperlinks) are not parts.
            if r.is_external() || r.target.is_empty() || r.target.starts_with('#') {
                continue;
            }
            let target = PartName::resolve(source.as_ref(), &r.target)
                .unwrap_or_else(|e| panic!("{source:?} -> {}: {e}", r.target));
            assert!(
                pkg.contains(&target),
                "{source:?} -> {target} ({}) does not exist",
                r.rel_type
            );
        }
    }
}

/// Asserts the identifier rules PowerPoint relies on.
pub fn assert_ids_unique(deck: &Presentation) {
    let mut slide_ids = HashSet::new();
    for slide in deck.slides() {
        assert!(slide.id() >= 256, "slide id {} below 256", slide.id());
        assert!(slide_ids.insert(slide.id()), "duplicate slide id {}", slide.id());
        let mut shape_ids = HashSet::new();
        for info in slide.shapes() {
            for s in info.walk() {
                if s.id != 0 {
                    assert!(
                        shape_ids.insert(s.id),
                        "duplicate shape id {} on {}",
                        s.id,
                        slide.part_name()
                    );
                }
            }
        }
    }
    let pres = deck.presentation();
    let mut master_ids = HashSet::new();
    for m in pres.sld_master_id_lst.iter().flat_map(|l| &l.sld_master_id) {
        let id = m.id.expect("master id");
        assert!(id >= 2_147_483_648);
        assert!(master_ids.insert(id));
        let rid = m.r_id.as_deref().unwrap();
        let master = deck
            .package()
            .relationship_target(Some(deck.part_name()), rid)
            .unwrap();
        let data = pml::elements::SLD_MASTER
            .parse_bytes(deck.package().part(&master).unwrap().data())
            .unwrap();
        for l in data.sld_layout_id_lst.iter().flat_map(|l| &l.sld_layout_id) {
            let id = l.id.expect("layout id");
            assert!(id >= 2_147_483_648);
            assert!(master_ids.insert(id), "layout id {id} is not unique");
        }
    }
}

/// All checks on a saved presentation: reopen, schema validation, package
/// consistency and identifier rules. Returns the reopened presentation.
pub fn check_saved(bytes: &[u8]) -> Presentation {
    let deck = Presentation::from_bytes(bytes).expect("reopen");
    assert_package_consistent(deck.package());
    assert_schema_valid(deck.package());
    assert_ids_unique(&deck);
    deck
}
