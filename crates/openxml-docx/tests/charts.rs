//! Charts in Word documents: every chart kind, inline and floating, in the
//! body and in a header; all parts validate and charts read back.

mod common;

use common::save_validate_reopen;
use openxml_docx::{
    Chart, ChartKind, Document, Floating, HorizontalAlignment, HorizontalAnchor, HorizontalPosition, Length,
    Series, VerticalAnchor, VerticalPosition, Wrap,
};
use openxml_opc::Package;
use openxml_opc::known::{content_types as ct, rel_types};

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

fn chart(kind: ChartKind) -> Chart {
    Chart::new(kind)
        .title(format!("{kind:?}"))
        .categories(["Jan", "Feb", "Mar"])
        .series(Series::new("Sales", [4.0, 7.5, 6.0]).color(0x5B9BD5))
        .series(Series::new("Costs", [3.0, 4.0, 5.5]).color(0xA5A5A5))
}

#[test]
fn every_chart_kind_in_the_body() {
    let mut doc = Document::new();
    doc.add_heading("Charts", 1).unwrap();
    for kind in KINDS {
        doc.add_chart(&chart(kind), Length::cm(15.0), Length::cm(7.5))
            .unwrap();
    }
    let back = save_validate_reopen(&mut doc);
    let charts = back.charts().unwrap();
    assert_eq!(charts.len(), KINDS.len());
    for (info, kind) in charts.iter().zip(KINDS) {
        assert_eq!(info.plots[0].kind, Some(kind));
        assert_eq!(info.title.as_deref(), Some(format!("{kind:?}").as_str()));
        assert_eq!(info.plots[0].series[0].values, [Some(4.0), Some(7.5), Some(6.0)]);
    }
    let pkg = Package::from_bytes(&doc.to_bytes().unwrap()).unwrap();
    let chart_parts: Vec<_> = pkg
        .parts()
        .filter(|(_, p)| p.content_type() == ct::CHART)
        .map(|(n, _)| n.clone())
        .collect();
    assert_eq!(chart_parts.len(), KINDS.len());
    for part in &chart_parts {
        assert!(part.as_str().starts_with("/word/charts/chart"));
        let wb = pkg.related_part(Some(part), rel_types::PACKAGE).unwrap();
        assert!(wb.as_str().starts_with("/word/embeddings/"));
    }
    // Drawing ids stay unique and the text of the document is unaffected.
    assert_eq!(back.text(), "Charts\n\n\n\n\n\n\n\n");
}

#[test]
fn floating_chart_and_chart_in_a_header() {
    let mut doc = Document::new();
    let floating = Floating::new(
        HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Right),
        VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::cm(0.5)),
        Wrap::Square,
    );
    {
        let mut p = doc.add_paragraph("Text flows around the chart on the right.");
        p.add_chart(
            &chart(ChartKind::Pie),
            Length::cm(6.0),
            Length::cm(6.0),
            Some(&floating),
        )
        .unwrap();
    }
    {
        let mut header = doc.set_header("Report").unwrap();
        header
            .add_chart(&chart(ChartKind::Line), Length::cm(4.0), Length::cm(1.5), None)
            .unwrap();
    }
    let back = save_validate_reopen(&mut doc);
    assert_eq!(back.charts().unwrap().len(), 1, "body charts only");
    let pkg = Package::from_bytes(&doc.to_bytes().unwrap()).unwrap();
    let header = pkg
        .parts()
        .find(|(_, p)| p.content_type() == ct::WML_HEADER)
        .map(|(n, _)| n.clone())
        .unwrap();
    assert_eq!(
        pkg.related_parts(Some(&header), rel_types::CHART).len(),
        1,
        "the header relates its own chart"
    );
    let xml = String::from_utf8(pkg.part(&pkg.main_part().unwrap()).unwrap().data().to_vec()).unwrap();
    assert!(
        xml.contains("<wp:anchor") && xml.contains("<wp:wrapSquare"),
        "floating frame"
    );
}
