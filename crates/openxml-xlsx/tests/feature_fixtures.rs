//! Reading (and editing) the worksheet features of files written by Excel
//! and other producers: `fixtures/poi` and `tests/data` (Apache POI test
//! files, see `tests/data/NOTICE.md`).

mod common;

use std::path::PathBuf;

use common::assert_saved_valid;
use openxml_xlsx::*;

fn poi(name: &str) -> Workbook {
    Workbook::open(openxml_testkit::fixture(&format!("poi/{name}"))).unwrap()
}

fn data(name: &str) -> Workbook {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    Workbook::open(path).unwrap()
}

#[test]
fn comments_from_excel() {
    let wb = poi("comments.xlsx");
    let s = wb.worksheet("Sheet1").unwrap();
    let comments = s.comments().unwrap();
    assert_eq!(
        comments.iter().map(|c| c.cell.to_string()).collect::<Vec<_>>(),
        ["A1", "A3", "A4"]
    );
    assert!(comments.iter().all(|c| c.author == "Sven Nissel" && !c.visible));
    assert_eq!(comments[0].text.text(), "comment top row1 (index0)\n");
    let first_run = &comments[0].text.runs()[0];
    let font = first_run.font.as_ref().unwrap();
    assert!(font.bold);
    assert_eq!((font.size, font.name.as_deref()), (Some(8.0), Some("Tahoma")));
    assert_eq!(font.color, Some(Color::Indexed(81)));
    assert!(s.comment("A2").unwrap().is_none());

    let wb = poi("FormulaEvalTestData_Copy.xlsx");
    let s = wb.worksheet("EverythingTests").unwrap();
    let c = s.comment("B420").unwrap().unwrap();
    assert_eq!(c.author, "Amol");
    assert!(
        c.text
            .text()
            .starts_with("Amol:\nDollar impl in FormulaEvaluator")
    );
    let link = s.hyperlink("D725").unwrap().unwrap();
    assert_eq!(link.target, LinkTarget::Url("http://poi.apache.org/".into()));
}

#[test]
fn pictures_and_charts_from_excel() {
    let wb = poi("WithDrawing.xlsx");
    let s = wb.worksheet("Sheet1").unwrap();
    let images = s.images().unwrap();
    assert_eq!(
        images
            .iter()
            .map(|i| i.description.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["clock.jpg", "cow.pict", "tomcat.png", "wrench.emf", "santa.wmf"],
        "the text box is not a picture"
    );
    let Anchor::TwoCell { from, to, edit_as } = images[0].anchor else {
        panic!("{:?}", images[0].anchor)
    };
    assert_eq!(
        (from.cell.to_string(), to.cell.to_string()),
        ("A1".into(), "D9".into())
    );
    assert_eq!(to.dy, Length::emu(152_400));
    assert_eq!(edit_as, EditAs::OneCell);
    assert!(matches!(
        images.iter().find(|i| i.id == 7).unwrap().anchor,
        Anchor::TwoCell { .. }
    ));
    assert_eq!(images[0].name, "Picture 1");
    assert_eq!(images[0].content_type.as_deref(), Some("image/jpeg"));
    assert!(s.image_data(&images[0]).unwrap().starts_with(&[0xFF, 0xD8]));
    let raw = s.drawing().unwrap().unwrap();
    assert_eq!(raw.anchor.len(), 6);

    let wb = poi("WithChart.xlsx");
    let with_drawing: Vec<String> = wb
        .worksheet_names()
        .into_iter()
        .filter(|n| wb.worksheet(n).unwrap().drawing_part().is_some())
        .collect();
    assert_eq!(with_drawing.len(), 1);
    let s = wb.worksheet(&with_drawing[0]).unwrap();
    assert!(s.images().unwrap().is_empty(), "a chart is not a picture");
    let drawing = s.drawing().unwrap().unwrap();
    assert_eq!(drawing.anchor.len(), 1);
    let openxml_schema::dml_spreadsheet_drawing::EG_Anchor::TwoCellAnchor(a) = &drawing.anchor[0] else {
        panic!("two-cell anchor")
    };
    assert!(matches!(
        a.choice,
        Some(openxml_schema::dml_spreadsheet_drawing::CT_TwoCellAnchor_Choice::GraphicFrame(_))
    ));

    let wb = data("absolute-anchor-over-empty-sheet.xlsx");
    let images = wb.worksheet("picture").unwrap().images().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].name, "VBA_LogoBNEF");
    assert_eq!(
        images[0].anchor,
        Anchor::Absolute {
            x: Length::emu(4_463_143),
            y: Length::emu(6_599_464),
            width: Length::emu(10_084_254),
            height: Length::emu(6_762_750),
        }
    );
}

#[test]
fn pictures_are_added_next_to_existing_ones() {
    let mut wb = poi("WithDrawing.xlsx");
    let id = wb
        .worksheet_mut("Sheet1")
        .unwrap()
        .add_image("K20", &openxml_core::image::tiny_png(30, 30))
        .unwrap();
    assert_eq!(id, 10, "after the largest existing object id (9)");
    let bytes = assert_saved_valid(&mut wb);
    let wb = Workbook::from_bytes(&bytes).unwrap();
    assert_eq!(wb.worksheet("Sheet1").unwrap().images().unwrap().len(), 6);
}

#[test]
fn tables_from_excel() {
    let wb = poi("WithTable.xlsx");
    let s = wb.worksheet("Foglio1").unwrap();
    let tables = s.tables().unwrap();
    assert_eq!(tables.len(), 1);
    let t = &tables[0];
    assert_eq!(
        (t.id, t.name.as_str(), t.range.to_string()),
        (1, "Tabella1", "A1:B2".to_owned())
    );
    assert_eq!(
        t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(t.style.as_deref(), Some("TableStyleMedium9"));
    assert_eq!(t.auto_filter.unwrap().to_string(), "A1:B2");
    assert!(t.header_row && !t.totals_row);

    // A new table gets the next id and name.
    let mut wb = poi("WithTable.xlsx");
    {
        let mut s = wb.worksheet_mut("Foglio2").unwrap();
        s.set_value("A1", "x").unwrap();
        let t = s.add_table("A1:A3", &Table::new()).unwrap();
        assert_eq!((t.id, t.name.as_str()), (2, "Table2"));
    }
    assert_saved_valid(&mut wb);
}

#[test]
fn data_validations_from_excel() {
    let mut wb = poi("DataValidations-49244.xlsx");
    {
        let s = wb.worksheet("Sheet1").unwrap();
        let dvs = s.data_validations();
        assert_eq!(dvs.len(), 52);
        let (ranges, first) = &dvs[0];
        assert_eq!(ranges, &[CellRange::parse("C6").unwrap()]);
        assert_eq!(
            first.rule,
            ValidationRule::WholeNumber(Comparison::GreaterThan("E6".into()))
        );
        assert_eq!(first.input_title.as_deref(), Some("Integer Input"));
        assert!(first.allow_blank && first.show_input_message && first.show_error_message);
        assert_eq!(dvs[1].0, [CellRange::parse("C8:C9").unwrap()]);
        assert_eq!(
            dvs[5].1.rule,
            ValidationRule::WholeNumber(Comparison::Between("E14".into(), "F14".into())),
            "no operator means between"
        );
        let kinds = |f: fn(&ValidationRule) -> bool| dvs.iter().filter(|(_, d)| f(&d.rule)).count();
        assert!(kinds(|r| matches!(r, ValidationRule::Decimal(_))) >= 8);
        assert!(kinds(|r| matches!(r, ValidationRule::TextLength(_))) >= 1);
    }
    wb.worksheet_mut("Sheet1")
        .unwrap()
        .add_data_validation("Z1", &DataValidation::list(["x"]).unwrap())
        .unwrap();
    let bytes = assert_saved_valid(&mut wb);
    let wb = Workbook::from_bytes(&bytes).unwrap();
    assert_eq!(wb.worksheet("Sheet1").unwrap().data_validations().len(), 53);
}

#[test]
fn conditional_formats_from_excel() {
    let wb = data("NewStyleConditionalFormattings.xlsx");
    let name = wb.worksheet_names()[0].clone();
    let s = wb.worksheet(&name).unwrap();
    let cfs = s.conditional_formats();
    let (ranges, first) = &cfs[0];
    assert_eq!(ranges, &[CellRange::parse("C2:C17").unwrap()]);
    assert_eq!(
        first.rule,
        CfRule::CellIs {
            operator: CfOperator::GreaterThan,
            formulas: vec!["0".into()]
        }
    );
    assert_eq!(first.priority, Some(23));
    let style = first.style.as_ref().unwrap();
    assert_eq!(style.font.as_ref().unwrap().color, Some(Color::Rgb(0, 0x61, 0)));
    assert_eq!(style.fill, Some(Fill::solid(Color::Rgb(0xC6, 0xEF, 0xCE))));
    assert_eq!(
        cfs[1].1.rule,
        CfRule::CellIs {
            operator: CfOperator::Between,
            formulas: vec!["10".into(), "30".into()]
        }
    );
    assert_eq!(
        cfs[2].1.rule,
        CfRule::DataBar {
            min: CfValue::Min,
            max: CfValue::Max,
            color: Color::Rgb(0x63, 0xC3, 0x84),
            show_value: true
        }
    );
    let CfRule::ColorScale(stops) = &cfs[3].1.rule else {
        panic!("{:?}", cfs[3].1.rule)
    };
    assert_eq!(
        stops.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>(),
        [CfValue::Min, CfValue::Percentile(50.0), CfValue::Max]
    );
    assert!(matches!(
        cfs[5].1.rule,
        CfRule::IconSet {
            icons: IconSetType::V3TrafficLights1,
            ..
        }
    ));

    let wb = data("WithConditionalFormatting.xlsx");
    let s = wb.worksheet("CF").unwrap();
    let cfs = s.conditional_formats();
    assert_eq!(cfs.len(), 5);
    assert!(cfs[0].1.stop_if_true);
    assert_eq!(cfs[2].1.rule, CfRule::Expression("$A$8>5".into()));
    assert_eq!(
        cfs[4].1.rule,
        CfRule::CellIs {
            operator: CfOperator::Between,
            formulas: vec!["\"A\"".into(), "\"AAA\"".into()]
        }
    );
}

#[test]
fn conditional_formats_are_added_to_excel_files() {
    let mut wb = data("NewStyleConditionalFormattings.xlsx");
    let name = wb.worksheet_names()[0].clone();
    let before = wb.stylesheet().dxfs.as_ref().unwrap().dxf.len();
    let priority = wb
        .worksheet_mut(&name)
        .unwrap()
        .add_conditional_format(
            "Z1:Z9",
            &ConditionalFormat::duplicates().style(CellStyle::new().italic()),
        )
        .unwrap();
    assert_eq!(priority, 24, "after the highest existing priority");
    assert_eq!(wb.stylesheet().dxfs.as_ref().unwrap().dxf.len(), before + 1);
    let bytes = wb.to_bytes().unwrap();
    let wb = Workbook::from_bytes(&bytes).unwrap();
    let s = wb.worksheet(&name).unwrap();
    let last = s.conditional_formats().pop().unwrap().1;
    assert_eq!(last.rule, CfRule::DuplicateValues);
    assert!(last.style.unwrap().font.unwrap().italic);
    assert_eq!(
        s.conditional_formats()[0].1.style.as_ref().unwrap().fill,
        Some(Fill::solid(Color::Rgb(0xC6, 0xEF, 0xCE))),
        "existing dxf ids still resolve"
    );
}

#[test]
fn protection_from_excel() {
    let wb = data("workbookProtection-sheet_password-2013.xlsx");
    let s = wb.worksheet("Sheet1").unwrap();
    assert!(s.sheet_has_password());
    assert!(s.verify_sheet_password("pwd"));
    assert!(!s.verify_sheet_password("pwd2"));
    let p = s.protection().unwrap();
    assert!(!p.edit_objects && !p.edit_scenarios && p.select_locked_cells);

    let wb = data("sheetProtection_allLocked.xlsx");
    let p = wb.worksheet("Foglio1").unwrap().protection().unwrap();
    assert!(!p.select_locked_cells && !p.select_unlocked_cells && !p.format_cells);
    assert!(wb.worksheet("Foglio2").unwrap().protection().is_none());
    assert!(!wb.worksheet("Foglio1").unwrap().verify_sheet_password("x"));
    assert!(
        wb.worksheet("Foglio1").unwrap().verify_sheet_password(""),
        "no password set"
    );
}

#[test]
fn views_links_breaks_and_outlines_from_excel() {
    let wb = data("SheetTabColors.xlsx");
    assert_eq!(wb.worksheet("default").unwrap().tab_color(), None);
    assert_eq!(
        wb.worksheet("indexedRed").unwrap().tab_color(),
        Some(Color::Indexed(10))
    );
    assert_eq!(
        wb.worksheet("customOrange").unwrap().tab_color(),
        Some(Color::Rgb(0x7F, 0x27, 0))
    );

    let wb = data("53282.xlsx");
    let links = wb.worksheet("Sheet1").unwrap().hyperlinks();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].range.to_string(), "O1");
    assert_eq!(
        links[0].target,
        LinkTarget::Url("mailto:nobody@nowhere.uk\u{a0}".into())
    );
    assert_eq!(
        links[1].target,
        LinkTarget::Url("mailto:nobody@nowhere.com".into())
    );

    let wb = data("49156.xlsx");
    let s = wb.worksheet("1").unwrap();
    assert_eq!(s.row_breaks(), [51]);
    assert_eq!(s.tab_color(), Some(Color::Indexed(12)));

    let wb = data("56502.xlsx");
    assert_eq!(wb.worksheet("Tabelle1").unwrap().outline_summary(), (false, true));
}

#[test]
fn structural_edits_on_excel_files_drop_the_calc_chain() {
    let mut wb = data("49156.xlsx");
    let chain = wb
        .package()
        .parts()
        .find(|(n, _)| n.as_str().ends_with("calcChain.xml"))
        .map(|(n, _)| n.clone());
    assert!(chain.is_some());
    wb.insert_rows("1", 10, 5).unwrap();
    assert_eq!(wb.worksheet("1").unwrap().row_breaks(), [56]);
    let bytes = assert_saved_valid(&mut wb);
    let wb = Workbook::from_bytes(&bytes).unwrap();
    assert!(
        !wb.package().contains(&chain.unwrap()),
        "Excel rebuilds the chain"
    );
}
