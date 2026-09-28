//! The streaming worksheet writer and row reader.

mod common;

use std::ops::ControlFlow;

use common::{assert_saved_valid, reopen};
use openxml_xlsx::{
    CellRef, CellStyle, CellValue, DateTime, Error, MAX_ROW, NumberFormat, StyleId, Workbook,
};

#[test]
fn writes_a_large_sheet_row_by_row() {
    let mut wb = Workbook::new();
    let bold = wb.add_style(&CellStyle::new().bold());
    let pct = wb.add_style(&CellStyle::new().number_format(NumberFormat::PERCENT));
    {
        let mut sheet = wb.add_streaming_worksheet("Big").unwrap();
        assert_eq!(sheet.next_row(), 1);
        let header = ["id", "name", "ratio", "flag", "when", "twice"];
        sheet
            .write_styled_row(header.iter().map(|h| (*h, Some(bold))))
            .unwrap();
        for i in 1..=20_000u32 {
            let row = i + 1;
            let written = sheet
                .write_styled_row([
                    (CellValue::from(i), None),
                    (CellValue::from(format!("item {}", i % 100)), None),
                    (CellValue::from(f64::from(i) / 20_000.0), Some(pct)),
                    (CellValue::from(i % 2 == 0), None),
                    (
                        CellValue::from(DateTime::from_ymd(2024, 1, 1 + (i % 28) as u8).unwrap()),
                        None,
                    ),
                    (CellValue::formula(format!("A{row}*2")), None),
                ])
                .unwrap();
            assert_eq!(written, row);
        }
        sheet.set_column_width(2, 18.0).unwrap();
        sheet.freeze_panes("A2").unwrap();
        sheet.finish().unwrap();
    }
    assert_eq!(wb.sheet_names(), ["Sheet1", "Big"]);
    let bytes = assert_saved_valid(&mut wb);
    let back = Workbook::from_bytes(&bytes).unwrap();
    let s = back.worksheet("Big").unwrap();
    assert_eq!(s.cell("A1").unwrap().as_str(), Some("id"));
    assert!(
        back.cell_style(s.cell_style("A1").unwrap().unwrap())
            .unwrap()
            .font
            .unwrap()
            .bold
    );
    assert_eq!(s.cell("A20001").unwrap(), CellValue::Number(20_000.0));
    assert_eq!(s.cell("B101").unwrap().as_str(), Some("item 0"));
    assert_eq!(s.cell("C10001").unwrap(), CellValue::Number(0.5));
    assert_eq!(s.cell("D3").unwrap(), CellValue::Bool(true));
    assert_eq!(
        s.cell("E2").unwrap(),
        CellValue::DateTime(DateTime::from_ymd(2024, 1, 2).unwrap())
    );
    assert_eq!(s.cell("F2").unwrap(), CellValue::formula("A2*2"));
    assert_eq!(s.dimension().unwrap().to_string(), "A1:F20001");
    assert_eq!(s.column_width(2), Some(18.0));
    assert_eq!(s.frozen_at(), Some(CellRef::parse("A2").unwrap()));
    assert_eq!(
        back.shared_string_count(),
        6 + 100,
        "text is deduplicated in the shared table"
    );
    assert_eq!(
        back.raw_workbook().calc_pr.as_ref().unwrap().full_calc_on_load,
        Some(true)
    );
}

#[test]
fn every_value_kind_is_streamed() {
    let mut wb = Workbook::new();
    let date = DateTime::from_ymd_hms(2021, 6, 15, 12, 30, 0).unwrap();
    {
        let mut s = wb.add_streaming_worksheet("Kinds").unwrap();
        s.write_row([
            CellValue::Number(1.5),
            CellValue::Text("  spaced  ".into()),
            CellValue::Bool(false),
            CellValue::Error("#N/A".into()),
            CellValue::DateTime(date),
            CellValue::Empty,
            CellValue::formula_with_result("A1*2", 3.0),
            CellValue::formula_with_result("\"<a&b>\"", "<a&b>"),
            CellValue::formula_with_result("TRUE()", true),
            CellValue::formula_with_result("NA()", CellValue::Error("#N/A".into())),
            CellValue::Text("ctrl\u{1}\r".into()),
        ])
        .unwrap();
        s.finish().unwrap();
    }
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    let s = back.worksheet("Kinds").unwrap();
    let v = |r: &str| s.cell(r).unwrap();
    assert_eq!(v("A1"), CellValue::Number(1.5));
    assert_eq!(v("B1"), CellValue::Text("  spaced  ".into()));
    assert_eq!(v("C1"), CellValue::Bool(false));
    assert_eq!(v("D1"), CellValue::Error("#N/A".into()));
    assert_eq!(v("E1"), CellValue::DateTime(date));
    assert_eq!(back.number_format_of(s.cell_style("E1").unwrap().unwrap()).0, 22);
    assert_eq!(v("F1"), CellValue::Empty, "empty values leave a gap");
    assert_eq!(v("G1"), CellValue::formula_with_result("A1*2", 3.0));
    assert_eq!(v("H1"), CellValue::formula_with_result("\"<a&b>\"", "<a&b>"));
    assert_eq!(v("I1"), CellValue::formula_with_result("TRUE()", true));
    assert_eq!(v("J1").result(), &CellValue::Error("#N/A".into()));
    assert_eq!(v("K1"), CellValue::Text("ctrl\u{1}\r".into()));
    assert_eq!(
        back.raw_workbook().calc_pr.as_ref().unwrap().full_calc_on_load,
        None,
        "all formulas were cached"
    );
}

#[test]
fn gaps_limits_and_errors() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.add_streaming_worksheet("Gaps").unwrap();
        s.skip_rows(4).unwrap();
        assert_eq!(s.write_row([1]).unwrap(), 5);
        assert_eq!(
            s.write_row(Vec::<CellValue>::new()).unwrap(),
            6,
            "empty rows are skipped"
        );
        assert!(
            s.write_styled_row([(1, Some(StyleId(999)))]).is_err(),
            "unknown style"
        );
        assert!(s.write_row([f64::NAN]).is_err());
        assert!(s.set_column_width(0, 10.0).is_err());
        assert!(s.freeze_panes("A0").is_err());
        assert!(s.skip_rows(MAX_ROW).is_err());
        s.skip_rows(MAX_ROW - s.next_row()).unwrap();
        assert_eq!(s.write_row(["last"]).unwrap(), MAX_ROW);
        assert!(s.write_row(["overflow"]).is_err());
        let too_wide = vec![CellValue::from(1); 16_385];
        assert!(s.write_row(too_wide).is_err());
        s.finish().unwrap();
    }
    assert!(
        matches!(wb.add_streaming_worksheet("gaps"), Err(Error::InvalidArgument(_))),
        "duplicate name"
    );
    assert!(wb.add_streaming_worksheet("bad/name").is_err());
    let back = reopen(&mut wb);
    let s = back.worksheet("Gaps").unwrap();
    assert_eq!(s.rows().map(|r| r.index()).collect::<Vec<_>>(), [5, MAX_ROW]);
    assert_eq!(s.cell((MAX_ROW, 1)).unwrap().as_str(), Some("last"));
}

#[test]
fn unfinished_streams_are_discarded_and_empty_streams_are_valid() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.add_streaming_worksheet("Dropped").unwrap();
        s.write_row(["never saved"]).unwrap();
    }
    assert_eq!(wb.sheet_names(), ["Sheet1"]);
    wb.add_streaming_worksheet("Empty").unwrap().finish().unwrap();
    assert_saved_valid(&mut wb);
    let back = reopen(&mut wb);
    assert_eq!(back.sheet_names(), ["Sheet1", "Empty"]);
    assert_eq!(back.worksheet("Empty").unwrap().rows().count(), 0);
}

#[test]
fn streamed_sheets_can_be_edited_afterwards() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.add_streaming_worksheet("S").unwrap();
        for i in 0..10 {
            s.write_row([i]).unwrap();
        }
        s.finish().unwrap();
    }
    wb.worksheet_mut("S").unwrap().set_value("B5", "edited").unwrap();
    // The row reader sees unsaved edits of loaded sheets.
    let mut fifth = Vec::new();
    wb.for_each_row("S", |row, cells| {
        if row == 5 {
            fifth = cells.to_vec();
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(fifth.len(), 2);
    assert_eq!(fifth[1].1.as_str(), Some("edited"));
    let back = reopen(&mut wb);
    let s = back.worksheet("S").unwrap();
    assert_eq!(s.cell("A10").unwrap(), CellValue::Number(9.0));
    assert_eq!(s.cell("B5").unwrap().as_str(), Some("edited"));
}

#[test]
fn row_reader_streams_from_the_saved_part() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.add_streaming_worksheet("Data").unwrap();
        for i in 1..=5_000 {
            s.write_row([CellValue::from(i), CellValue::from(format!("v{i}"))])
                .unwrap();
        }
        s.finish().unwrap();
    }
    let bytes = wb.to_bytes().unwrap();
    let back = Workbook::from_bytes(&bytes).unwrap();
    let mut rows = 0;
    let mut total = 0.0;
    back.for_each_row("Data", |row, cells| {
        rows += 1;
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].0, CellRef::new(row, 1).unwrap());
        total += cells[0].1.as_f64().unwrap();
        assert_eq!(cells[1].1.as_str(), Some(format!("v{row}").as_str()));
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(rows, 5_000);
    assert_eq!(total, 5_000.0 * 5_001.0 / 2.0);
}
