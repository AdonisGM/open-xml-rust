//! End-to-end checks through the facade: every format is created, saved,
//! validated against the ECMA-376 schemas and read back, and the files
//! produced by one API are readable by the lower layers.

use openxml::docx::Document;
use openxml::opc::Package;
use openxml::pptx::{LayoutKind, Presentation};
use openxml::xlsx::{CellValue, Workbook};
use openxml_testkit::validate_package;

#[test]
fn word_round_trip_and_validation() {
    let mut doc = Document::new();
    doc.add_heading("Title", 1).unwrap();
    doc.add_paragraph("Body text with ünïcödé and \"quotes\" & <brackets>.");
    let bytes = doc.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert!(validate_package(&pkg).is_empty(), "{:?}", validate_package(&pkg));
    let back = Document::from_bytes(&bytes).unwrap();
    assert_eq!(
        back.text(),
        "Title\nBody text with ünïcödé and \"quotes\" & <brackets>."
    );
}

#[test]
fn excel_round_trip_and_validation() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", "x").unwrap();
        s.set_value("B2", 2.5).unwrap();
        s.set_value("C3", true).unwrap();
        s.set_formula("D4", "B2*2").unwrap();
    }
    wb.add_worksheet("Second").unwrap();
    let bytes = wb.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert!(validate_package(&pkg).is_empty(), "{:?}", validate_package(&pkg));
    let back = Workbook::from_bytes(&bytes).unwrap();
    assert_eq!(back.sheet_names(), ["Sheet1", "Second"]);
    let s = back.worksheet("Sheet1").unwrap();
    assert_eq!(s.cell("A1").unwrap(), CellValue::Text("x".into()));
    assert_eq!(s.cell("B2").unwrap(), CellValue::Number(2.5));
    assert_eq!(s.cell("C3").unwrap(), CellValue::Bool(true));
    assert_eq!(s.cell("D4").unwrap().as_formula(), Some("B2*2"));
}

#[test]
fn powerpoint_round_trip_and_validation() {
    let mut deck = Presentation::new();
    {
        let mut s = deck.add_slide(LayoutKind::TitleAndContent).unwrap();
        s.set_title("Agenda").unwrap();
        s.set_body_text(&["One", "Two"]).unwrap();
    }
    let bytes = deck.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert!(validate_package(&pkg).is_empty(), "{:?}", validate_package(&pkg));
    let back = Presentation::from_bytes(&bytes).unwrap();
    assert_eq!(back.slide_count(), 1);
    assert_eq!(back.slide(0).unwrap().title().as_deref(), Some("Agenda"));
    assert!(back.slide(0).unwrap().text().contains("One\nTwo"));
}

#[test]
fn every_generated_part_passes_the_rust_validator_and_round_trips() {
    let mut doc = Document::new();
    doc.add_paragraph("x");
    let mut wb = Workbook::new();
    wb.worksheet_mut("Sheet1").unwrap().set_value("A1", 1).unwrap();
    let mut deck = Presentation::new();
    deck.add_slide(LayoutKind::Title).unwrap();
    for bytes in [
        doc.to_bytes().unwrap(),
        wb.to_bytes().unwrap(),
        deck.to_bytes().unwrap(),
    ] {
        let pkg = Package::from_bytes(&bytes).unwrap();
        for (name, part) in pkg.parts() {
            let Ok(xml) = openxml::xml::decode_xml_bytes(part.data()) else {
                continue;
            };
            if let Some(result) = openxml::schema::validate_xml(&xml) {
                let issues = result.unwrap();
                assert!(issues.is_empty(), "{name}: {issues:?}");
            }
            if let Some(out) = openxml::schema::round_trip_xml(&xml) {
                assert!(out.is_ok(), "{name}");
            }
        }
    }
}
