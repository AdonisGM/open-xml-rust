//! Creating workbooks: every feature is written, saved, reopened and checked,
//! and every generated part is validated against the ECMA-376 schemas.

mod common;

use common::{assert_saved_valid, assert_valid, reopen};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::sml;
use openxml_xlsx::{
    Border, BorderStyle, CellRange, CellRef, CellStyle, CellValue, Color, DateSystem, DateTime, Error,
    HorizontalAlignment, NumberFormat, SheetKind, StyleId, VerticalAlignment, Workbook,
};

fn pn(s: &str) -> PartName {
    PartName::new(s).unwrap()
}

#[test]
fn new_workbook_has_the_parts_excel_needs_and_is_valid() {
    let mut wb = Workbook::new();
    assert_eq!(wb.sheet_names(), ["Sheet1"]);
    assert_eq!(wb.worksheet_names(), ["Sheet1"]);
    assert_eq!(wb.sheet_kind("Sheet1"), Some(SheetKind::Worksheet));
    let bytes = assert_saved_valid(&mut wb);
    let pkg = Package::from_bytes(&bytes).unwrap();
    let main = pkg.main_part().unwrap();
    assert_eq!(main, pn("/xl/workbook.xml"));
    assert_eq!(pkg.part(&main).unwrap().content_type(), ct::SML_WORKBOOK);
    let sheets = pkg.related_parts(Some(&main), rel_types::WORKSHEET);
    assert_eq!(sheets, vec![pn("/xl/worksheets/sheet1.xml")]);
    assert_eq!(pkg.part(&sheets[0]).unwrap().content_type(), ct::SML_WORKSHEET);
    let styles = pkg.related_part(Some(&main), rel_types::STYLES).unwrap();
    assert_eq!(pkg.part(&styles).unwrap().content_type(), ct::SML_STYLES);
    assert!(
        pkg.related_part(Some(&main), rel_types::SHARED_STRINGS).is_none(),
        "no strings, no table"
    );
    assert!(pkg.related_part(None, rel_types::EXTENDED_PROPERTIES).is_some());
    let core = pkg.core_properties().unwrap();
    assert_eq!(core.creator.as_deref(), Some("openxml-rust"));
    assert!(core.created.is_some());

    let workbook = sml::elements::WORKBOOK
        .parse_bytes(pkg.part(&main).unwrap().data())
        .unwrap();
    let sheet = &workbook.sheets.as_ref().unwrap().sheet[0];
    assert_eq!((sheet.name.as_deref(), sheet.sheet_id), (Some("Sheet1"), Some(1)));
    assert!(workbook.book_views.is_some());
    let stylesheet = sml::elements::STYLE_SHEET
        .parse_bytes(pkg.part(&styles).unwrap().data())
        .unwrap();
    assert_eq!(stylesheet.fills.as_ref().unwrap().fill.len(), 2);
    assert_eq!(stylesheet.cell_xfs.as_ref().unwrap().count, Some(1));
    let ws = sml::elements::WORKSHEET
        .parse_bytes(pkg.part(&sheets[0]).unwrap().data())
        .unwrap();
    assert_eq!(
        ws.sheet_views.as_ref().unwrap().sheet_view[0].tab_selected,
        Some(true)
    );
}

#[test]
fn every_value_kind_round_trips() {
    let mut wb = Workbook::new();
    let date = DateTime::from_ymd(2024, 2, 29).unwrap();
    let stamp = DateTime::from_ymd_hms_milli(2023, 12, 31, 23, 59, 58, 500).unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", 42).unwrap();
        s.set_value("B1", -0.125).unwrap();
        s.set_value("C1", 1e-10).unwrap();
        s.set_value("D1", 6.02e23).unwrap();
        s.set_value("A2", "plain").unwrap();
        s.set_value("B2", "  padded  ").unwrap();
        s.set_value("C2", "Tiếng Việt · 日本語 · emoji 😀").unwrap();
        s.set_value("D2", "tab\tline\nbreak\rcr\u{1}ctl").unwrap();
        s.set_value("E2", "_x0041_ literal").unwrap();
        s.set_value("F2", "<xml> & \"quotes\"").unwrap();
        s.set_value("A3", true).unwrap();
        s.set_value("B3", false).unwrap();
        s.set_value("C3", CellValue::Error("#DIV/0!".into())).unwrap();
        s.set_value("A4", date).unwrap();
        s.set_value("B4", stamp).unwrap();
        s.set_value("A5", CellValue::formula("=SUM(A1:D1)")).unwrap();
        s.set_value("B5", CellValue::formula_with_result("A1*2", 84))
            .unwrap();
        s.set_value("C5", CellValue::formula_with_result("A2&\"!\"", "plain!"))
            .unwrap();
        s.set_value("D5", CellValue::formula_with_result("A1>1", true))
            .unwrap();
        s.set_value(
            "E5",
            CellValue::formula_with_result("1/0", CellValue::Error("#DIV/0!".into())),
        )
        .unwrap();
        s.set_value("F5", CellValue::formula_with_result("DATE(2024,2,29)", date))
            .unwrap();
        s.set_value((7, 3), "tuple address").unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    let v = |r: &str| s.cell(r).unwrap();
    assert_eq!(v("A1"), CellValue::Number(42.0));
    assert_eq!(v("B1"), CellValue::Number(-0.125));
    assert_eq!(v("C1"), CellValue::Number(1e-10));
    assert_eq!(v("D1"), CellValue::Number(6.02e23));
    assert_eq!(v("A2"), CellValue::Text("plain".into()));
    assert_eq!(v("B2"), CellValue::Text("  padded  ".into()));
    assert_eq!(v("C2"), CellValue::Text("Tiếng Việt · 日本語 · emoji 😀".into()));
    assert_eq!(v("D2"), CellValue::Text("tab\tline\nbreak\rcr\u{1}ctl".into()));
    assert_eq!(v("E2"), CellValue::Text("_x0041_ literal".into()));
    assert_eq!(v("F2"), CellValue::Text("<xml> & \"quotes\"".into()));
    assert_eq!(v("A3"), CellValue::Bool(true));
    assert_eq!(v("B3"), CellValue::Bool(false));
    assert_eq!(v("C3"), CellValue::Error("#DIV/0!".into()));
    assert_eq!(v("A4"), CellValue::DateTime(date));
    assert_eq!(v("B4"), CellValue::DateTime(stamp));
    assert_eq!(v("A5"), CellValue::formula("SUM(A1:D1)"));
    assert_eq!(v("B5"), CellValue::formula_with_result("A1*2", 84));
    assert_eq!(v("C5"), CellValue::formula_with_result("A2&\"!\"", "plain!"));
    assert_eq!(v("D5"), CellValue::formula_with_result("A1>1", true));
    assert_eq!(v("E5").result(), &CellValue::Error("#DIV/0!".into()));
    assert_eq!(v("F5").as_formula(), Some("DATE(2024,2,29)"));
    assert_eq!(v("C7").as_str(), Some("tuple address"));
    assert_eq!(v("Z99"), CellValue::Empty);
    // Date cells received date formats automatically.
    let date_style = s.cell_style("A4").unwrap().unwrap();
    let stamp_style = s.cell_style("B4").unwrap().unwrap();
    assert_eq!(back.number_format_of(date_style).0, 14);
    assert_eq!(back.number_format_of(stamp_style).0, 22);
    // A formula without a cached value asks Excel to recalculate on open.
    assert_eq!(
        back.raw_workbook().calc_pr.as_ref().unwrap().full_calc_on_load,
        Some(true)
    );
    assert_eq!(s.dimension().unwrap().to_string(), "A1:F7");
}

#[test]
fn shared_strings_are_deduplicated() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for r in 1..=50 {
            s.set_value((r, 1), "same").unwrap();
            s.set_value((r, 2), format!("row {}", r % 5)).unwrap();
        }
    }
    let bytes = assert_saved_valid(&mut wb);
    let pkg = Package::from_bytes(&bytes).unwrap();
    let sst_part = pkg
        .related_part(Some(&pn("/xl/workbook.xml")), rel_types::SHARED_STRINGS)
        .unwrap();
    assert_eq!(sst_part, pn("/xl/sharedStrings.xml"));
    let sst = sml::elements::SST
        .parse_bytes(pkg.part(&sst_part).unwrap().data())
        .unwrap();
    assert_eq!(sst.unique_count, Some(6));
    assert_eq!(sst.count, Some(100));
    let back = Workbook::from_bytes(&bytes).unwrap();
    assert_eq!(back.shared_string_count(), 6);
    assert_eq!(
        back.worksheet("Sheet1").unwrap().cell("B7").unwrap().as_str(),
        Some("row 2")
    );
}

#[test]
fn overwriting_and_clearing_cells() {
    let mut wb = Workbook::new();
    let bold = wb.add_style(&CellStyle::new().bold());
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("B2", "text").unwrap();
        s.set_value("B2", 5).unwrap();
        assert_eq!(s.cell("B2").unwrap(), CellValue::Number(5.0));
        s.set_value("C2", CellValue::formula("B2+1")).unwrap();
        s.set_value("C2", false).unwrap();
        assert_eq!(
            s.cell("C2").unwrap(),
            CellValue::Bool(false),
            "the formula is gone"
        );
        s.set_value("D2", 1).unwrap();
        s.set_cell_style("D2", bold).unwrap();
        s.clear("D2").unwrap();
        assert_eq!(
            s.as_view().cell_style("D2").unwrap(),
            Some(bold),
            "styles survive clearing"
        );
        s.set_value("E9", 1).unwrap();
        s.set_value("E9", CellValue::Empty).unwrap();
        s.clear("Q100").unwrap();
    }
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    assert_eq!(s.cell("B2").unwrap(), CellValue::Number(5.0));
    assert_eq!(s.cell("D2").unwrap(), CellValue::Empty);
    assert_eq!(
        s.rows().map(|r| r.index()).collect::<Vec<_>>(),
        [2],
        "row 9 disappeared with its only cell"
    );
    assert_eq!(s.used_range().unwrap().to_string(), "B2:D2");
}

#[test]
fn cells_and_rows_are_kept_in_sheet_order() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for r in ["C3", "A1", "B3", "AA2", "A3", "B1", "Z2"] {
            s.set_value(r, r).unwrap();
        }
    }
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    let order: Vec<String> = s
        .rows()
        .flat_map(|r| r.cells().map(|(c, _)| c.to_string()).collect::<Vec<_>>())
        .collect();
    assert_eq!(order, ["A1", "B1", "Z2", "AA2", "A3", "B3", "C3"]);
    let raw_refs: Vec<String> = s
        .raw()
        .sheet_data
        .as_ref()
        .unwrap()
        .row
        .iter()
        .flat_map(|r| r.c.iter().map(|c| c.r.clone().unwrap()))
        .collect();
    assert_eq!(raw_refs, order, "the XML itself is ordered");
}

#[test]
fn invalid_values_and_references_are_rejected() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        assert!(matches!(s.set_value("A0", 1), Err(Error::InvalidArgument(_))));
        assert!(s.set_value("XFE1", 1).is_err());
        assert!(s.set_value("A1", f64::NAN).is_err());
        assert!(s.set_value("A1", f64::INFINITY).is_err());
        assert!(s.set_value("A1", "x".repeat(32_768)).is_err());
        assert!(s.set_value("A1", CellValue::Error("bad".into())).is_err());
        assert!(s.set_formula("A1", "=").is_err());
        assert!(s.set_cell_style("A1", StyleId(99)).is_err());
        assert!(s.set_value("A1", "x".repeat(32_767)).is_ok());
    }
    wb.set_date_system(DateSystem::V1904);
    let mut s = wb.worksheet_mut("Sheet1").unwrap();
    assert!(
        s.set_value("B1", DateTime::from_ymd(1903, 1, 1).unwrap())
            .is_err(),
        "before the 1904 epoch"
    );
}

#[test]
fn styles_are_written_deduplicated_and_read_back() {
    let mut wb = Workbook::new();
    let header = CellStyle::new()
        .bold()
        .font_size(14.0)
        .font_color(Color::Rgb(255, 255, 255))
        .fill_color(Color::Rgb(0x44, 0x72, 0xC4))
        .border(Border::all(BorderStyle::Thin, Some(Color::Rgb(0, 0, 0))))
        .horizontal(HorizontalAlignment::Center)
        .vertical(VerticalAlignment::Center)
        .wrap_text();
    let money = CellStyle::new().number_format(NumberFormat::custom("#,##0.00 [$₫-42A]"));
    let pct = CellStyle::new()
        .number_format(NumberFormat::PERCENT_DECIMAL_2)
        .italic();
    let h1 = wb.add_style(&header);
    let h2 = wb.add_style(&header);
    assert_eq!(h1, h2);
    let m = wb.add_style(&money);
    let p = wb.add_style(&pct);
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", "Amount").unwrap();
        s.set_cell_style("A1", h1).unwrap();
        s.set_value("A2", 1234.5).unwrap();
        s.set_cell_style("A2", m).unwrap();
        s.set_value("A3", 0.1234).unwrap();
        s.set_cell_style("A3", p).unwrap();
        s.set_range_style(CellRange::parse("C1:D2").unwrap(), h1).unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    let style_of = |r: &str| back.cell_style(s.cell_style(r).unwrap().unwrap()).unwrap();
    assert_eq!(style_of("A1"), header);
    assert_eq!(style_of("A2"), money);
    assert_eq!(style_of("A3"), pct);
    assert_eq!(style_of("D2"), header, "range styles create empty styled cells");
    assert_eq!(
        s.cell("A2").unwrap(),
        CellValue::Number(1234.5),
        "custom non-date formats keep numbers"
    );
    let sheet = back.stylesheet();
    assert_eq!(sheet.num_fmts.as_ref().unwrap().num_fmt.len(), 1);
    assert_eq!(sheet.num_fmts.as_ref().unwrap().num_fmt[0].num_fmt_id, Some(164));
}

#[test]
fn custom_date_formats_are_detected() {
    let mut wb = Workbook::new();
    let iso = wb.add_style(&CellStyle::new().number_format(NumberFormat::custom("yyyy-mm-dd hh:mm")));
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_cell_style("A1", iso).unwrap();
        s.set_value("A1", DateTime::from_ymd_hms(2025, 7, 4, 9, 30, 0).unwrap())
            .unwrap();
        s.set_value("B1", 45_000).unwrap();
        s.set_cell_style("B1", iso).unwrap();
    }
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    assert_eq!(
        s.cell_style("A1").unwrap(),
        Some(iso),
        "an existing date format is kept"
    );
    assert_eq!(
        s.cell("A1").unwrap(),
        CellValue::DateTime(DateTime::from_ymd_hms(2025, 7, 4, 9, 30, 0).unwrap())
    );
    assert_eq!(
        s.cell("B1").unwrap(),
        CellValue::DateTime(DateTime::from_ymd(2023, 3, 15).unwrap())
    );
}

#[test]
fn the_1904_date_system() {
    let mut wb = Workbook::new();
    wb.set_date_system(DateSystem::V1904);
    let d = DateTime::from_ymd(2000, 1, 1).unwrap();
    wb.worksheet_mut("Sheet1").unwrap().set_value("A1", d).unwrap();
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    assert_eq!(back.date_system(), DateSystem::V1904);
    assert_eq!(
        back.raw_workbook().workbook_pr.as_ref().unwrap().date1904,
        Some(true)
    );
    let s = back.worksheet("Sheet1").unwrap();
    assert_eq!(s.cell("A1").unwrap(), CellValue::DateTime(d));
    let raw = &s.raw().sheet_data.as_ref().unwrap().row[0].c[0];
    assert_eq!(raw.v.as_deref(), Some("35064"), "serial counted from 1904-01-01");
}

#[test]
fn adding_renaming_and_removing_sheets() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Data").unwrap().set_value("A1", "d").unwrap();
    wb.add_worksheet("Summary").unwrap();
    assert_eq!(wb.sheet_names(), ["Sheet1", "Data", "Summary"]);
    for bad in [
        "",
        "a:b",
        "a/b",
        "a\\b",
        "a?b",
        "a*b",
        "[x]",
        "'quoted'",
        "History",
        &"x".repeat(32),
    ] {
        assert!(
            matches!(wb.add_worksheet(bad), Err(Error::InvalidArgument(_))),
            "{bad:?}"
        );
    }
    assert!(
        wb.add_worksheet("DATA").is_err(),
        "names are unique case-insensitively"
    );
    assert!(wb.add_worksheet(&"x".repeat(31)).is_ok());
    wb.remove_worksheet(&"x".repeat(31)).unwrap();
    wb.rename_worksheet("Summary", "Tổng hợp").unwrap();
    assert!(wb.rename_worksheet("Data", "sheet1").is_err());
    wb.rename_worksheet("Data", "data").unwrap();
    wb.set_defined_name("Global", "Sheet1!$A$1", None).unwrap();
    wb.set_defined_name("OnData", "data!$A$1", Some(1)).unwrap();
    wb.set_defined_name("OnSummary", "'Tổng hợp'!$A$1", Some(2))
        .unwrap();
    wb.set_active_sheet(2).unwrap();
    assert_saved_valid(&mut wb);

    let mut back = reopen(&mut wb);
    assert_eq!(back.sheet_names(), ["Sheet1", "data", "Tổng hợp"]);
    assert_eq!(
        back.worksheet("DATA").unwrap().cell("A1").unwrap().as_str(),
        Some("d"),
        "case-insensitive lookup"
    );
    assert_eq!(back.active_sheet(), 2);
    assert_eq!(back.worksheet_at(2).unwrap().name(), "Tổng hợp");
    back.remove_worksheet("data").unwrap();
    assert_eq!(back.sheet_names(), ["Sheet1", "Tổng hợp"]);
    let names = back.defined_names();
    assert_eq!(names.len(), 2);
    assert_eq!(
        names.iter().find(|n| n.name == "OnSummary").unwrap().local_sheet,
        Some(1),
        "scopes shift down"
    );
    assert_eq!(back.active_sheet(), 1);
    assert!(
        back.package().part(&pn("/xl/worksheets/sheet2.xml")).is_none(),
        "the part is removed"
    );
    assert_saved_valid(&mut back);
    back.remove_worksheet("Sheet1").unwrap();
    assert!(back.remove_worksheet("Tổng hợp").is_err(), "the last sheet stays");
    assert!(matches!(back.worksheet("missing"), Err(Error::NotFound(_))));
    assert!(back.worksheet_at(5).is_err());
    let again = reopen(&mut back);
    assert_eq!(again.sheet_names(), ["Tổng hợp"]);
}

#[test]
fn new_sheets_get_fresh_part_names_and_ids() {
    let mut wb = Workbook::new();
    wb.add_worksheet("B").unwrap();
    wb.remove_worksheet("Sheet1").unwrap();
    wb.add_worksheet("C").unwrap();
    let bytes = assert_saved_valid(&mut wb);
    let workbook = Workbook::from_bytes(&bytes).unwrap();
    let ids: Vec<u32> = workbook
        .raw_workbook()
        .sheets
        .as_ref()
        .unwrap()
        .sheet
        .iter()
        .map(|s| s.sheet_id.unwrap())
        .collect();
    assert_eq!(ids, [2, 3], "sheet ids are never reused");
    let parts: Vec<String> = workbook
        .worksheet_names()
        .iter()
        .map(|n| workbook.worksheet(n).unwrap().part_name().to_string())
        .collect();
    assert_eq!(parts, ["/xl/worksheets/sheet2.xml", "/xl/worksheets/sheet1.xml"]);
}

#[test]
fn defined_names_validation_and_removal() {
    let mut wb = Workbook::new();
    for bad in ["", "1abc", "A1", "has space", "a-b"] {
        assert!(wb.set_defined_name(bad, "Sheet1!A1", None).is_err(), "{bad:?}");
    }
    assert!(wb.set_defined_name("x", "A1", Some(9)).is_err());
    wb.set_defined_name("Rate", "=0.2", None).unwrap();
    wb.set_defined_name("rate", "0.3", None).unwrap();
    assert_eq!(
        wb.defined_names(),
        [openxml_xlsx::DefinedName {
            name: "Rate".into(),
            formula: "0.3".into(),
            local_sheet: None
        }]
    );
    assert!(wb.remove_defined_name("RATE", None));
    assert!(!wb.remove_defined_name("RATE", None));
    assert!(wb.defined_names().is_empty());
    assert!(reopen(&mut wb).raw_workbook().defined_names.is_none());
}

#[test]
fn merges_widths_heights_and_panes() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.merge_cells(CellRange::parse("A1:C1").unwrap()).unwrap();
        s.merge_cells(CellRange::parse("A3:B4").unwrap()).unwrap();
        assert!(
            s.merge_cells(CellRange::parse("B1:B2").unwrap()).is_err(),
            "overlap"
        );
        assert!(
            s.merge_cells(CellRange::parse("E5").unwrap()).is_err(),
            "single cell"
        );
        s.merge_cells(CellRange::parse("E5:F5").unwrap()).unwrap();
        assert!(s.unmerge_cells(CellRange::parse("E5:F5").unwrap()));
        assert!(!s.unmerge_cells(CellRange::parse("E5:F5").unwrap()));
        s.set_column_width(2, 30.5).unwrap();
        s.set_column_width(4, 12.0).unwrap();
        s.set_column_width(2, 25.0).unwrap();
        assert!(s.set_column_width(0, 1.0).is_err());
        assert!(s.set_column_width(1, 300.0).is_err());
        s.set_row_height(2, 40.0).unwrap();
        assert!(s.set_row_height(2, 500.0).is_err());
        assert!(s.set_row_height(0, 10.0).is_err());
        s.freeze_panes("B3").unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    let merges: Vec<String> = s.merged_ranges().iter().map(|m| m.to_string()).collect();
    assert_eq!(merges, ["A1:C1", "A3:B4"]);
    assert_eq!(s.column_width(2), Some(25.0));
    assert_eq!(s.column_width(4), Some(12.0));
    assert_eq!(s.column_width(3), None);
    assert_eq!(s.row_height(2), Some(40.0));
    assert_eq!(s.row_height(3), None);
    assert_eq!(s.frozen_at(), Some(CellRef::parse("B3").unwrap()));
    let pane = s.raw().sheet_views.as_ref().unwrap().sheet_view[0]
        .pane
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!((pane.x_split, pane.y_split), (Some(1.0), Some(2.0)));
    assert_eq!(pane.active_pane, Some(sml::ST_Pane::BottomRight));
}

#[test]
fn column_width_splits_existing_ranges() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.raw_mut().cols = vec![sml::CT_Cols {
            col: vec![sml::CT_Col {
                min: Some(1),
                max: Some(5),
                width: Some(9.0),
                style: Some(0),
                ..Default::default()
            }],
            ..Default::default()
        }];
        s.set_column_width(3, 20.0).unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let s = back.worksheet("Sheet1").unwrap();
    let cols: Vec<(u32, u32, f64)> = s.raw().cols[0]
        .col
        .iter()
        .map(|c| (c.min.unwrap(), c.max.unwrap(), c.width.unwrap()))
        .collect();
    assert_eq!(cols, [(1, 2, 9.0), (3, 3, 20.0), (4, 5, 9.0)]);
    assert_eq!(s.column_width(5), Some(9.0));
}

#[test]
fn freezing_rows_or_columns_only_and_unfreezing() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Cols").unwrap().freeze_panes("C1").unwrap();
    wb.add_worksheet("Rows").unwrap().freeze_panes("A2").unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.freeze_panes("B2").unwrap();
        s.freeze_panes("A1").unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let pane = |n: &str| {
        back.worksheet(n)
            .unwrap()
            .raw()
            .sheet_views
            .as_ref()
            .unwrap()
            .sheet_view[0]
            .pane
            .clone()
    };
    assert!(pane("Sheet1").is_none());
    assert_eq!(back.worksheet("Sheet1").unwrap().frozen_at(), None);
    let cols = pane("Cols").unwrap();
    assert_eq!(
        (cols.x_split, cols.y_split, cols.active_pane),
        (Some(2.0), None, Some(sml::ST_Pane::TopRight))
    );
    let rows = pane("Rows").unwrap();
    assert_eq!(
        (rows.x_split, rows.y_split, rows.active_pane),
        (None, Some(1.0), Some(sml::ST_Pane::BottomLeft))
    );
    assert_eq!(
        back.worksheet("Rows").unwrap().frozen_at(),
        Some(CellRef::parse("A2").unwrap())
    );
}

#[test]
fn saving_to_files_writers_and_readers() {
    let dir = std::env::temp_dir().join(format!("openxml-xlsx-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("book.xlsx");
    let mut wb = Workbook::new();
    wb.worksheet_mut("Sheet1")
        .unwrap()
        .set_value("A1", "file")
        .unwrap();
    wb.save(&path).unwrap();
    let opened = Workbook::open(&path).unwrap();
    assert_eq!(
        opened.worksheet("Sheet1").unwrap().cell("A1").unwrap().as_str(),
        Some("file")
    );
    let cursor = wb.to_cursor().unwrap();
    let from_reader = Workbook::from_reader(std::io::Cursor::new(cursor.into_inner())).unwrap();
    assert_eq!(from_reader.sheet_names(), ["Sheet1"]);
    let pkg = wb.into_package().unwrap();
    assert!(pkg.main_part().is_some());
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(Workbook::open(dir.join("missing.xlsx")).is_err());
}

#[test]
fn saving_is_deterministic_and_idempotent() {
    let mut wb = Workbook::new();
    wb.set_core_properties(&openxml_opc::CoreProperties {
        title: Some("T".into()),
        ..Default::default()
    })
    .unwrap();
    wb.worksheet_mut("Sheet1").unwrap().set_value("A1", "x").unwrap();
    let first = wb.to_bytes().unwrap();
    let second = wb.to_bytes().unwrap();
    assert_eq!(first, second);
    let mut reopened = Workbook::from_bytes(&first).unwrap();
    assert_eq!(
        reopened.to_bytes().unwrap(),
        first,
        "an untouched workbook is saved unchanged"
    );
    assert_eq!(reopened.core_properties().unwrap().title.as_deref(), Some("T"));
}

#[test]
fn non_workbooks_are_rejected() {
    let mut pkg = Package::new();
    let doc = pn("/word/document.xml");
    pkg.add_part(
        doc.clone(),
        ct::WML_DOCUMENT,
        b"<w:document xmlns:w=\"urn:x\"/>".to_vec(),
    )
    .unwrap();
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &doc)
        .unwrap();
    assert!(matches!(
        Workbook::from_package(pkg),
        Err(Error::InvalidDocument(_))
    ));
    assert!(matches!(
        Workbook::from_package(Package::new()),
        Err(Error::InvalidDocument(_))
    ));
    assert!(Workbook::from_bytes(b"not a zip").is_err());
}

#[test]
fn chart_sheets_are_listed_but_not_worksheets() {
    let mut wb = Workbook::new();
    // Add a chart sheet by hand through the escape hatches.
    let part = pn("/xl/chartsheets/sheet1.xml");
    let xml = r#"<chartsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><drawing r:id="rId1"/></chartsheet>"#;
    wb.package_mut()
        .add_part(part.clone(), ct::SML_CHARTSHEET, xml.as_bytes().to_vec())
        .unwrap();
    let rid = wb
        .package_mut()
        .add_relationship(Some(&pn("/xl/workbook.xml")), rel_types::CHARTSHEET, &part)
        .unwrap();
    wb.raw_workbook_mut()
        .sheets
        .as_mut()
        .unwrap()
        .sheet
        .push(sml::CT_Sheet {
            name: Some("Chart1".into()),
            sheet_id: Some(9),
            r_id: Some(rid),
            ..Default::default()
        });
    let back = reopen(&mut wb);
    assert_eq!(back.sheet_names(), ["Sheet1", "Chart1"]);
    assert_eq!(back.worksheet_names(), ["Sheet1"]);
    assert_eq!(back.sheet_kind("chart1"), Some(SheetKind::Chartsheet));
    assert!(matches!(back.worksheet("Chart1"), Err(Error::InvalidArgument(_))));
    assert!(back.worksheet_at(1).is_err());
    assert_eq!(back.sheet_count(), 2);
}

#[test]
fn raw_escape_hatches_are_saved() {
    let mut wb = Workbook::new();
    wb.stylesheet_mut().cell_styles.as_mut().unwrap().cell_style[0].name = Some("Normal".into());
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.raw_mut().sheet_pr = Some(Box::new(sml::CT_SheetPr {
            tab_color: Some(Box::new(sml::CT_Color {
                rgb: Some(openxml_xml::HexBinary(vec![0xFF, 0xFF, 0, 0])),
                ..Default::default()
            })),
            ..Default::default()
        }));
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let pr = back.worksheet("Sheet1").unwrap().raw().sheet_pr.clone().unwrap();
    assert!(pr.tab_color.is_some());
    assert_eq!(back.workbook_part().as_str(), "/xl/workbook.xml");
}

#[test]
fn validates_the_generated_package_parts_individually() {
    let mut wb = Workbook::new();
    wb.worksheet_mut("Sheet1")
        .unwrap()
        .set_value("A1", "valid")
        .unwrap();
    let bytes = wb.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert_valid(&pkg);
    let validated: Vec<String> = pkg
        .parts()
        .filter(|(_, p)| p.content_type().ends_with("+xml"))
        .map(|(n, _)| n.to_string())
        .collect();
    assert!(validated.contains(&"/xl/sharedStrings.xml".to_owned()));
    assert!(validated.contains(&"/docProps/app.xml".to_owned()));
}
