//! Helpers shared by the integration tests.

#![allow(dead_code)]

use std::path::PathBuf;

use openxml_docx::Document;
use openxml_opc::{Package, PartName};
use openxml_testkit::validate_package;
use openxml_xml::decode_xml_bytes;

/// Checks every XML part of a package: against the ECMA-376 schemas with
/// `xmllint`, and with the crate's own schema validator.
pub fn validate(pkg: &Package) {
    let failures = validate_package(pkg);
    assert!(failures.is_empty(), "schema validation failed: {failures:#?}");
    for (name, part) in pkg.parts() {
        let Ok(text) = decode_xml_bytes(part.data()) else {
            continue;
        };
        if let Some(result) = openxml_schema::validate_xml(&text) {
            let issues = result.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(issues.is_empty(), "{name}: {issues:#?}");
        }
    }
}

/// Saves, validates every part and reopens.
pub fn save_validate_reopen(doc: &mut Document) -> Document {
    let bytes = doc.to_bytes().expect("save");
    let pkg = Package::from_bytes(&bytes).expect("valid package");
    validate(&pkg);
    Document::from_bytes(&bytes).expect("reopen")
}

/// Saves and reopens without validation (for fixtures that were invalid
/// before being edited).
pub fn save_reopen(doc: &mut Document) -> Document {
    Document::from_bytes(&doc.to_bytes().expect("save")).expect("reopen")
}

/// Part name.
pub fn pn(s: &str) -> PartName {
    PartName::new(s).unwrap()
}

/// Path of a POI fixture.
pub fn fixture(name: &str) -> PathBuf {
    openxml_testkit::fixture(&format!("poi/{name}"))
}

/// Opens a POI fixture.
pub fn open(name: &str) -> Document {
    Document::open(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// XML text of a part.
pub fn part_xml(pkg: &Package, name: &str) -> String {
    decode_xml_bytes(
        pkg.part(&pn(name))
            .unwrap_or_else(|| panic!("{name} missing"))
            .data(),
    )
    .unwrap()
    .into_owned()
}

/// Asserts that every part except `changed` is byte-identical in `after`.
pub fn assert_untouched_except(before: &Package, after: &Package, changed: &[&str]) {
    for (name, part) in before.parts() {
        if changed.contains(&name.as_str()) {
            continue;
        }
        let other = after
            .part(name)
            .unwrap_or_else(|| panic!("{name} missing after the edit"));
        assert_eq!(other.data(), part.data(), "{name} must not change");
    }
}
