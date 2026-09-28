//! Integration tests against packages produced by Office applications.

use std::path::PathBuf;

use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn opens_excel_workbook_from_ecma_distribution() {
    let pkg = Package::open_path(fixture("ecma/PivotTableFormats.xlsx")).unwrap();
    let workbook = pkg.main_part().expect("officeDocument relationship");
    assert_eq!(workbook, PartName::new("/xl/workbook.xml").unwrap());
    assert_eq!(pkg.part(&workbook).unwrap().content_type(), ct::SML_WORKBOOK);
    let sheets = pkg.related_parts(Some(&workbook), rel_types::WORKSHEET);
    assert!(!sheets.is_empty());
    for sheet in &sheets {
        assert_eq!(pkg.part(sheet).unwrap().content_type(), ct::SML_WORKSHEET);
    }
    assert!(pkg.related_part(Some(&workbook), rel_types::STYLES).is_some());
    let core = pkg.core_properties().unwrap();
    assert!(core.creator.is_some() || core.created.is_some());
}

#[test]
fn round_trip_preserves_all_parts_and_relationships() {
    let original = std::fs::read(fixture("ecma/PivotTableFormats.xlsx")).unwrap();
    let pkg = Package::from_bytes(&original).unwrap();
    let saved = pkg.to_bytes().unwrap();
    let back = Package::from_bytes(&saved).unwrap();
    assert_eq!(back.part_count(), pkg.part_count());
    for (name, part) in pkg.parts() {
        let other = back
            .part(name)
            .unwrap_or_else(|| panic!("{name} missing after round trip"));
        assert_eq!(other.content_type(), part.content_type(), "{name}");
        assert_eq!(other.data(), part.data(), "{name}");
        assert_eq!(other.relationships(), part.relationships(), "{name}");
    }
    assert_eq!(back.package_relationships(), pkg.package_relationships());
    assert_eq!(back, pkg);
}
