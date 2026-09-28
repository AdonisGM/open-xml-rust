//! A worksheet with data and a chart built from it.
//!
//! ```text
//! cargo run -p openxml-xlsx --example chart_workbook -- chart.xlsx
//! ```

use openxml_xlsx::{Anchor, AnchorPoint, CellRef, ChartKind, EditAs, LegendPosition, Workbook};

fn main() -> openxml_core::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "chart.xlsx".into());
    let mut wb = Workbook::new();
    {
        let mut s = wb.worksheet_mut("Sheet1")?;
        s.set_value("A1", "Month")?;
        s.set_value("B1", "North")?;
        s.set_value("C1", "South")?;
        for (i, (m, n, so)) in [
            ("Jan", 10.0, 7.0),
            ("Feb", 12.5, 9.5),
            ("Mar", 9.0, 11.0),
            ("Apr", 14.0, 8.0),
        ]
        .into_iter()
        .enumerate()
        {
            s.set_value(format!("A{}", i + 2).as_str(), m)?;
            s.set_value(format!("B{}", i + 2).as_str(), n)?;
            s.set_value(format!("C{}", i + 2).as_str(), so)?;
        }
        let chart = s
            .chart_from_range(ChartKind::Column, "A1:C5")?
            .title("Units sold")
            .legend(Some(LegendPosition::Bottom));
        let anchor = Anchor::TwoCell {
            from: AnchorPoint::at(CellRef::parse("A7")?),
            to: AnchorPoint::at(CellRef::parse("H22")?),
            edit_as: EditAs::TwoCell,
        };
        s.add_chart(&chart, &anchor)?;
    }
    wb.save(&path)?;
    println!("wrote {path}");
    Ok(())
}
