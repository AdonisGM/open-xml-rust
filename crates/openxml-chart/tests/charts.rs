//! Charts: every kind validates against the ECMA-376 schemas and reads back
//! with the data it was built from; embedded workbooks hold the same data at
//! the referenced cells; charts written by Office applications are readable.

use openxml_chart::*;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_testkit::{Validation, fixture, validate_package, validate_xml};
use openxml_xlsx::{CellValue, Workbook};

const KINDS: [ChartKind; 8] = [
    ChartKind::Column,
    ChartKind::Bar,
    ChartKind::Line,
    ChartKind::Area,
    ChartKind::Pie,
    ChartKind::Doughnut,
    ChartKind::Scatter,
    ChartKind::Radar,
];

fn sample(kind: ChartKind) -> Chart {
    let chart = Chart::new(kind)
        .title("Sales by quarter")
        .categories(["Q1", "Q2", "Q3", "Q4"])
        .series(Series::new("North", [10.0, 12.5, 9.0, 14.0]).color(0x4472C4))
        .axis_titles("Quarter", "Units")
        .legend(Some(LegendPosition::Bottom))
        .data_labels(true);
    if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) {
        chart
    } else {
        chart.series(Series::new("South", [8.0, f64::NAN, 11.0, 7.5]).color(0xED7D31))
    }
}

fn assert_valid(xml: &str) {
    match validate_xml(xml) {
        Ok(Validation::Valid | Validation::Skipped) => {}
        Err(e) => panic!("XSD validation failed:\n{e}\n{xml}"),
    }
    let issues = openxml_schema::validate_xml(xml)
        .expect("known root")
        .expect("parses");
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn every_kind_is_schema_valid_and_reads_back() {
    for kind in KINDS {
        let chart = sample(kind);
        let xml = chart.to_xml();
        assert_valid(&xml);
        let info = read_chart(xml.as_bytes()).unwrap();
        assert_eq!(info.title.as_deref(), Some("Sales by quarter"), "{kind:?}");
        assert_eq!(info.plots.len(), 1);
        assert_eq!(info.plots[0].kind, Some(kind));
        let north = &info.plots[0].series[0];
        assert_eq!(north.name.as_deref(), Some("North"));
        assert_eq!(north.values, [Some(10.0), Some(12.5), Some(9.0), Some(14.0)]);
        if kind == ChartKind::Scatter {
            assert_eq!(north.categories, ["1", "2", "3", "4"], "default x values");
        } else {
            assert_eq!(north.categories, ["Q1", "Q2", "Q3", "Q4"]);
        }
        if kind.has_axes() {
            let south = &info.plots[0].series[1];
            assert_eq!(
                south.values,
                [Some(8.0), None, Some(11.0), Some(7.5)],
                "missing point kept as a gap"
            );
        }
    }
}

#[test]
fn grouping_and_style_options_are_schema_valid() {
    for kind in [
        ChartKind::Column,
        ChartKind::Bar,
        ChartKind::Line,
        ChartKind::Area,
    ] {
        for grouping in [Grouping::Standard, Grouping::Stacked, Grouping::PercentStacked] {
            let xml = sample(kind)
                .grouping(grouping)
                .markers(false)
                .smooth(true)
                .to_xml();
            assert_valid(&xml);
        }
    }
    assert_valid(
        &sample(ChartKind::Scatter)
            .scatter_lines(true)
            .smooth(true)
            .x_axis_only()
            .to_xml(),
    );
    assert_valid(&sample(ChartKind::Radar).radar_filled(true).to_xml());
    assert_valid(&sample(ChartKind::Doughnut).hole_size(30).legend(None).to_xml());
    assert_valid(
        &Chart::new(ChartKind::Column)
            .series(Series::new("empty", []))
            .to_xml(),
    );
    assert_valid(&sample(ChartKind::Line).number_format("0.0%").to_xml());
}

/// Helper so the test above can also cover a chart without a title.
trait NoTitle {
    fn x_axis_only(self) -> Self;
}
impl NoTitle for Chart {
    fn x_axis_only(mut self) -> Self {
        self.title = None;
        self.y_axis_title = None;
        self
    }
}

#[test]
fn stacked_bars_overlap_and_titles_toggle_auto_title() {
    let xml = sample(ChartKind::Column).grouping(Grouping::Stacked).to_xml();
    assert!(xml.contains(r#"<c:grouping val="stacked"/>"#));
    assert!(xml.contains(r#"<c:overlap val="100"/>"#));
    assert!(xml.contains(r#"<c:autoTitleDeleted val="false"/>"#));
    let untitled = Chart::new(ChartKind::Pie)
        .series(Series::new("s", [1.0]))
        .to_xml();
    assert!(untitled.contains(r#"<c:autoTitleDeleted val="true"/>"#));
    assert!(!untitled.contains("<c:catAx>"), "pie charts have no axes");
}

fn host_package() -> (Package, PartName) {
    let mut pkg = Package::new();
    let slide = PartName::new("/ppt/slides/slide1.xml").unwrap();
    pkg.add_part(slide.clone(), ct::PML_SLIDE, b"<p:sld/>".to_vec())
        .unwrap();
    (pkg, slide)
}

#[test]
fn embedded_workbook_holds_the_referenced_data() {
    for kind in KINDS {
        let (mut pkg, slide) = host_package();
        let chart = sample(kind);
        let inserted = insert_chart(&mut pkg, &slide, "/ppt", &chart, ChartData::EmbeddedWorkbook).unwrap();
        assert_eq!(inserted.part.as_str(), "/ppt/charts/chart1.xml");
        assert_eq!(pkg.part(&inserted.part).unwrap().content_type(), ct::CHART);
        assert_eq!(
            pkg.relationship_target(Some(&slide), &inserted.rel_id),
            Some(inserted.part.clone())
        );
        let wb_part = inserted.workbook.clone().unwrap();
        assert_eq!(
            wb_part.as_str(),
            "/ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx"
        );
        assert_eq!(pkg.part(&wb_part).unwrap().content_type(), XLSX_CONTENT_TYPE);
        assert_eq!(
            pkg.related_part(Some(&inserted.part), rel_types::PACKAGE),
            Some(wb_part.clone())
        );

        let chart_xml = std::str::from_utf8(pkg.part(&inserted.part).unwrap().data())
            .unwrap()
            .to_owned();
        assert_valid(&chart_xml);
        let rel = pkg
            .relationships(Some(&inserted.part))
            .unwrap()
            .first_by_type(rel_types::PACKAGE)
            .unwrap();
        assert!(
            chart_xml.contains(&format!(r#"<c:externalData r:id="{}">"#, rel.id)),
            "{chart_xml}"
        );

        // The embedded workbook is valid and holds the cached values at the referenced cells.
        let wb_bytes = pkg.part(&wb_part).unwrap().data().to_vec();
        let embedded = Package::from_bytes(&wb_bytes).unwrap();
        assert!(
            validate_package(&embedded).is_empty(),
            "{:?}",
            validate_package(&embedded)
        );
        let workbook = Workbook::from_bytes(&wb_bytes).unwrap();
        let sheet = workbook.worksheet(DATA_SHEET).unwrap();
        let info = read_chart(chart_xml.as_bytes()).unwrap();
        for s in &info.plots[0].series {
            let range = s.values_ref.as_deref().expect("reference to the embedded sheet");
            let cells = range.trim_start_matches("Sheet1!").replace('$', "");
            let (first, _) = cells.split_once(':').unwrap();
            let col: String = first.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
            for (i, v) in s.values.iter().enumerate() {
                let cell = sheet.cell(format!("{col}{}", i + 2).as_str()).unwrap();
                match v {
                    Some(x) => assert_eq!(cell, CellValue::Number(*x), "{kind:?} {col}{}", i + 2),
                    None => assert_eq!(cell, CellValue::Empty),
                }
            }
        }
        if kind != ChartKind::Scatter {
            assert_eq!(sheet.cell("A2").unwrap(), CellValue::Text("Q1".into()));
            assert_eq!(sheet.cell("B1").unwrap(), CellValue::Text("North".into()));
        }
    }
}

#[test]
fn charts_can_reference_host_cells() {
    let (mut pkg, slide) = host_package();
    let chart = Chart::new(ChartKind::Column)
        .categories(["a", "b"])
        .categories_ref("Data!$A$2:$A$3")
        .series(Series::new("v", [1.0, 2.0]).references(Some("Data!$B$1"), "Data!$B$2:$B$3", None));
    let inserted = insert_chart(&mut pkg, &slide, "/xl", &chart, ChartData::InChart).unwrap();
    assert!(inserted.workbook.is_none());
    let xml = std::str::from_utf8(pkg.part(&inserted.part).unwrap().data())
        .unwrap()
        .to_owned();
    assert_valid(&xml);
    assert!(
        xml.contains("<c:f>Data!$B$2:$B$3</c:f>") && xml.contains("<c:f>Data!$A$2:$A$3</c:f>"),
        "{xml}"
    );
    assert!(!xml.contains("externalData"));
    // A second chart gets the next part name.
    let second = insert_chart(&mut pkg, &slide, "/xl", &chart, ChartData::InChart).unwrap();
    assert_eq!(second.part.as_str(), "/xl/charts/chart2.xml");
    assert_ne!(second.rel_id, inserted.rel_id);
}

#[test]
fn reads_charts_written_by_office() {
    let read = |file: &str, part: &str| {
        let pkg = Package::open_path(fixture(file)).unwrap();
        read_chart(pkg.part(&PartName::new(part).unwrap()).unwrap().data()).unwrap()
    };
    let bar = read("poi/bar-chart.pptx", "/ppt/charts/chart1.xml");
    assert_eq!(bar.plots[0].kind, Some(ChartKind::Bar));
    assert_eq!(bar.plots[0].series[0].name.as_deref(), Some("Sales"));
    assert_eq!(
        bar.plots[0].series[0].categories,
        ["1st Qtr", "2nd Qtr", "3rd Qtr", "4th Qtr"]
    );
    let pie = read("poi/pie-chart.pptx", "/ppt/charts/chart1.xml");
    assert_eq!(
        pie.plots[0].series[0].values,
        [Some(8.2), Some(3.2), Some(1.4), Some(1.2)]
    );
    let line = read("poi/WithChart.xlsx", "/xl/charts/chart1.xml");
    assert_eq!(line.plots[0].kind, Some(ChartKind::Line));
    assert_eq!(line.plots[0].series.len(), 2);
    let word = read("poi/chartex.docx", "/word/charts/chart1.xml");
    assert_eq!(word.title.as_deref(), Some("my chart looks nice"));
    assert_eq!(word.plots[0].series.len(), 3);
    assert_eq!(
        word.plots[0].series[0].values,
        [Some(4.3), Some(2.5), Some(3.5), Some(4.5)]
    );
}

#[test]
fn graphic_data_references_the_chart() {
    let data = graphic_data("rId3");
    assert_eq!(chart_rel_id(&data).as_deref(), Some("rId3"));
    let xml =
        openxml_xml::RawElement::from_typed(&chart_reference("rId3"), openxml_xml::Ns::C, "chart").to_xml();
    assert!(xml.contains("rId3"));
}
