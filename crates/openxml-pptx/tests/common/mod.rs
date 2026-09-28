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

/// Asserts that the crate's own validator (`openxml_schema::validate_xml`)
/// reports no issue for the XML parts accepted by `filter`.
pub fn assert_rust_valid(pkg: &Package, filter: impl Fn(&PartName) -> bool) {
    let mut problems = Vec::new();
    for (name, part) in pkg.parts() {
        if !filter(name) || !part.content_type().ends_with("xml") {
            continue;
        }
        let xml = String::from_utf8_lossy(part.data());
        match openxml_schema::validate_xml(&xml) {
            None => {}
            Some(Err(e)) => problems.push(format!("{name}: {e}")),
            Some(Ok(issues)) => problems.extend(issues.iter().map(|i| format!("{name}: {i}"))),
        }
    }
    assert!(problems.is_empty(), "validator issues:\n{}", problems.join("\n"));
}

/// Asserts that every `cTn/@id` of every slide's timing tree is unique within the slide.
pub fn assert_time_node_ids_unique(deck: &Presentation) {
    for slide in deck.slides() {
        let Some(timing) = slide.raw().timing.as_deref() else {
            continue;
        };
        let raw = openxml_xml::RawElement::from_typed(timing, openxml_xml::Ns::P, "timing");
        let mut seen = HashSet::new();
        for e in raw.descendants() {
            if &*e.name.local == "cTn" {
                let id = e.attr(openxml_xml::Ns::NONE, "id").expect("cTn id");
                assert!(
                    seen.insert(id.to_owned()),
                    "duplicate cTn id {id} on {}",
                    slide.part_name()
                );
            }
        }
    }
}

/// Asserts that every shape targeted by the timing tree (`p:spTgt`, build
/// entries) and every connector end (`stCxn`/`endCxn`) exists on its slide.
pub fn assert_references_resolve(deck: &Presentation) {
    use openxml_xml::{Ns, RawElement};
    for slide in deck.slides() {
        let ids: HashSet<u32> = slide
            .shapes()
            .iter()
            .flat_map(|s| s.walk().into_iter().map(|w| w.id).collect::<Vec<_>>())
            .collect();
        let raw = RawElement::from_typed(slide.raw(), Ns::P, "sld");
        for e in raw.descendants() {
            let attr = match &*e.name.local {
                "spTgt" | "bldP" | "bldGraphic" => "spid",
                "stCxn" | "endCxn" => "id",
                _ => continue,
            };
            let id: u32 = e
                .attr(Ns::NONE, attr)
                .expect("target id")
                .parse()
                .expect("numeric id");
            assert!(
                ids.contains(&id),
                "{} refers to missing shape {id} on {}",
                e.name.local,
                slide.part_name()
            );
        }
    }
}

/// All checks on a saved presentation: reopen, XSD validation, the crate's
/// validator on every part, package consistency, identifier rules and
/// internal references. Returns the reopened presentation.
pub fn check_saved(bytes: &[u8]) -> Presentation {
    let deck = Presentation::from_bytes(bytes).expect("reopen");
    assert_package_consistent(deck.package());
    assert_schema_valid(deck.package());
    assert_rust_valid(deck.package(), |_| true);
    assert_ids_unique(&deck);
    assert_time_node_ids_unique(&deck);
    assert_references_resolve(&deck);
    deck
}

/// Saves, checks and reopens.
pub fn round_trip(deck: &mut Presentation) -> Presentation {
    let bytes = deck.to_bytes().expect("save");
    check_saved(&bytes)
}

/// Saves and reopens a presentation based on third-party files: package
/// consistency and identifier rules (the source parts may not be schema-valid).
pub fn round_trip_consistent(deck: &mut Presentation) -> Presentation {
    let bytes = deck.to_bytes().expect("save");
    let deck = Presentation::from_bytes(&bytes).expect("reopen");
    assert_package_consistent(deck.package());
    assert_ids_unique(&deck);
    assert_time_node_ids_unique(&deck);
    deck
}
