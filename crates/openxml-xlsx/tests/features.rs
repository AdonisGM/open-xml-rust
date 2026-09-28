//! Create → save → reopen tests for the worksheet features: pictures,
//! comments, data validation, conditional formatting, tables, filters,
//! hyperlinks, printing, protection, rich text, outline and views.

mod common;

use common::assert_saved_valid;
use openxml_core::image::tiny_png;
use openxml_opc::Package;
use openxml_xlsx::*;

/// Every produced XML part passes the crate's schema validator without issues.
fn assert_schema_clean(bytes: &[u8]) {
    let pkg = Package::from_bytes(bytes).unwrap();
    for (name, part) in pkg.parts() {
        let Ok(text) = openxml_xml::decode_xml_bytes(part.data()) else {
            continue;
        };
        if let Some(result) = openxml_schema::validate_xml(&text) {
            let issues = result.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(issues.is_empty(), "{name}: {issues:?}");
        }
    }
}

fn save_valid(wb: &mut Workbook) -> Workbook {
    let bytes = assert_saved_valid(wb);
    assert_schema_clean(&bytes);
    Workbook::from_bytes(&bytes).unwrap()
}

#[test]
fn pictures_are_anchored_listed_shared_and_removed() {
    let png = tiny_png(128, 64);
    let other = tiny_png(10, 10);
    let mut wb = Workbook::new();
    wb.add_worksheet("Two").unwrap();
    let (a, b, c, d);
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_column_width(2, 20.0).unwrap();
        a = s.add_image("B2", &png).unwrap();
        b = s
            .add_image_with(
                &png,
                &Image::one_cell("E5")
                    .unwrap()
                    .width(Length::cm(4.0))
                    .description("Logo")
                    .title("Company")
                    .name("Logo picture"),
            )
            .unwrap();
        c = s
            .add_image_with(
                &other,
                &Image::absolute(Length::emu(952_500), Length::emu(190_500)),
            )
            .unwrap();
        d = s
            .add_image_with(&other, &Image::over("H2:J6").unwrap().edit_as(EditAs::Absolute))
            .unwrap();
        assert_eq!((a, b, c, d), (2, 3, 4, 5));
    }
    wb.worksheet_mut("Two").unwrap().add_image("A1", &png).unwrap();
    let wb2 = save_valid(&mut wb);
    let media: Vec<_> = wb2
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/xl/media/"))
        .collect();
    assert_eq!(media.len(), 2, "identical images are stored once");
    let s = wb2.worksheet("Sheet1").unwrap();
    let images = s.images().unwrap();
    assert_eq!(images.len(), 4);
    let Anchor::TwoCell { from, to, edit_as } = images[0].anchor else {
        panic!("{:?}", images[0].anchor)
    };
    assert_eq!(from.cell.to_string(), "B2");
    assert_eq!(edit_as, EditAs::OneCell);
    // 128 px wide from B (140 px wide): still in column B; 64 px high = 3.2 rows of 20 px.
    assert_eq!(to.cell.to_string(), "B5");
    assert_eq!(to.dx, Length::px(128.0));
    assert_eq!(to.dy, Length::px(4.0));
    let Anchor::OneCell { width, height, .. } = images[1].anchor else {
        panic!()
    };
    assert_eq!(width, Length::cm(4.0));
    assert_eq!(height, Length::cm(2.0), "the aspect ratio is kept");
    assert_eq!(images[1].description.as_deref(), Some("Logo"));
    assert_eq!(images[1].title.as_deref(), Some("Company"));
    assert_eq!(images[1].name, "Logo picture");
    assert!(matches!(images[2].anchor, Anchor::Absolute { x, .. } if x == Length::px(100.0)));
    let Anchor::TwoCell { from, to, edit_as } = images[3].anchor else {
        panic!()
    };
    assert_eq!(
        (from.cell.to_string(), to.cell.to_string()),
        ("H2".into(), "K7".into())
    );
    assert_eq!(edit_as, EditAs::Absolute);
    assert_eq!(images[0].content_type.as_deref(), Some("image/png"));
    assert_eq!(s.image_data(&images[0]), Some(png.as_slice()));

    // Removing pictures keeps shared image parts until the last user goes.
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        assert!(s.remove_image(a).unwrap());
        assert!(!s.remove_image(a).unwrap());
        assert!(s.remove_image(c).unwrap());
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Sheet1").unwrap();
    assert_eq!(s.images().unwrap().len(), 2);
    let mut wb5 = wb4;
    {
        let mut s = wb5.worksheet_mut("Sheet1").unwrap();
        assert!(s.remove_image(b).unwrap());
        assert!(s.remove_image(d).unwrap());
        assert!(
            s.as_view().drawing_part().is_none(),
            "the empty drawing is removed"
        );
    }
    let wb6 = save_valid(&mut wb5);
    let media = wb6
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/xl/media/"))
        .count();
    assert_eq!(media, 1, "the image still used by sheet Two survives");
    assert_eq!(wb6.worksheet("Two").unwrap().images().unwrap().len(), 1);
}

#[test]
fn graphic_frames_can_be_added_to_the_drawing() {
    let mut wb = Workbook::new();
    let mut s = wb.worksheet_mut("Sheet1").unwrap();
    let drawing = s.drawing_part().unwrap();
    // Stand-in for a chart part: any part related from the drawing.
    let target = openxml_opc::PartName::new("/xl/charts/chart1.xml").unwrap();
    let chart_xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:layout/><c:pieChart/></c:plotArea></c:chart></c:chartSpace>"#;
    s.package_mut()
        .add_part(
            target.clone(),
            openxml_opc::known::content_types::CHART,
            chart_xml.as_bytes().to_vec(),
        )
        .unwrap();
    let rid = s
        .relate_from_drawing(openxml_opc::known::rel_types::CHART, &target)
        .unwrap();
    assert_eq!(
        s.relate_from_drawing(openxml_opc::known::rel_types::CHART, &target)
            .unwrap(),
        rid,
        "an existing relationship is reused"
    );
    let mut chart_ref = openxml_xml::RawElement::new(openxml_xml::Ns::C, "chart");
    chart_ref.set_attr(openxml_xml::Ns::R, "id", rid.clone());
    let data = openxml_schema::dml::CT_GraphicalObjectData {
        uri: Some("http://schemas.openxmlformats.org/drawingml/2006/chart".into()),
        any: vec![chart_ref],
        ..Default::default()
    };
    let anchor = Anchor::TwoCell {
        from: AnchorPoint::at(CellRef::parse("B2").unwrap()),
        to: AnchorPoint::at(CellRef::parse("H15").unwrap()),
        edit_as: EditAs::TwoCell,
    };
    let id = s.add_graphic_frame(&anchor, "Chart 1", data).unwrap();
    assert_eq!(id, 2);
    let wb2 = save_valid(&mut wb);
    let xml = String::from_utf8(wb2.package().part(&drawing).unwrap().data().to_vec()).unwrap();
    assert!(xml.contains("<xdr:graphicFrame macro=\"\">"), "{xml}");
    assert!(
        xml.contains(&format!(
            "<c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"{rid}\"/>"
        )),
        "{xml}"
    );
    assert!(xml.contains("<xdr:cNvPr id=\"2\" name=\"Chart 1\"/>"), "{xml}");
    assert!(wb2.worksheet("Sheet1").unwrap().images().unwrap().is_empty());
}

#[test]
fn comments_are_written_with_vml_and_read_back() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Other").unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", "value").unwrap();
        s.add_comment("A1", "Alice", "First note").unwrap();
        s.add_rich_comment("C5", "Bob", &RichText::new().bold("Bob:").push("\nsecond"))
            .unwrap();
        s.add_comment("B3", "Alice", "third").unwrap();
        assert!(s.set_comment_visible("C5", true).unwrap());
        assert!(!s.set_comment_visible("Z9", true).unwrap());
    }
    wb.worksheet_mut("Other")
        .unwrap()
        .add_comment("D4", "Carol", "elsewhere")
        .unwrap();
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let comments = s.comments().unwrap();
    assert_eq!(
        comments.iter().map(|c| c.cell.to_string()).collect::<Vec<_>>(),
        ["A1", "B3", "C5"],
        "kept in sheet order"
    );
    assert_eq!(comments[0].author, "Alice");
    assert_eq!(comments[0].text.text(), "First note");
    assert!(!comments[0].visible);
    assert_eq!(comments[2].author, "Bob");
    assert_eq!(comments[2].text.text(), "Bob:\nsecond");
    assert!(comments[2].text.runs()[0].font.as_ref().unwrap().bold);
    assert!(comments[2].visible);
    assert!(!s.has_threaded_comments());
    // The two sheets use different VML shape id blocks.
    let vml_ids: Vec<String> = wb2
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().ends_with(".vml"))
        .map(|(_, p)| {
            let text = String::from_utf8(p.data().to_vec()).unwrap();
            let start = text.find("data=\"").unwrap() + 6;
            text[start..start + 1].to_owned()
        })
        .collect();
    assert_eq!(vml_ids.len(), 2);
    assert_ne!(vml_ids[0], vml_ids[1]);

    // Replace, remove and clean up.
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        s.add_comment("A1", "Dave", "replaced").unwrap();
        assert!(s.remove_comment("B3").unwrap());
        assert!(!s.remove_comment("B3").unwrap());
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Sheet1").unwrap();
    let c = s.comment("A1").unwrap().unwrap();
    assert_eq!(
        (c.author.as_str(), c.text.text()),
        ("Dave", "replaced".to_owned())
    );
    assert_eq!(s.comments().unwrap().len(), 2);
    let mut wb5 = wb4;
    {
        let mut s = wb5.worksheet_mut("Sheet1").unwrap();
        assert!(s.remove_comment("A1").unwrap());
        assert!(s.remove_comment("C5").unwrap());
        assert!(s.as_view().comments_part().is_none());
        assert!(s.raw_mut().legacy_drawing.is_none());
    }
    let wb6 = save_valid(&mut wb5);
    assert_eq!(
        wb6.package()
            .parts()
            .filter(|(n, _)| n.as_str().ends_with(".vml") || n.as_str().contains("comments"))
            .count(),
        2,
        "only the other sheet's comments and VML remain"
    );
}

#[test]
fn comments_are_added_to_existing_vml() {
    let mut wb = Workbook::open(openxml_testkit::fixture("poi/comments.xlsx")).unwrap();
    let name = wb.worksheet_names()[0].clone();
    {
        let mut s = wb.worksheet_mut(&name).unwrap();
        s.add_comment("D10", "Tester", "added").unwrap();
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet(&name).unwrap();
    let comments = s.comments().unwrap();
    assert_eq!(comments.len(), 4);
    assert_eq!(comments[3].text.text(), "added");
    let vml = wb2
        .package()
        .parts()
        .find(|(n, _)| n.as_str().ends_with(".vml"))
        .map(|(_, p)| String::from_utf8(p.data().to_vec()).unwrap())
        .unwrap();
    assert!(vml.contains("_x0000_s1028"), "the next id of the existing block");
}

#[test]
fn data_validations_round_trip() {
    let mut wb = Workbook::new();
    let rules = vec![
        ("A2:A50", DataValidation::list(["Yes", "No", "Maybe"]).unwrap()),
        ("B2:B50", DataValidation::list_source("$H$1:$H$4").dropdown(false)),
        (
            "C2:C50",
            DataValidation::whole(Comparison::between(1, 10))
                .input_message("Quantity", "1 to 10")
                .error_message(ErrorStyle::Stop, "Invalid", "Enter a whole number from 1 to 10"),
        ),
        ("D2:D50", DataValidation::decimal(Comparison::at_least(0.5))),
        (
            "E2:E50",
            DataValidation::date(Comparison::between(
                validation::date_operand(DateTime::from_ymd(2024, 1, 1).unwrap()),
                validation::date_operand(DateTime::from_ymd(2024, 12, 31).unwrap()),
            ))
            .error_message(ErrorStyle::Warning, "Date", "Outside 2024"),
        ),
        (
            "F2:F50",
            DataValidation::time(Comparison::less_than(validation::time_operand(18, 0, 0))),
        ),
        (
            "G2:G50",
            DataValidation::text_length(Comparison::at_most(20)).allow_blank(false),
        ),
        (
            "I2:I50 K2:K50",
            DataValidation::custom("ISNUMBER(I2)").error_message(ErrorStyle::Information, "", "Numbers only"),
        ),
    ];
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for (r, dv) in &rules {
            s.add_data_validation(*r, dv).unwrap();
        }
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let got = s.data_validations();
    assert_eq!(got.len(), rules.len());
    for ((ranges, dv), (r, expected)) in got.iter().zip(&rules) {
        assert_eq!(ranges, &r.to_ranges().unwrap());
        assert_eq!(dv, expected);
    }
    assert_eq!(s.raw().data_validations.as_ref().unwrap().count, Some(8));
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        assert_eq!(s.remove_data_validations("A1:B100").unwrap(), 2);
        assert_eq!(s.remove_data_validations("K5").unwrap(), 0, "I2:I50 remains");
        assert_eq!(s.as_view().data_validations().len(), 6);
        assert_eq!(
            s.as_view().data_validations()[5].0,
            vec![CellRange::parse("I2:I50").unwrap()]
        );
        let too_long = DataValidation::whole(Comparison::equal(1)).input_message("x".repeat(40), "m");
        assert!(s.add_data_validation("A1", &too_long).is_err());
    }
    save_valid(&mut wb3);
}

#[test]
fn conditional_formats_round_trip() {
    let mut wb = Workbook::new();
    let bad = CellStyle::new()
        .font_color(Color::Rgb(0x9C, 0, 6))
        .fill_color(Color::Rgb(0xFF, 0xC7, 0xCE));
    let good = CellStyle::new()
        .bold()
        .fill(Fill::solid(Color::Rgb(0xC6, 0xEF, 0xCE)));
    let formats = vec![
        (
            "A1:A20",
            ConditionalFormat::cell_is(CfOperator::LessThan, "0").style(bad.clone()),
        ),
        (
            "B1:B20",
            ConditionalFormat::between("10", "30")
                .style(good.clone())
                .stop_if_true(),
        ),
        (
            "C1:C20",
            ConditionalFormat::expression("MOD(ROW(),2)=0").style(bad.clone()),
        ),
        (
            "D1:D20",
            ConditionalFormat::color_scale_2(Color::Rgb(255, 255, 255), Color::Rgb(0x63, 0xBE, 0x7B)),
        ),
        (
            "E1:E20",
            ConditionalFormat::color_scale_3(
                Color::Rgb(0xF8, 0x69, 0x6B),
                Color::Rgb(0xFF, 0xEB, 0x84),
                Color::Rgb(0x63, 0xBE, 0x7B),
            ),
        ),
        (
            "F1:F20",
            ConditionalFormat::data_bar(Color::Rgb(0x63, 0x8E, 0xC6)),
        ),
        ("G1:G20", ConditionalFormat::icon_set(IconSetType::V5Arrows)),
        ("H1:H20", ConditionalFormat::top(3).style(good.clone())),
        ("H1:H20", ConditionalFormat::bottom(3).style(bad.clone())),
        ("I1:I20", ConditionalFormat::above_average().style(good.clone())),
        ("J1:J20", ConditionalFormat::duplicates().style(bad.clone())),
        ("J1:J20", ConditionalFormat::unique().style(good.clone())),
        (
            "K1:K20",
            ConditionalFormat::contains_text("error").style(bad.clone()),
        ),
        ("K1:K20", ConditionalFormat::begins_with("OK").style(good.clone())),
        ("K1:K20", ConditionalFormat::ends_with("!").style(good.clone())),
        (
            "L1:L20",
            ConditionalFormat::time_period(TimePeriod::Last7Days).style(good.clone()),
        ),
        (
            "M1:M20",
            ConditionalFormat::new(CfRule::ContainsBlanks).style(bad.clone()),
        ),
    ];
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for (i, (r, f)) in formats.iter().enumerate() {
            assert_eq!(s.add_conditional_format(*r, f).unwrap(), i as i32 + 1);
        }
        let styled_bar = ConditionalFormat::data_bar(Color::Theme(4)).style(good.clone());
        assert!(s.add_conditional_format("A1", &styled_bar).is_err());
    }
    assert_eq!(
        wb.stylesheet().dxfs.as_ref().unwrap().dxf.len(),
        2,
        "identical formats share one dxf"
    );
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let got = s.conditional_formats();
    assert_eq!(got.len(), formats.len());
    for (i, ((ranges, f), (r, expected))) in got.iter().zip(&formats).enumerate() {
        assert_eq!(ranges, &r.to_ranges().unwrap());
        let mut expected = expected.clone();
        expected.priority = Some(i as i32 + 1);
        assert_eq!(f, &expected);
    }
    assert_eq!(wb2.stylesheet().dxfs.as_ref().unwrap().count, Some(2));
    let mut wb3 = wb2;
    assert_eq!(
        wb3.worksheet_mut("Sheet1")
            .unwrap()
            .remove_conditional_formats("K5")
            .unwrap(),
        3
    );
    save_valid(&mut wb3);
}

#[test]
fn tables_with_totals_and_unique_names() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for (c, h) in ["Region", "Sales", "Sales"].iter().enumerate() {
            s.set_value((1, c as u32 + 1), *h).unwrap();
        }
        s.set_value("A2", "North").unwrap();
        s.set_value("B2", 10.0).unwrap();
        s.set_value("A3", "South").unwrap();
        s.set_value("B3", 20.0).unwrap();
        let t = s
            .add_table(
                "A1:D4",
                &Table::new()
                    .name("Revenue")
                    .style(Some("TableStyleLight9"))
                    .column_stripes(true)
                    .totals("Region", TotalsRow::Label("Total".into()))
                    .totals("Sales", TotalsRow::Sum)
                    .totals("Sales2", TotalsRow::Average)
                    .totals("Column4", TotalsRow::Formula("COUNTA(Revenue[Region])".into())),
            )
            .unwrap();
        assert_eq!(
            t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["Region", "Sales", "Sales2", "Column4"]
        );
        assert_eq!(
            s.cell("C1").unwrap().as_str(),
            Some("Sales2"),
            "header cells hold the names"
        );
        assert_eq!(s.cell("A4").unwrap().as_str(), Some("Total"));
        assert_eq!(
            s.cell("B4").unwrap().as_formula(),
            Some("SUBTOTAL(109,Revenue[Sales])")
        );
        assert_eq!(
            s.cell("C4").unwrap().as_formula(),
            Some("SUBTOTAL(101,Revenue[Sales2])")
        );
        assert_eq!(
            s.cell("D4").unwrap().as_formula(),
            Some("COUNTA(Revenue[Region])")
        );
        // A second table gets the next id and the default name.
        s.set_value("F1", 2024.0).unwrap();
        let t2 = s.add_table("F1:G3", &Table::new().no_auto_filter()).unwrap();
        assert_eq!((t2.id, t2.name.as_str()), (2, "Table2"));
        assert_eq!(
            s.cell("F1").unwrap().as_str(),
            Some("2024"),
            "numeric headers become text"
        );
        assert!(s.add_table("C3:E6", &Table::new()).is_err(), "overlap");
        assert!(
            s.add_table("J1:K3", &Table::new().name("revenue")).is_err(),
            "names are unique"
        );
        assert!(s.add_table("J1:K1", &Table::new()).is_err(), "too small");
        assert!(s.add_table("J1:K3", &Table::new().name("A1")).is_err());
    }
    wb.add_worksheet("Other").unwrap();
    wb.worksheet_mut("Other")
        .unwrap()
        .add_table("B2:C5", &Table::new().no_header_row())
        .unwrap();
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let tables = s.tables().unwrap();
    assert_eq!(tables.len(), 2);
    let t = &tables[0];
    assert_eq!(
        (t.name.as_str(), t.range.to_string()),
        ("Revenue", "A1:D4".to_owned())
    );
    assert!(t.header_row && t.totals_row);
    assert_eq!(t.auto_filter.unwrap().to_string(), "A1:D3");
    assert_eq!(t.style.as_deref(), Some("TableStyleLight9"));
    assert_eq!(t.columns[1].totals, Some(TotalsRow::Sum));
    assert_eq!(
        t.columns[3].totals,
        Some(TotalsRow::Formula("COUNTA(Revenue[Region])".into()))
    );
    assert!(tables[1].auto_filter.is_none());
    let other = wb2.worksheet("Other").unwrap().tables().unwrap();
    assert_eq!((other[0].id, other[0].header_row), (3, false));
    assert_eq!(other[0].columns[0].name, "Column1");
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        s.table_mut("Table2").unwrap().comment = Some("edited".into());
        assert!(s.remove_table("Revenue").unwrap());
        assert!(!s.remove_table("Revenue").unwrap());
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Sheet1").unwrap();
    assert_eq!(s.tables().unwrap().len(), 1);
    assert_eq!(
        s.raw_table("Table2").unwrap().unwrap().comment.as_deref(),
        Some("edited")
    );
    assert_eq!(s.cell("B2").unwrap().as_f64(), Some(10.0), "cells stay");
    let table_parts = wb4
        .package()
        .parts()
        .filter(|(n, _)| n.as_str().starts_with("/xl/tables/"))
        .count();
    assert_eq!(table_parts, 2);
}

#[test]
fn auto_filter_criteria_sort_and_hidden_rows() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        let rows = [
            ("Name", "Dept", ""),
            ("An", "Sales", "7"),
            ("Binh", "IT", "3"),
            ("Chi", "Sales", "9"),
            ("Dung", "HR", "5"),
        ];
        for (i, (a, b, c)) in rows.iter().enumerate() {
            let r = i as u32 + 1;
            s.set_value((r, 1), *a).unwrap();
            s.set_value((r, 2), *b).unwrap();
            match c.parse::<f64>() {
                Ok(n) => s.set_value((r, 3), n).unwrap(),
                Err(_) => s.set_value((r, 3), "Score").unwrap(),
            }
        }
        s.set_auto_filter("A1:C5").unwrap();
        s.set_filter_column(1, ColumnFilter::values(["Sales", "HR"]))
            .unwrap();
        s.set_filter_column(2, ColumnFilter::greater_than(4.0)).unwrap();
        assert!(s.set_filter_column(5, ColumnFilter::top(1)).is_err());
        s.set_sort(&[SortKey {
            column: 2,
            descending: true,
        }])
        .unwrap();
        assert_eq!(s.apply_auto_filter().unwrap(), 1, "only Binh (IT, 3) is hidden");
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let af = s.auto_filter().unwrap();
    assert_eq!(af.range.to_string(), "A1:C5");
    assert_eq!(af.columns.len(), 2);
    assert_eq!(af.columns[0], (1, ColumnFilter::values(["Sales", "HR"])));
    assert_eq!(
        af.sort,
        [SortKey {
            column: 2,
            descending: true
        }]
    );
    assert!(s.is_row_hidden(3) && !s.is_row_hidden(2));
    let name = wb2
        .raw_workbook()
        .defined_names
        .as_ref()
        .unwrap()
        .defined_name
        .iter()
        .find(|n| n.name.as_deref() == Some("_xlnm._FilterDatabase"))
        .unwrap()
        .clone();
    assert_eq!(
        (name.value.as_str(), name.local_sheet_id, name.hidden),
        ("Sheet1!$A$1:$C$5", Some(0), Some(true))
    );
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        assert!(s.clear_filter_column(1));
        assert_eq!(s.apply_auto_filter().unwrap(), 1, "only 3 fails > 4");
        s.set_filter_column(0, ColumnFilter::custom(FilterOperator::Equal, "C*"))
            .unwrap();
        assert_eq!(s.apply_auto_filter().unwrap(), 3, "only Chi remains");
        assert!(s.remove_auto_filter().unwrap());
        assert!(!s.as_view().is_row_hidden(3), "rows are shown again");
    }
    let wb4 = save_valid(&mut wb3);
    assert!(wb4.worksheet("Sheet1").unwrap().auto_filter().is_none());
    assert!(wb4.defined_names().is_empty());
}

#[test]
fn hyperlinks_external_internal_and_styled() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Data Sheet").unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_link("A1", "Rust", LinkTarget::Url("https://www.rust-lang.org/".into()))
            .unwrap();
        s.set_link("A2", "Mail", LinkTarget::Url("mailto:someone@example.com".into()))
            .unwrap();
        s.set_link("A3", "Data", LinkTarget::Location("'Data Sheet'!B2".into()))
            .unwrap();
        s.add_hyperlink_with(
            &Hyperlink::new("C1:D2", LinkTarget::Url("https://example.com/a b?x=1&y=2".into()))
                .unwrap()
                .tooltip("Open example"),
        )
        .unwrap();
        s.add_internal_link("E5", "Sheet1!A1").unwrap();
        assert!(s.add_hyperlink("F1", "").is_err());
    }
    let hyperlink_style = wb.worksheet("Sheet1").unwrap().cell_style("A1").unwrap().unwrap();
    assert_eq!(wb.style_name(hyperlink_style).as_deref(), Some("Hyperlink"));
    let style = wb.cell_style(hyperlink_style).unwrap();
    assert!(style.font.as_ref().unwrap().underline);
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let links = s.hyperlinks();
    assert_eq!(links.len(), 5);
    assert_eq!(
        links[0].target,
        LinkTarget::Url("https://www.rust-lang.org/".into())
    );
    assert_eq!(links[2].target, LinkTarget::Location("'Data Sheet'!B2".into()));
    assert_eq!(links[3].tooltip.as_deref(), Some("Open example"));
    assert_eq!(
        links[3].target,
        LinkTarget::Url("https://example.com/a b?x=1&y=2".into())
    );
    assert_eq!(s.hyperlink("D2").unwrap().unwrap().range.to_string(), "C1:D2");
    let rels = wb2.package().relationships(Some(s.part_name())).unwrap();
    assert_eq!(rels.iter().filter(|r| r.is_external()).count(), 3);
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        assert!(s.remove_hyperlink("A1").unwrap());
        assert!(!s.remove_hyperlink("A1").unwrap());
        s.add_hyperlink("A2", "https://replaced.example/").unwrap();
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Sheet1").unwrap();
    assert_eq!(s.hyperlinks().len(), 4);
    assert_eq!(
        s.hyperlink("A2").unwrap().unwrap().target,
        LinkTarget::Url("https://replaced.example/".into())
    );
    let rels = wb4.package().relationships(Some(s.part_name())).unwrap();
    assert_eq!(
        rels.iter().filter(|r| r.is_external()).count(),
        2,
        "unused relationships are removed"
    );
}

#[test]
fn print_setup_round_trip() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Report").unwrap();
    {
        let mut s = wb.worksheet_mut("Report").unwrap();
        s.set_page_setup(&PageSetup {
            orientation: Some(Orientation::Landscape),
            paper_size: Some(print::paper::A4),
            first_page_number: Some(3),
            black_and_white: true,
            copies: Some(2),
            page_order: Some(PageOrder::OverThenDown),
            ..PageSetup::default()
        })
        .unwrap();
        s.fit_to_pages(1, 0).unwrap();
        s.set_page_margins(&PageMargins {
            left: 0.5,
            right: 0.5,
            top: 1.0,
            bottom: 1.0,
            header: 0.4,
            footer: 0.4,
        })
        .unwrap();
        s.set_print_options(&PrintOptions {
            gridlines: true,
            headings: true,
            center_horizontally: true,
            center_vertically: false,
        });
        s.set_header_footer(
            &HeaderFooter::new()
                .header(print::hf::sections("&D", "&B Sales && Co", "&A"))
                .footer(print::hf::sections("", "Page &P of &N", ""))
                .first("First page", ""),
        )
        .unwrap();
        s.set_print_area("A1:H40").unwrap();
        s.set_print_titles(Some((1, 2)), Some((1, 1))).unwrap();
        s.add_row_break(20).unwrap();
        s.add_row_break(10).unwrap();
        s.add_column_break(4).unwrap();
        let tiny = PageSetup {
            scale: Some(5),
            ..PageSetup::default()
        };
        assert!(s.set_page_setup(&tiny).is_err());
        assert!(
            s.set_header_footer(&HeaderFooter::new().header("x".repeat(256)))
                .is_err()
        );
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Report").unwrap();
    let setup = s.page_setup();
    assert_eq!(setup.orientation, Some(Orientation::Landscape));
    assert_eq!(setup.paper_size, Some(9));
    assert_eq!((setup.fit_to_width, setup.fit_to_height), (Some(1), Some(0)));
    assert_eq!(setup.first_page_number, Some(3));
    assert!(setup.black_and_white);
    assert_eq!(setup.copies, Some(2));
    assert_eq!(s.page_margins().unwrap().top, 1.0);
    assert!(s.print_options().gridlines && s.print_options().center_horizontally);
    let hf = s.header_footer().unwrap();
    assert_eq!(hf.odd_header.as_deref(), Some("&L&D&C&B Sales && Co&R&A"));
    assert!(hf.different_first);
    assert_eq!(hf.first_header.as_deref(), Some("First page"));
    assert_eq!(s.print_area().as_deref(), Some("Report!$A$1:$H$40"));
    assert_eq!(s.print_titles().as_deref(), Some("Report!$1:$2,Report!$A:$A"));
    assert_eq!(s.row_breaks(), [10, 20]);
    assert_eq!(s.column_breaks(), [4]);
    let names = wb2.defined_names();
    assert!(
        names.iter().all(|n| n.local_sheet == Some(1)),
        "names are local to the sheet"
    );
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Report").unwrap();
        assert!(s.clear_print_area());
        s.set_print_titles(None, None).unwrap();
        assert!(s.remove_row_break(10));
        s.set_print_options(&PrintOptions::default());
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Report").unwrap();
    assert!(s.print_area().is_none() && s.print_titles().is_none());
    assert_eq!(s.row_breaks(), [20]);
    assert_eq!(s.print_options(), PrintOptions::default());
}

#[test]
fn sheet_and_workbook_protection() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Modern").unwrap();
    let unlocked = wb.add_style(&CellStyle::new().unlocked());
    let hidden = wb.add_style(&CellStyle::new().formula_hidden());
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", 5.0).unwrap();
        s.set_cell_style("A1", unlocked).unwrap();
        s.set_formula("B1", "A1*2").unwrap();
        s.set_cell_style("B1", hidden).unwrap();
        s.protect(
            &SheetProtection::new()
                .password("password")
                .allow_sort()
                .allow_format_cells(),
        )
        .unwrap();
    }
    wb.worksheet_mut("Modern")
        .unwrap()
        .protect(&SheetProtection::new().password("pwd").sha512(1_000))
        .unwrap();
    wb.protect_workbook(&WorkbookProtection::new().password("book").lock_windows(true));
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let p = s.protection().unwrap();
    assert!(p.sort && p.format_cells && !p.insert_rows && p.select_locked_cells);
    assert!(s.sheet_has_password());
    assert!(s.verify_sheet_password("password"));
    assert!(!s.verify_sheet_password("Password"));
    assert_eq!(
        s.raw()
            .sheet_protection
            .as_ref()
            .unwrap()
            .password
            .as_ref()
            .unwrap()
            .as_bytes(),
        [0x83, 0xAF]
    );
    let style = wb2.cell_style(s.cell_style("A1").unwrap().unwrap()).unwrap();
    assert_eq!(
        style.protection,
        Some(CellProtection {
            locked: false,
            hidden: false
        })
    );
    let style = wb2.cell_style(s.cell_style("B1").unwrap().unwrap()).unwrap();
    assert_eq!(
        style.protection,
        Some(CellProtection {
            locked: true,
            hidden: true
        })
    );
    let m = wb2.worksheet("Modern").unwrap();
    assert!(m.verify_sheet_password("pwd") && !m.verify_sheet_password("pw"));
    let raw = m.raw().sheet_protection.clone().unwrap();
    assert_eq!(raw.algorithm_name.as_deref(), Some("SHA-512"));
    assert_eq!(raw.spin_count, Some(1000));
    assert_eq!(raw.salt_value.unwrap().0.len(), 16);
    assert_eq!(wb2.workbook_protection(), Some((true, true)));
    assert!(wb2.verify_workbook_password("book"));
    let mut wb3 = wb2;
    assert!(wb3.worksheet_mut("Sheet1").unwrap().unprotect());
    assert!(wb3.unprotect_workbook());
    let wb4 = save_valid(&mut wb3);
    assert!(wb4.worksheet("Sheet1").unwrap().protection().is_none());
    assert!(wb4.workbook_protection().is_none());
}

#[test]
fn rich_text_cells() {
    let mut wb = Workbook::new();
    let text = RichText::new().push("Total: ").bold("42").push_styled(
        " units",
        Font {
            italic: true,
            color: Some(Color::Rgb(0x80, 0x80, 0x80)),
            size: Some(9.0),
            name: Some("Arial".into()),
            vertical_align: Some(FontVerticalAlign::Superscript),
            ..Font::default()
        },
    );
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_rich_text("A1", &text).unwrap();
        s.set_rich_text("A2", &text).unwrap();
        s.set_rich_text("A3", &RichText::from("plain")).unwrap();
        s.set_value("A4", "plain").unwrap();
    }
    assert_eq!(
        wb.shared_string_count(),
        2,
        "identical rich and plain strings are shared"
    );
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    assert_eq!(s.rich_text("A1").unwrap().unwrap(), text);
    assert_eq!(s.rich_text("A2").unwrap().unwrap(), text);
    assert_eq!(s.cell("A1").unwrap().as_str(), Some("Total: 42 units"));
    assert_eq!(s.rich_text("A4").unwrap().unwrap(), RichText::from("plain"));
    assert_eq!(s.rich_text("B9").unwrap(), None);
}

#[test]
fn extended_cell_styles_and_named_styles() {
    let mut wb = Workbook::new();
    let diagonal = BorderSide {
        style: BorderStyle::Dashed,
        color: Some(Color::Rgb(1, 2, 3)),
    };
    let styles = [
        CellStyle::new().double_underline().superscript(),
        CellStyle::new()
            .underline_style(UnderlineStyle::SingleAccounting)
            .subscript()
            .strike(),
        CellStyle::new().fill(Fill::pattern(
            PatternType::DarkGrid,
            Some(Color::Rgb(255, 0, 0)),
            Some(Color::Theme(1)),
        )),
        CellStyle::new().fill(Fill::linear_gradient(
            90.0,
            Color::Rgb(255, 255, 255),
            Color::Rgb(0x44, 0x72, 0xC4),
        )),
        CellStyle::new().fill(Fill::gradient(GradientFill {
            kind: GradientKind::Path {
                left: 0.5,
                right: 0.5,
                top: 0.5,
                bottom: 0.5,
            },
            stops: vec![(0.0, Color::Theme(0)), (1.0, Color::Theme(4))],
        })),
        CellStyle::new().border(Border::all(BorderStyle::Thin, None).with_diagonal(diagonal, true, true)),
        CellStyle::new().shrink_to_fit().indent(2),
        CellStyle::new().vertical_text(),
        CellStyle::new().rotation(135),
        CellStyle::new().unlocked().formula_hidden(),
        CellStyle::new().number_format(NumberFormat::accounting("$", 2)),
        CellStyle::new().number_format(NumberFormat::currency("€", 0)),
        CellStyle::new().number_format(NumberFormat::percent(1)),
        CellStyle::new().number_format(NumberFormat::DURATION),
    ];
    let ids: Vec<StyleId> = styles.iter().map(|s| wb.add_style(s)).collect();
    let title_style = CellStyle::new().bold().font_size(18.0);
    let title = wb.add_named_style("Title Big", &title_style).unwrap();
    assert_eq!(wb.add_named_style("Title Big", &title_style).unwrap(), title);
    assert!(
        wb.add_named_style("title big", &CellStyle::new().italic())
            .is_err()
    );
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for (i, id) in ids.iter().enumerate() {
            s.set_value((i as u32 + 1, 1), "x").unwrap();
            s.set_cell_style((i as u32 + 1, 1), *id).unwrap();
        }
        s.set_value("B1", "title").unwrap();
        s.set_cell_style("B1", title).unwrap();
    }
    let wb2 = save_valid(&mut wb);
    for (style, id) in styles.iter().zip(&ids) {
        assert_eq!(&wb2.cell_style(*id).unwrap(), style);
    }
    assert_eq!(
        NumberFormat::decimal(2, true),
        NumberFormat::THOUSANDS_DECIMAL_2,
        "built-in codes map to ids"
    );
    assert_eq!(NumberFormat::percent(0), NumberFormat::PERCENT);
    assert_eq!(NumberFormat::scientific(2), NumberFormat::SCIENTIFIC);
    assert_eq!(NumberFormat::decimal(3, false), NumberFormat::custom("0.000"));
    assert_eq!(NumberFormat::DURATION.code(), Some("[h]:mm:ss"));
    assert!(wb2.named_styles().contains(&"Title Big".to_owned()));
    let s = wb2.worksheet("Sheet1").unwrap();
    assert_eq!(
        wb2.style_name(s.cell_style("B1").unwrap().unwrap()).as_deref(),
        Some("Title Big")
    );
    assert_eq!(wb2.style_name(ids[0]).as_deref(), Some("Normal"));
}

#[test]
fn outline_hidden_lines_and_defaults() {
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for r in 1..=10 {
            s.set_value((r, 1), f64::from(r)).unwrap();
        }
        s.group_rows(2, 8, false).unwrap();
        s.group_rows(3, 5, true).unwrap();
        s.group_columns(2, 4, true).unwrap();
        s.set_row_hidden(10, true).unwrap();
        s.set_column_hidden(7, true).unwrap();
        s.set_columns_width(8, 9, 30.0).unwrap();
        s.set_default_row_height(18.0).unwrap();
        s.set_default_column_width(12.0).unwrap();
        s.set_outline_summary(true, false);
        assert!(s.group_rows(0, 1, false).is_err());
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    assert_eq!(
        (
            s.row_outline_level(4),
            s.row_outline_level(7),
            s.row_outline_level(9)
        ),
        (2, 1, 0)
    );
    assert!(s.is_row_hidden(4) && !s.is_row_hidden(7) && s.is_row_hidden(10));
    assert_eq!(
        s.raw().sheet_data.as_ref().unwrap().row[5].collapsed,
        Some(true),
        "summary row 6"
    );
    assert_eq!(s.column_outline_level(3), 1);
    assert!(s.is_column_hidden(3) && s.is_column_hidden(7) && !s.is_column_hidden(8));
    assert_eq!(s.column_width(9), Some(30.0));
    assert_eq!(s.default_row_height(), 18.0);
    assert_eq!(s.default_column_width(), Some(12.0));
    assert_eq!(s.outline_summary(), (true, false));
    let f = s.raw().sheet_format_pr.as_ref().unwrap();
    assert_eq!((f.outline_level_row, f.outline_level_col), (Some(2), Some(1)));
    let mut wb3 = wb2;
    {
        let mut s = wb3.worksheet_mut("Sheet1").unwrap();
        s.ungroup_rows(3, 5).unwrap();
        s.ungroup_columns(2, 4).unwrap();
        s.set_row_hidden(10, false).unwrap();
    }
    let wb4 = save_valid(&mut wb3);
    let s = wb4.worksheet("Sheet1").unwrap();
    assert_eq!(s.row_outline_level(4), 1);
    assert!(!s.is_row_hidden(4) && !s.is_row_hidden(10) && !s.is_column_hidden(3));
}

#[test]
fn sheet_views_visibility_order_and_calc_settings() {
    let mut wb = Workbook::new();
    wb.add_worksheet("B").unwrap();
    wb.add_worksheet("C").unwrap();
    {
        let mut s = wb.worksheet_mut("B").unwrap();
        s.set_zoom(150).unwrap();
        s.set_show_gridlines(false);
        s.set_show_headings(false);
        s.set_show_zeros(false);
        s.set_right_to_left(true);
        s.set_tab_color(Some(Color::Rgb(0xFF, 0x80, 0)));
        s.set_view_mode(SheetViewMode::PageBreakPreview);
        s.freeze_panes("B2").unwrap();
        s.set_selection("C3", "C3:D4 F6").unwrap();
        s.set_top_left_cell("A1").unwrap();
        s.set_print_area("A1:D10").unwrap();
        assert!(s.set_zoom(5).is_err());
        assert!(s.set_selection("A1", "C3:D4").is_err());
    }
    wb.set_active_sheet(2).unwrap();
    wb.set_sheet_visibility("C", SheetVisibility::Hidden).unwrap();
    assert_eq!(
        wb.active_sheet(),
        0,
        "hiding the active sheet activates a visible one"
    );
    wb.set_sheet_visibility("Sheet1", SheetVisibility::VeryHidden)
        .unwrap();
    assert!(
        wb.set_sheet_visibility("B", SheetVisibility::Hidden).is_err(),
        "one sheet stays visible"
    );
    wb.move_sheet("B", 0).unwrap();
    assert_eq!(wb.sheet_names(), ["B", "Sheet1", "C"]);
    wb.set_calc_properties(&CalcProperties {
        mode: CalcMode::Manual,
        iterate: true,
        iterate_count: 50,
        iterate_delta: 0.01,
        ..CalcProperties::default()
    });
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("B").unwrap();
    assert_eq!(s.zoom(), 150);
    assert!(!s.show_gridlines() && !s.show_headings() && !s.show_zeros() && s.right_to_left());
    assert_eq!(s.tab_color(), Some(Color::Rgb(0xFF, 0x80, 0)));
    assert_eq!(s.view_mode(), SheetViewMode::PageBreakPreview);
    assert_eq!(s.active_cell().unwrap().to_string(), "C3");
    assert_eq!(s.selection().len(), 2);
    assert_eq!(s.frozen_at().unwrap().to_string(), "B2");
    assert_eq!(
        s.print_area().as_deref(),
        Some("B!$A$1:$D$10"),
        "the local name follows the move"
    );
    assert_eq!(wb2.defined_names()[0].local_sheet, Some(0));
    assert_eq!(
        wb2.sheet_visibility("Sheet1").unwrap(),
        SheetVisibility::VeryHidden
    );
    assert_eq!(wb2.sheet_visibility("C").unwrap(), SheetVisibility::Hidden);
    assert_eq!(wb2.sheet_visibility("B").unwrap(), SheetVisibility::Visible);
    assert_eq!(wb2.sheet_names(), ["B", "Sheet1", "C"]);
    let calc = wb2.calc_properties();
    assert_eq!(
        (calc.mode, calc.iterate, calc.iterate_count),
        (CalcMode::Manual, true, 50)
    );
}

#[test]
fn inserting_and_deleting_rows_and_columns_updates_references() {
    let png = tiny_png(20, 20);
    let mut wb = Workbook::new();
    wb.add_worksheet("Other").unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for r in 1..=10 {
            s.set_value((r, 1), f64::from(r)).unwrap();
        }
        s.set_formula("B1", "SUM(A1:A10)").unwrap();
        s.set_formula("B2", "A5*2").unwrap();
        s.set_formula("B3", "$A$10+A3").unwrap();
        s.merge_cells(CellRange::parse("C5:D6").unwrap()).unwrap();
        s.add_data_validation("E4:E8", &DataValidation::custom("E4>A4"))
            .unwrap();
        let bold = ConditionalFormat::expression("A4>$A$9").style(CellStyle::new().bold());
        s.add_conditional_format("A4:A9", &bold).unwrap();
        s.add_hyperlink("F7", "https://example.com/").unwrap();
        s.add_hyperlink("F3", "https://example.org/").unwrap();
        s.add_comment("G8", "me", "note").unwrap();
        s.add_image("H6", &png).unwrap();
        s.set_print_area("A1:H10").unwrap();
        s.add_row_break(6).unwrap();
        s.set_value("J12", "h1").unwrap();
        s.set_value("K12", "h2").unwrap();
        s.add_table("J12:K15", &Table::new().name("T")).unwrap();
    }
    wb.worksheet_mut("Other")
        .unwrap()
        .set_formula("A1", "Sheet1!A5+Sheet1!A2")
        .unwrap();
    // Insert two rows above row 4.
    wb.insert_rows("Sheet1", 4, 2).unwrap();
    {
        let s = wb.worksheet("Sheet1").unwrap();
        assert_eq!(s.cell("A6").unwrap().as_f64(), Some(4.0));
        assert_eq!(s.cell("A12").unwrap().as_f64(), Some(10.0));
        assert_eq!(s.cell("B1").unwrap().as_formula(), Some("SUM(A1:A12)"));
        assert_eq!(s.cell("B2").unwrap().as_formula(), Some("A7*2"));
        assert_eq!(s.cell("B3").unwrap().as_formula(), Some("$A$12+A3"));
        assert_eq!(s.merged_ranges(), [CellRange::parse("C7:D8").unwrap()]);
        let dv = &s.data_validations()[0];
        assert_eq!(dv.0, [CellRange::parse("E6:E10").unwrap()]);
        assert_eq!(dv.1.rule, ValidationRule::Custom("E6>A6".into()));
        let (ranges, cf) = &s.conditional_formats()[0];
        assert_eq!(ranges, &[CellRange::parse("A6:A11").unwrap()]);
        assert_eq!(cf.rule, CfRule::Expression("A6>$A$11".into()));
        assert_eq!(
            s.hyperlink("F9").unwrap().unwrap().target,
            LinkTarget::Url("https://example.com/".into())
        );
        assert!(s.hyperlink("F3").unwrap().is_some());
        assert_eq!(s.comments().unwrap()[0].cell.to_string(), "G10");
        let Anchor::TwoCell { from, .. } = s.images().unwrap()[0].anchor else {
            panic!("two-cell anchor")
        };
        assert_eq!(from.cell.to_string(), "H8");
        assert_eq!(s.print_area().as_deref(), Some("Sheet1!$A$1:$H$12"));
        assert_eq!(s.row_breaks(), [8]);
        assert_eq!(s.tables().unwrap()[0].range.to_string(), "J14:K17");
    }
    assert_eq!(
        wb.worksheet("Other").unwrap().cell("A1").unwrap().as_formula(),
        Some("Sheet1!A7+Sheet1!A2")
    );
    // Delete rows 6..=7 (the values 4 and 5).
    wb.delete_rows("Sheet1", 6, 2).unwrap();
    {
        let s = wb.worksheet("Sheet1").unwrap();
        assert_eq!(s.cell("A6").unwrap().as_f64(), Some(6.0));
        assert_eq!(s.cell("B1").unwrap().as_formula(), Some("SUM(A1:A10)"));
        assert_eq!(
            s.cell("B2").unwrap().as_formula(),
            Some("#REF!*2"),
            "A7 held 5 and was deleted"
        );
        assert_eq!(
            s.merged_ranges(),
            [CellRange::parse("C6:D6").unwrap()],
            "C7:D8 lost its first row"
        );
        assert!(s.table("T").unwrap().is_some());
    }
    assert_eq!(
        wb.worksheet("Other").unwrap().cell("A1").unwrap().as_formula(),
        Some("Sheet1!#REF!+Sheet1!A2")
    );
    // Columns: insert one before B, then delete column A.
    wb.insert_columns("Sheet1", 2, 1).unwrap();
    {
        let s = wb.worksheet("Sheet1").unwrap();
        assert_eq!(s.cell("C1").unwrap().as_formula(), Some("SUM(A1:A10)"));
        assert_eq!(s.comments().unwrap()[0].cell.to_string(), "H8");
        assert_eq!(s.tables().unwrap()[0].range.to_string(), "K12:L15");
    }
    assert!(wb.insert_columns("Sheet1", 12, 1).is_err(), "inside a table");
    assert!(wb.delete_rows("Sheet1", 12, 1).is_err(), "the table header");
    wb.delete_columns("Sheet1", 1, 1).unwrap();
    {
        let s = wb.worksheet("Sheet1").unwrap();
        assert_eq!(s.cell("B1").unwrap().as_formula(), Some("SUM(#REF!)"));
        assert_eq!(s.print_area().as_deref(), Some("Sheet1!$A$1:$H$10"));
        assert_eq!(s.tables().unwrap()[0].range.to_string(), "J12:K15");
    }
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    assert_eq!(s.dimension(), s.used_range());
}

#[test]
fn renaming_and_removing_sheets_keeps_references_valid() {
    let png = tiny_png(8, 8);
    let mut wb = Workbook::new();
    wb.add_worksheet("Data").unwrap();
    wb.add_worksheet("Untouched").unwrap();
    wb.worksheet_mut("Untouched")
        .unwrap()
        .set_value("A1", 1.0)
        .unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_formula("A1", "Data!A1*2").unwrap();
        s.set_formula("A2", "SUM(Data!A1:B2)").unwrap();
    }
    {
        let mut s = wb.worksheet_mut("Data").unwrap();
        s.add_image("B2", &png).unwrap();
        s.add_comment("A1", "x", "y").unwrap();
        s.set_value("C1", "h").unwrap();
        s.add_table("C1:C3", &Table::new()).unwrap();
    }
    wb.set_defined_name("Total", "Data!$A$1:$A$9", None).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(&bytes).unwrap();
    let untouched = wb.worksheet("Untouched").unwrap().part_name().clone();
    let before = wb.package().part(&untouched).unwrap().data().to_vec();
    wb.rename_worksheet("Data", "My Data").unwrap();
    assert_eq!(
        wb.worksheet("Sheet1").unwrap().cell("A1").unwrap().as_formula(),
        Some("'My Data'!A1*2")
    );
    assert_eq!(wb.defined_names()[0].formula, "'My Data'!$A$1:$A$9");
    wb.remove_worksheet("My Data").unwrap();
    assert_eq!(
        wb.worksheet("Sheet1").unwrap().cell("A2").unwrap().as_formula(),
        Some("SUM(#REF!)")
    );
    assert_eq!(wb.defined_names()[0].formula, "#REF!");
    let wb2 = save_valid(&mut wb);
    let leftovers: Vec<String> = wb2
        .package()
        .parts()
        .map(|(n, _)| n.as_str().to_owned())
        .filter(|n| {
            n.contains("drawing") || n.contains("comments") || n.contains("tables") || n.contains("media")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "parts of the removed sheet are removed: {leftovers:?}"
    );
    assert_eq!(
        wb2.package().part(&untouched).unwrap().data(),
        before.as_slice(),
        "untouched sheets are not rewritten"
    );
}

#[test]
fn formulas_are_calculated_on_request() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Rates").unwrap();
    wb.worksheet_mut("Rates").unwrap().set_value("A1", 0.1).unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.set_value("A1", 100.0).unwrap();
        s.set_value("A2", 50.0).unwrap();
        s.set_value("A3", "text").unwrap();
        s.set_formula("B1", "SUM(A1:A3)*(1+Rates!A1)").unwrap();
        s.set_formula("B2", "AVERAGE(A1:A2)").unwrap();
        s.set_formula("B3", "IF(B1>100,CONCAT(\"high: \",B1),\"low\")")
            .unwrap();
        s.set_formula("B4", "COUNT(A1:A3)&\"/\"&COUNTA(A1:A3)").unwrap();
        s.set_formula("B5", "MAX(A1:A2)-MIN(A1:A2)").unwrap();
        s.set_formula("B6", "A1/0").unwrap();
        s.set_formula("B7", "VLOOKUP(1,A1:A3,1)").unwrap();
        s.set_formula("B8", "B7+1").unwrap();
        s.set_formula("C1", "C2+1").unwrap();
        s.set_formula("C2", "C1+1").unwrap();
    }
    let report = wb.calculate().unwrap();
    assert_eq!(
        report,
        CalcReport {
            calculated: 6,
            skipped: 4,
            updated: 6
        }
    );
    let wb2 = save_valid(&mut wb);
    let s = wb2.worksheet("Sheet1").unwrap();
    let result = |c: &str| s.cell(c).unwrap().result().clone();
    assert!((result("B1").as_f64().unwrap() - 165.0).abs() < 1e-9);
    assert_eq!(result("B2"), CellValue::Number(75.0));
    assert!(result("B3").as_str().unwrap().starts_with("high: 165"));
    assert_eq!(result("B4"), CellValue::Text("2/3".into()));
    assert_eq!(result("B5"), CellValue::Number(50.0));
    assert_eq!(result("B6"), CellValue::Error("#DIV/0!".into()));
    assert_eq!(
        result("B7"),
        CellValue::Empty,
        "unsupported functions keep their result"
    );
    assert_eq!(result("C1"), CellValue::Empty, "circular references are skipped");
    let mut wb3 = wb2;
    let rates = wb3.worksheet("Rates").unwrap().part_name().clone();
    let before = wb3.package().part(&rates).unwrap().data().to_vec();
    assert_eq!(wb3.calculate().unwrap().updated, 0, "results are already current");
    let bytes = wb3.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert_eq!(pkg.part(&rates).unwrap().data(), before.as_slice());
}

#[test]
fn untouched_parts_stay_byte_identical_after_feature_edits() {
    let original = std::fs::read(openxml_testkit::fixture("poi/WithDrawing.xlsx")).unwrap();
    let before = Package::from_bytes(&original).unwrap();
    let mut wb = Workbook::from_bytes(&original).unwrap();
    {
        let mut s = wb.worksheet_mut("Sheet2").unwrap();
        s.add_image("B2", &tiny_png(4, 4)).unwrap();
        s.add_comment("A1", "me", "hello").unwrap();
        s.add_data_validation("C1:C5", &DataValidation::list(["a", "b"]).unwrap())
            .unwrap();
        s.add_hyperlink("D1", "https://example.com/").unwrap();
    }
    let bytes = assert_saved_valid(&mut wb);
    let after = Package::from_bytes(&bytes).unwrap();
    let sheet2 = wb.worksheet("Sheet2").unwrap().part_name().clone();
    for (name, part) in before.parts() {
        if *name == sheet2 {
            continue;
        }
        let new = after.part(name).unwrap_or_else(|| panic!("{name} is kept"));
        assert_eq!(new.data(), part.data(), "{name} is byte-identical");
    }
    let wb2 = Workbook::from_bytes(&bytes).unwrap();
    assert_eq!(wb2.worksheet("Sheet1").unwrap().images().unwrap().len(), 5);
    assert_eq!(wb2.worksheet("Sheet2").unwrap().images().unwrap().len(), 1);
}
