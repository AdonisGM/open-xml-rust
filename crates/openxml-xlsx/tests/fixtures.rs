//! Reading and rewriting workbooks produced by Excel and other applications.

mod common;

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::path::PathBuf;

use openxml_opc::Package;
use openxml_testkit::{fixture, office_files, workspace_root};
use openxml_xlsx::{CellRef, CellStyle, CellValue, DateSystem, DateTime, Workbook};
use openxml_xml::compare::{DiffKind, semantic_diff};
use openxml_xml::{RawElement, decode_xml_bytes};

fn xlsx_fixtures() -> Vec<PathBuf> {
    let files: Vec<PathBuf> = office_files(&workspace_root().join("fixtures"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "xlsx"))
        .collect();
    assert!(
        files.len() >= 15,
        "expected the committed xlsx fixtures, found {}",
        files.len()
    );
    files
}

fn open(name: &str) -> Workbook {
    Workbook::open(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn cell(wb: &Workbook, sheet: &str, r: &str) -> CellValue {
    wb.worksheet(sheet).unwrap().cell(r).unwrap()
}

/// All values of all worksheets, keyed by sheet and cell.
fn all_values(wb: &Workbook) -> BTreeMap<(String, CellRef), CellValue> {
    let mut out = BTreeMap::new();
    for name in wb.worksheet_names() {
        let sheet = wb.worksheet(&name).unwrap();
        for row in sheet.rows() {
            for (r, v) in row.cells() {
                out.insert((name.clone(), r), v);
            }
        }
    }
    out
}

#[test]
fn every_fixture_opens_and_every_cell_reads() {
    for path in xlsx_fixtures() {
        let wb = Workbook::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(wb.sheet_count() > 0, "{}", path.display());
        for name in wb.worksheet_names() {
            let sheet = wb
                .worksheet(&name)
                .unwrap_or_else(|e| panic!("{} {name}: {e}", path.display()));
            for row in sheet.rows() {
                assert!(row.index() >= 1);
                for (r, v) in row.cells() {
                    assert_eq!(sheet.cell(r).unwrap(), v, "{} {name}!{r}", path.display());
                }
            }
        }
    }
}

#[test]
fn sample_spreadsheet_values() {
    let wb = open("poi/SampleSS.xlsx");
    assert_eq!(wb.sheet_names(), ["First Sheet", "Sheet Number 2", "Sheet3"]);
    assert_eq!(
        cell(&wb, "First Sheet", "A1"),
        CellValue::Text("Test spreadsheet".into())
    );
    assert_eq!(
        cell(&wb, "First Sheet", "B2"),
        CellValue::Text("2nd row 2nd column".into())
    );
    assert_eq!(cell(&wb, "Sheet Number 2", "C7"), CellValue::Number(2.0));
    assert_eq!(
        cell(&wb, "Sheet Number 2", "D7"),
        CellValue::formula_with_result("SUM(A7:C7)", 13)
    );
    let sheet3 = wb.worksheet("Sheet3").unwrap();
    assert_eq!(sheet3.rows().count(), 0);
    assert_eq!(sheet3.used_range(), None);
}

#[test]
fn rich_and_inline_strings() {
    let wb = open("poi/sample.xlsx");
    assert_eq!(cell(&wb, "Sheet1", "A6"), CellValue::Text("consectetuer".into()));
    assert_eq!(cell(&wb, "Sheet1", "B6"), CellValue::Number(666.0));
    assert_eq!(
        cell(&wb, "rich test", "A1"),
        CellValue::Text("The quick brown fox jumps over the lazy dog".into()),
        "rich text runs are concatenated"
    );
    let wb = open("poi/InlineStrings.xlsx");
    assert_eq!(
        cell(&wb, "Sheet1", "C2"),
        CellValue::Text("1st Inline String".into())
    );
    assert_eq!(cell(&wb, "Sheet1", "C7"), CellValue::Text("The End".into()));
    assert_eq!(
        cell(&wb, "Sheet1", "D4"),
        CellValue::formula_with_result("A4-A$2", 31)
    );
}

#[test]
fn shared_formulas_are_expanded_per_cell() {
    let wb = open("poi/shared_formulas.xlsx");
    assert_eq!(
        cell(&wb, "Label", "A2"),
        CellValue::formula_with_result("B2", "ProductionOrderConfirmation")
    );
    assert_eq!(
        cell(&wb, "Label", "A3"),
        CellValue::formula_with_result("B3", "RequiredAcceptanceDate")
    );
    assert_eq!(cell(&wb, "Label", "A41").as_formula(), Some("B41"));
    let wb = open("poi/NumberFormatTests.xlsx");
    assert_eq!(wb.date_system(), DateSystem::V1904);
    assert_eq!(
        cell(&wb, "Tests", "A2"),
        CellValue::formula_with_result("TEXT(C2, B2)", "12.34")
    );
    assert_eq!(cell(&wb, "Tests", "A4").as_formula(), Some("TEXT(C4, B4)"));
    assert_eq!(cell(&wb, "Tests", "A152").as_formula(), Some("TEXT(C152, B152)"));
}

#[test]
fn dates_with_various_formats() {
    let wb = open("poi/Formatting.xlsx");
    let expected = DateTime::from_ymd(2006, 11, 24).unwrap();
    for r in ["B2", "B3", "B4", "B5", "B6", "B7"] {
        assert_eq!(cell(&wb, "Sheet1", r), CellValue::DateTime(expected), "{r}");
    }
    for r in ["B10", "B11", "B12", "B13"] {
        assert_eq!(cell(&wb, "Sheet1", r), CellValue::Number(10.52), "{r}");
    }
}

#[test]
fn ecma_pivot_table_workbook() {
    let wb = open("ecma/PivotTableFormats.xlsx");
    assert_eq!(wb.sheet_count(), 23);
    assert_eq!(wb.sheet_names()[0], "source data");
    assert_eq!(
        cell(&wb, "source data", "B2"),
        CellValue::Text("Freehafer    ".into()),
        "whitespace is kept"
    );
    assert_eq!(cell(&wb, "source data", "I2"), CellValue::Number(53.34));
    assert_eq!(
        cell(&wb, "source data", "H3"),
        CellValue::Text("09999".into()),
        "text that looks numeric"
    );
    assert_eq!(cell(&wb, "Report 1", "B4"), CellValue::Text("Postal Code".into()));
    assert_eq!(
        wb.worksheet("Report 2").unwrap().dimension().unwrap().to_string(),
        "B2:E27"
    );
}

#[test]
fn merged_cells_and_non_latin_text() {
    let wb = open("poi/DataValidations-49244.xlsx");
    assert_eq!(wb.worksheet("Sheet1").unwrap().merged_ranges().len(), 3);
    let wb = open("poi/style-alternate-content.xlsx");
    let sheet = wb.worksheet("Sheet2").unwrap();
    assert_eq!(sheet.merged_ranges().len(), 12);
    assert_eq!(
        sheet.cell("A2").unwrap(),
        CellValue::Text("사이버서당 무상 콘텐츠 이용 신청서".into())
    );
}

#[test]
fn formula_heavy_workbook() {
    let wb = open("poi/FormulaEvalTestData_Copy.xlsx");
    assert_eq!(
        wb.worksheet_names(),
        ["EverythingTests", "FinanceLibTests", "StatsLibTests", "misc"]
    );
    assert_eq!(
        cell(&wb, "FinanceLibTests", "O2"),
        CellValue::formula_with_result("NPV(P2,R2:U2)", 162.5)
    );
    assert_eq!(cell(&wb, "misc", "D1"), CellValue::Bool(true));
    assert_eq!(
        cell(&wb, "misc", "B1"),
        CellValue::formula_with_result("\"1\"", "1")
    );
    let formulas = all_values(&wb)
        .values()
        .filter(|v| matches!(v, CellValue::Formula { .. }))
        .count();
    assert_eq!(formulas, 1189 + 27 + 62 + 17);
}

#[test]
fn untouched_workbooks_are_saved_byte_for_byte() {
    for path in xlsx_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut wb = Workbook::open(&path).unwrap();
        // Reading does not mark anything as changed.
        let _ = all_values(&wb);
        let saved = Package::from_bytes(&wb.to_bytes().unwrap()).unwrap();
        assert_eq!(saved, original, "{}", path.display());
    }
}

fn semantic_differences(original: &Package, saved: &Package) -> Vec<openxml_xml::compare::Difference> {
    let mut out = Vec::new();
    for (name, part) in original.parts() {
        let other = saved.part(name).unwrap_or_else(|| panic!("{name} lost"));
        assert_eq!(other.relationships(), part.relationships(), "{name}");
        if other.data() == part.data() {
            continue;
        }
        let a = RawElement::parse(&decode_xml_bytes(part.data()).unwrap()).unwrap();
        let b = RawElement::parse(&decode_xml_bytes(other.data()).unwrap()).unwrap();
        out.extend(
            semantic_diff(&a, &b)
                .into_iter()
                .filter(|d| d.kind != DiffKind::Reordered)
                .map(|mut d| {
                    d.path = format!("{name}{}", d.path);
                    d
                }),
        );
    }
    out
}

fn describe(diffs: &[openxml_xml::compare::Difference]) -> String {
    diffs.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
}

/// Marks every typed part the workbook manages as changed so that saving
/// re-serializes it (parts the workbook lacks are not created).
fn force_rewrite(wb: &mut Workbook) {
    for name in wb.worksheet_names() {
        wb.worksheet_mut(&name).unwrap();
    }
    let has_styles = wb
        .package()
        .related_part(Some(wb.workbook_part()), openxml_opc::known::rel_types::STYLES)
        .is_some();
    if has_styles {
        wb.stylesheet_mut();
    }
    wb.raw_workbook_mut();
}

#[test]
fn rewritten_parts_are_semantically_identical() {
    for path in xlsx_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut wb = Workbook::open(&path).unwrap();
        let before = all_values(&wb);
        force_rewrite(&mut wb);
        let bytes = wb.to_bytes().unwrap();
        let saved = Package::from_bytes(&bytes).unwrap();
        assert_eq!(saved.part_count(), original.part_count(), "{}", path.display());
        let diffs = semantic_differences(&original, &saved);
        assert!(diffs.is_empty(), "{}:\n{}", path.display(), describe(&diffs));
        let after = all_values(&Workbook::from_bytes(&bytes).unwrap());
        assert_eq!(after, before, "{}", path.display());
    }
}

/// Differences that rewriting a worksheet introduces on purpose: explicit
/// `r` references for rows and cells that omitted them, and a `<dimension>`
/// or row `spans` corrected to cover every cell.
fn is_intentional_normalization(d: &openxml_xml::compare::Difference) -> bool {
    let worksheet = d.path.contains("/worksheet");
    let reference_added = d.kind == DiffKind::UnexpectedAttribute && d.message.starts_with("@r =");
    let last_step = d.path.rsplit('/').next().unwrap_or("");
    let dimension_fixed =
        d.kind == DiffKind::Value && last_step.starts_with("dimension[") && d.message.starts_with("@ref");
    let spans_fixed = d.kind == DiffKind::Value && d.message.starts_with("@spans");
    // `@r "A0" vs "A1"`: an invalid reference in the source was repaired.
    let invalid_reference_repaired = d.kind == DiffKind::Value
        && d.message.starts_with("@r ")
        && d.message
            .split('"')
            .nth(1)
            .is_some_and(|original| CellRef::parse(original).is_err());
    worksheet && (reference_added || dimension_fixed || spans_fixed || invalid_reference_repaired)
}

#[test]
fn intentional_normalizations_are_recognised() {
    use openxml_xml::compare::Difference;
    let d = |kind, path: &str, message: &str| Difference {
        kind,
        path: path.into(),
        message: message.into(),
    };
    assert!(is_intentional_normalization(&d(
        DiffKind::UnexpectedAttribute,
        "/xl/worksheets/sheet1.xml/worksheet/sheetData[1]/row[1]",
        "@r = \"1\""
    )));
    assert!(is_intentional_normalization(&d(
        DiffKind::Value,
        "/xl/worksheets/sheet1.xml/worksheet/dimension[1]",
        "@ref \"A1\" vs \"A1:B2\""
    )));
    assert!(!is_intentional_normalization(&d(
        DiffKind::Value,
        "/xl/worksheets/sheet1.xml/worksheet/sheetData[1]/row[1]/c[1]",
        "@s \"1\" vs \"2\""
    )));
    assert!(is_intentional_normalization(&d(
        DiffKind::Value,
        "/xl/worksheets/sheet1.xml/worksheet/sheetData[1]/row[1]/c[1]",
        "@r \"A0\" vs \"A1\""
    )));
    assert!(!is_intentional_normalization(&d(
        DiffKind::Value,
        "/xl/worksheets/sheet1.xml/worksheet/sheetData[1]/row[1]/c[1]",
        "@r \"A2\" vs \"A1\""
    )));
    assert!(!is_intentional_normalization(&d(
        DiffKind::Value,
        "/xl/styles.xml/styleSheet/fonts[1]",
        "@count \"1\" vs \"2\""
    )));
}

#[test]
fn edits_to_real_workbooks_persist_and_leave_the_rest_alone() {
    for path in xlsx_fixtures() {
        let original = Package::open_path(&path).unwrap();
        let mut wb = Workbook::open(&path).unwrap();
        let first = wb.worksheet_names()[0].clone();
        let before = all_values(&wb);
        let highlight = wb.add_style(
            &CellStyle::new()
                .bold()
                .fill_color(openxml_xlsx::Color::Rgb(255, 255, 0)),
        );
        {
            let mut s = wb.worksheet_mut(&first).unwrap();
            s.set_value("XFD1048576", "far corner").unwrap();
            s.set_value("AA1", 12.5).unwrap();
            s.set_cell_style("AA1", highlight).unwrap();
        }
        wb.add_worksheet("Added by test")
            .unwrap()
            .set_value("A1", "new")
            .unwrap();
        let bytes = wb.to_bytes().unwrap();
        let back = Workbook::from_bytes(&bytes).unwrap();
        let s = back.worksheet(&first).unwrap();
        assert_eq!(
            s.cell("XFD1048576").unwrap().as_str(),
            Some("far corner"),
            "{}",
            path.display()
        );
        assert_eq!(s.cell("AA1").unwrap(), CellValue::Number(12.5));
        assert!(
            back.cell_style(s.cell_style("AA1").unwrap().unwrap())
                .unwrap()
                .font
                .unwrap()
                .bold
        );
        assert_eq!(
            back.worksheet("Added by test")
                .unwrap()
                .cell("A1")
                .unwrap()
                .as_str(),
            Some("new")
        );
        // Everything that was there before is still there.
        let mut after = all_values(&back);
        after.remove(&(first.clone(), CellRef::parse("XFD1048576").unwrap()));
        after.remove(&(first.clone(), CellRef::parse("AA1").unwrap()));
        after.retain(|(sheet, _), _| sheet != "Added by test");
        assert_eq!(after, before, "{}", path.display());
        // Parts the edit did not touch are byte-identical.
        let saved = Package::from_bytes(&bytes).unwrap();
        let first_part = back.worksheet(&first).unwrap().part_name().clone();
        for (name, part) in original.parts() {
            let untouched = !name.as_str().ends_with("workbook.xml")
                && !name.as_str().contains("sharedStrings")
                && !name.as_str().ends_with("styles.xml")
                && *name != first_part;
            if untouched {
                assert_eq!(
                    saved.part(name).unwrap().data(),
                    part.data(),
                    "{} {name}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn streaming_row_reader_matches_the_object_model() {
    for path in xlsx_fixtures() {
        let loaded = Workbook::open(&path).unwrap();
        let streaming = Workbook::open(&path).unwrap();
        for name in loaded.worksheet_names() {
            let expected: Vec<(u32, Vec<(CellRef, CellValue)>)> = loaded
                .worksheet(&name)
                .unwrap()
                .rows()
                .map(|r| (r.index(), r.cells().collect()))
                .collect();
            let mut got = Vec::new();
            streaming
                .for_each_row(&name, |row, cells| {
                    got.push((row, cells.to_vec()));
                    ControlFlow::Continue(())
                })
                .unwrap();
            assert_eq!(got, expected, "{} {name}", path.display());
        }
    }
}

/// Runs the fidelity checks over a directory given in `OPENXML_CORPUS`:
/// `OPENXML_CORPUS=/path cargo test -p openxml-xlsx --test fixtures -- --ignored --nocapture`.
/// Files that are not valid packages (fuzzing samples, encrypted files) are
/// counted but not treated as failures.
#[test]
#[ignore = "set OPENXML_CORPUS to a directory of .xlsx files"]
fn external_corpus() {
    let Some(dir) = std::env::var_os("OPENXML_CORPUS") else {
        return;
    };
    let files: Vec<PathBuf> = office_files(std::path::Path::new(&dir))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx")))
        .collect();
    let (mut ok, mut not_packages, mut not_workbooks, mut malformed, mut normalized) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    for path in &files {
        let Ok(original) = Package::open_path(path) else {
            not_packages += 1;
            continue;
        };
        let wb = match Workbook::from_package(original.clone()) {
            Ok(wb) => wb,
            Err(openxml_xlsx::Error::InvalidDocument(_)) => {
                not_workbooks += 1;
                continue;
            }
            Err(openxml_xlsx::Error::Xml { .. }) => {
                // Not well-formed XML (e.g. entity-expansion attacks): rejecting it is correct.
                malformed += 1;
                continue;
            }
            Err(e) => {
                failures.push(format!("{}: open: {e}", path.display()));
                continue;
            }
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<bool, String> {
            let mut wb = wb;
            for name in wb.worksheet_names() {
                if let Err(e) = wb.worksheet(&name) {
                    return Err(format!("sheet {name}: {e}"));
                }
            }
            let before = all_values(&wb);
            force_rewrite(&mut wb);
            let bytes = wb.to_bytes().map_err(|e| format!("save: {e}"))?;
            let saved = Package::from_bytes(&bytes).map_err(|e| format!("reopen: {e}"))?;
            let diffs = semantic_differences(&original, &saved);
            let (intentional, unexpected): (Vec<_>, Vec<_>) =
                diffs.into_iter().partition(is_intentional_normalization);
            if !unexpected.is_empty() {
                return Err(describe(&unexpected[..unexpected.len().min(5)]));
            }
            let after = all_values(&Workbook::from_bytes(&bytes).map_err(|e| e.to_string())?);
            if after != before {
                return Err("values changed after a rewrite".into());
            }
            Ok(!intentional.is_empty())
        }));
        match outcome {
            Ok(Ok(was_normalized)) => {
                ok += 1;
                normalized += usize::from(was_normalized);
            }
            Ok(Err(e)) => failures.push(format!("{}: {e}", path.display())),
            Err(_) => failures.push(format!("{}: panicked", path.display())),
        }
    }
    println!(
        "files {}, ok {ok} (of which normalized {normalized}), not packages {not_packages}, \
         not workbooks {not_workbooks}, malformed XML {malformed}, failures {}",
        files.len(),
        failures.len()
    );
    for f in &failures {
        println!("  {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn streaming_row_reader_can_stop_early() {
    let wb = open("poi/FormulaEvalTestData_Copy.xlsx");
    let mut seen = 0;
    wb.for_each_row("EverythingTests", |_, _| {
        seen += 1;
        if seen == 10 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert_eq!(seen, 10);
    assert!(
        wb.for_each_row("missing", |_, _| ControlFlow::Continue(()))
            .is_err()
    );
}
