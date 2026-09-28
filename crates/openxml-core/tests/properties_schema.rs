//! Property parts written by openxml-core validate against the ECMA-376 schemas.

use openxml_core::properties::{CustomProperties, PropertyValue, write_extended_properties};
use openxml_opc::Package;
use openxml_schema::shared_extended_properties::CT_Properties;
use openxml_testkit::validate_package;

#[test]
fn custom_and_extended_properties_are_schema_valid() {
    let mut pkg = Package::new();
    let mut custom = CustomProperties::default();
    custom.set("Client", "Contoso");
    custom.set("Budget", 125_000.5);
    custom.set("Approved", true);
    custom.set("Version", 3);
    custom.set("Due", PropertyValue::DateTime("2025-01-31T00:00:00Z".into()));
    custom.write(&mut pkg).unwrap();
    write_extended_properties(
        &mut pkg,
        &CT_Properties {
            application: Some("openxml-rust".into()),
            pages: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    let failures = validate_package(&pkg);
    assert!(failures.is_empty(), "{failures:?}");
    for (name, part) in pkg.parts() {
        let xml = std::str::from_utf8(part.data()).unwrap();
        let issues = openxml_schema::validate_xml(xml).unwrap().unwrap();
        assert!(issues.is_empty(), "{name}: {issues:?}");
    }
}
