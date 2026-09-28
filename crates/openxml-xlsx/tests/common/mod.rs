//! Helpers shared by the integration tests.
#![allow(dead_code)]

use openxml_opc::Package;
use openxml_xlsx::Workbook;

/// Saves and reopens a workbook.
pub fn reopen(wb: &mut Workbook) -> Workbook {
    let bytes = wb.to_bytes().expect("save");
    Workbook::from_bytes(&bytes).expect("reopen")
}

/// Whether a validator message only reports `xml:space` on a SpreadsheetML
/// `<t>` element.
///
/// The ECMA-376 schema types `<t>` as a plain string, but Excel (like other
/// producers) writes `xml:space="preserve"` on text with leading or trailing
/// whitespace and would otherwise trim it. The crate follows Excel; this is
/// the only schema deviation the tests accept.
fn only_xml_space_on_t(message: &str) -> bool {
    let errors: Vec<&str> = message
        .lines()
        .filter(|l| l.contains("Schemas validity error"))
        .collect();
    !errors.is_empty()
        && errors.iter().all(|l| {
            l.contains("element t: Schemas validity error")
                && l.contains("attribute '{http://www.w3.org/XML/1998/namespace}space' is not allowed")
        })
}

/// Asserts that every XML part of the package validates against the ECMA-376 schemas.
pub fn assert_valid(pkg: &Package) {
    let failures: Vec<_> = openxml_testkit::validate_package(pkg)
        .into_iter()
        .filter(|(_, message)| !only_xml_space_on_t(message))
        .collect();
    assert!(failures.is_empty(), "schema validation failed:\n{failures:#?}");
}

/// Saves the workbook and validates every part of the saved package.
pub fn assert_saved_valid(wb: &mut Workbook) -> Vec<u8> {
    let bytes = wb.to_bytes().expect("save");
    let pkg = Package::from_bytes(&bytes).expect("package");
    assert_valid(&pkg);
    bytes
}

#[test]
fn the_accepted_deviation_is_narrow() {
    let t_error = "-:2: element t: Schemas validity error : Element '{urn:x}t', attribute '{http://www.w3.org/XML/1998/namespace}space': The attribute '{http://www.w3.org/XML/1998/namespace}space' is not allowed.\n- fails to validate\n";
    assert!(only_xml_space_on_t(t_error));
    let other = "-:3: element c: Schemas validity error : Element 'c': This element is not expected.\n";
    assert!(!only_xml_space_on_t(other));
    assert!(!only_xml_space_on_t(&format!("{t_error}{other}")));
    assert!(!only_xml_space_on_t(""));
}
