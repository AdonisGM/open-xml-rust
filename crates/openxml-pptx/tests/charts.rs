//! Charts on slides: every chart kind produces a valid, consistent deck
//! whose chart parts and embedded workbooks read back with the source data.

mod common;

use common::{check_saved, round_trip};
use openxml_core::Length;
use openxml_opc::Package;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_pptx::{Chart, ChartKind, LayoutKind, Presentation, Series, ShapeKind};

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
        .title(format!("{kind:?} chart"))
        .categories(["Q1", "Q2", "Q3"])
        .series(Series::new("North", [3.0, 5.0, 4.0]).color(0x4472C4))
        .series(Series::new("South", [2.0, 6.0, 3.5]).color(0xED7D31))
        .data_labels(true)
}

#[test]
fn every_chart_kind_on_its_own_slide() {
    let mut deck = Presentation::new();
    for kind in KINDS {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly).unwrap();
        slide.set_title(&format!("{kind:?}")).unwrap();
        let id = slide
            .add_chart(
                &chart(kind),
                Length::cm(2.0),
                Length::cm(4.0),
                Length::cm(22.0),
                Length::cm(12.0),
            )
            .unwrap();
        assert!(id > 1);
    }
    let back = round_trip(&mut deck);
    for (i, kind) in KINDS.iter().enumerate() {
        let charts = back.slide_charts(i).unwrap();
        assert_eq!(charts.len(), 1);
        assert_eq!(
            charts[0].title.as_deref(),
            Some(format!("{kind:?} chart").as_str())
        );
        assert_eq!(charts[0].plots[0].kind, Some(*kind));
        assert_eq!(
            charts[0].plots[0].series[1].values,
            [Some(2.0), Some(6.0), Some(3.5)]
        );
        let shapes = back.slide(i).unwrap().shapes();
        assert!(
            shapes.iter().any(|s| s.kind == ShapeKind::Chart),
            "{kind:?}: {shapes:?}"
        );
    }
    // Eight chart parts, each with its own embedded workbook.
    let bytes = deck.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    let charts: Vec<_> = pkg
        .parts()
        .filter(|(_, p)| p.content_type() == ct::CHART)
        .collect();
    assert_eq!(charts.len(), 8);
    for (name, _) in charts {
        let wb = pkg
            .related_part(Some(name), rel_types::PACKAGE)
            .expect("embedded workbook");
        assert!(
            pkg.part(&wb).unwrap().data().starts_with(b"PK"),
            "{wb} is a ZIP package"
        );
    }
}

#[test]
fn several_charts_on_one_slide_and_removal() {
    let mut deck = Presentation::new();
    {
        let mut slide = deck.add_slide(LayoutKind::Blank).unwrap();
        for (i, kind) in [ChartKind::Pie, ChartKind::Line].into_iter().enumerate() {
            let x = Length::cm(1.0 + 12.0 * i as f64);
            slide
                .add_chart(
                    &chart(kind),
                    x,
                    Length::cm(2.0),
                    Length::cm(11.0),
                    Length::cm(9.0),
                )
                .unwrap();
        }
    }
    deck.add_slide(LayoutKind::Blank).unwrap();
    let saved = check_saved(&deck.to_bytes().unwrap());
    let kinds: Vec<_> = saved
        .slide_charts(0)
        .unwrap()
        .iter()
        .map(|c| c.plots[0].kind)
        .collect();
    assert_eq!(kinds, [Some(ChartKind::Pie), Some(ChartKind::Line)]);
    assert!(saved.slide_charts(1).unwrap().is_empty());
    assert!(saved.slide_charts(5).is_err());

    // Removing the slide drops its charts and their workbooks.
    deck.remove_slide(0).unwrap();
    let pkg = Package::from_bytes(&deck.to_bytes().unwrap()).unwrap();
    assert!(pkg.parts().all(|(_, p)| p.content_type() != ct::CHART));
    assert!(pkg.parts().all(|(n, _)| !n.as_str().contains("/embeddings/")));
}
