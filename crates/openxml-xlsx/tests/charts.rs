//! Charts on worksheets: built from cell ranges, referencing the cells,
//! schema-valid, readable back, and sharing the sheet's drawing with pictures.

mod common;

use common::assert_saved_valid;
use openxml_core::image::tiny_png;
use openxml_opc::Package;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_xlsx::{Anchor, AnchorPoint, CellRef, ChartKind, EditAs, Workbook};

fn anchor(from: &str, to: &str) -> Anchor {
    Anchor::TwoCell {
        from: AnchorPoint::at(CellRef::parse(from).unwrap()),
        to: AnchorPoint::at(CellRef::parse(to).unwrap()),
        edit_as: EditAs::TwoCell,
    }
}

fn fill(wb: &mut Workbook, sheet: &str) {
    let mut s = wb.worksheet_mut(sheet).unwrap();
    s.set_value("A1", "Month").unwrap();
    s.set_value("B1", "North").unwrap();
    s.set_value("C1", "South").unwrap();
    for (i, (m, n, so)) in [
        ("Jan", 10.0, 7.0),
        ("Feb", 12.5, 9.5),
        ("Mar", 9.0, 11.0),
        ("Apr", 14.0, 8.0),
    ]
    .into_iter()
    .enumerate()
    {
        let r = i + 2;
        s.set_value(format!("A{r}").as_str(), m).unwrap();
        s.set_value(format!("B{r}").as_str(), n).unwrap();
        s.set_value(format!("C{r}").as_str(), so).unwrap();
    }
}

#[test]
fn every_chart_kind_from_a_range() {
    let kinds = [
        ChartKind::Column,
        ChartKind::Bar,
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::Pie,
        ChartKind::Doughnut,
        ChartKind::Radar,
    ];
    let mut wb = Workbook::new();
    fill(&mut wb, "Sheet1");
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        for (i, kind) in kinds.iter().enumerate() {
            let chart = s
                .chart_from_range(*kind, "A1:C5")
                .unwrap()
                .title(format!("{kind:?}"));
            let top = 2 + 16 * i;
            s.add_chart(&chart, &anchor(&format!("E{top}"), &format!("L{}", top + 14)))
                .unwrap();
        }
    }
    let bytes = assert_saved_valid(&mut wb);
    let pkg = Package::from_bytes(&bytes).unwrap();
    for (name, part) in pkg.parts() {
        if part.content_type() == ct::CHART {
            let issues = openxml_schema::validate_xml(std::str::from_utf8(part.data()).unwrap())
                .unwrap()
                .unwrap();
            assert!(issues.is_empty(), "{name}: {issues:?}");
        }
    }
    let back = Workbook::from_bytes(&bytes).unwrap();
    let charts = back.worksheet("Sheet1").unwrap().charts().unwrap();
    assert_eq!(charts.len(), kinds.len());
    for (info, kind) in charts.iter().zip(kinds) {
        let plot = &info.plots[0];
        assert_eq!(plot.kind, Some(kind));
        assert_eq!(plot.series.len(), 2);
        assert_eq!(plot.series[0].name.as_deref(), Some("North"));
        assert_eq!(plot.series[0].categories, ["Jan", "Feb", "Mar", "Apr"]);
        assert_eq!(
            plot.series[1].values,
            [Some(7.0), Some(9.5), Some(11.0), Some(8.0)]
        );
        assert_eq!(plot.series[1].values_ref.as_deref(), Some("Sheet1!$C$2:$C$5"));
    }
    // One drawing part holds all charts; no workbook is embedded (the sheet is the data).
    let drawings: Vec<_> = pkg
        .parts()
        .filter(|(_, p)| p.content_type() == ct::DRAWING)
        .collect();
    assert_eq!(drawings.len(), 1);
    assert_eq!(
        pkg.related_parts(Some(drawings[0].0), rel_types::CHART).len(),
        kinds.len()
    );
    assert!(pkg.parts().all(|(n, _)| !n.as_str().contains("embeddings")));
}

#[test]
fn scatter_charts_and_quoted_sheet_names() {
    let mut wb = Workbook::new();
    wb.add_worksheet("Q1 Data").unwrap();
    {
        let mut s = wb.worksheet_mut("Q1 Data").unwrap();
        s.set_value("A1", "x").unwrap();
        s.set_value("B1", "y").unwrap();
        for i in 0..5 {
            s.set_value(format!("A{}", i + 2).as_str(), i as f64 * 0.5)
                .unwrap();
            s.set_value(format!("B{}", i + 2).as_str(), (i * i) as f64)
                .unwrap();
        }
        let chart = s.chart_from_range(ChartKind::Scatter, "A1:B6").unwrap();
        assert!(chart.categories.is_empty());
        s.add_chart(&chart, &anchor("D2", "J15")).unwrap();
    }
    let bytes = assert_saved_valid(&mut wb);
    let back = Workbook::from_bytes(&bytes).unwrap();
    let info = &back.worksheet("Q1 Data").unwrap().charts().unwrap()[0];
    let series = &info.plots[0].series[0];
    assert_eq!(series.values_ref.as_deref(), Some("'Q1 Data'!$B$2:$B$6"));
    assert_eq!(series.categories, ["0", "0.5", "1", "1.5", "2"], "x values");
    assert_eq!(
        series.values,
        [Some(0.0), Some(1.0), Some(4.0), Some(9.0), Some(16.0)]
    );
}

#[test]
fn charts_and_pictures_share_the_drawing() {
    let mut wb = Workbook::new();
    fill(&mut wb, "Sheet1");
    {
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.add_image("E2", &tiny_png(20, 10)).unwrap();
        let chart = s.chart_from_range(ChartKind::Line, "A1:C5").unwrap();
        s.add_chart(&chart, &anchor("E8", "L20")).unwrap();
    }
    let bytes = assert_saved_valid(&mut wb);
    let back = Workbook::from_bytes(&bytes).unwrap();
    let sheet = back.worksheet("Sheet1").unwrap();
    assert_eq!(sheet.images().unwrap().len(), 1);
    assert_eq!(sheet.charts().unwrap().len(), 1);
}

#[test]
fn chart_ranges_need_a_header_and_labels() {
    let mut wb = Workbook::new();
    fill(&mut wb, "Sheet1");
    let s = wb.worksheet("Sheet1").unwrap();
    assert!(s.chart_from_range(ChartKind::Column, "A1:A5").is_err());
    assert!(s.chart_from_range(ChartKind::Column, "A1:C1").is_err());
    assert!(s.charts().unwrap().is_empty());
}
