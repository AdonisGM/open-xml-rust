//! A deck with one chart per chart kind.
//!
//! ```text
//! cargo run -p openxml-pptx --example chart_deck -- charts.pptx
//! ```

use openxml_core::Length;
use openxml_pptx::{Chart, ChartKind, LayoutKind, LegendPosition, Presentation, Series};

fn main() -> openxml_core::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "charts.pptx".into());
    let mut deck = Presentation::new();
    for kind in [
        ChartKind::Column,
        ChartKind::Pie,
        ChartKind::Line,
        ChartKind::Scatter,
    ] {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title(&format!("{kind:?} chart"))?;
        let chart = Chart::new(kind)
            .categories(["Q1", "Q2", "Q3", "Q4"])
            .series(Series::new("North", [12.0, 15.0, 11.0, 18.0]).color(0x2E75B6))
            .series(Series::new("South", [9.0, 11.5, 14.0, 13.0]).color(0xC55A11))
            .legend(Some(LegendPosition::Bottom))
            .data_labels(kind == ChartKind::Pie);
        slide.add_chart(
            &chart,
            Length::cm(2.0),
            Length::cm(4.0),
            Length::cm(29.0),
            Length::cm(14.0),
        )?;
    }
    deck.save(&path)?;
    println!("wrote {path}");
    Ok(())
}
