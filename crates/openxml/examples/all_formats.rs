//! Creates a Word document, an Excel workbook and a PowerPoint deck that
//! share the same data, using the facade crate.
//!
//! ```text
//! cargo run -p openxml --example all_formats -- out/
//! ```

use openxml::core::FontSize;
use openxml::docx::{Document, ListKind};
use openxml::pptx::{LayoutKind, Presentation};
use openxml::xlsx::{CellStyle, Workbook};
use openxml::{Length, Result};

const SALES: &[(&str, f64)] = &[
    ("North", 134.0),
    ("South", 110.0),
    ("East", 98.5),
    ("West", 121.25),
];

fn main() -> Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir)?;

    // Word
    let mut doc = Document::new();
    doc.add_heading("Sales by region", 1)?;
    doc.add_paragraph("Figures for the second quarter.");
    for (region, value) in SALES {
        doc.add_list_item(&format!("{region}: {value}"), ListKind::Bullet, 0)?;
    }
    let mut table = doc.add_table(SALES.len() + 1, 2)?;
    table.cell(0, 0)?.set_text("Region");
    table.cell(0, 1)?.set_text("Sales");
    for (i, (region, value)) in SALES.iter().enumerate() {
        table.cell(i + 1, 0)?.set_text(region);
        table.cell(i + 1, 1)?.set_text(&value.to_string());
    }
    doc.save(dir.join("sales.docx"))?;

    // Excel
    let mut wb = Workbook::new();
    let bold = wb.add_style(&CellStyle::new().bold());
    {
        let mut sheet = wb.worksheet_mut("Sheet1")?;
        sheet.set_value("A1", "Region")?;
        sheet.set_value("B1", "Sales")?;
        sheet.set_cell_style("A1", bold)?;
        sheet.set_cell_style("B1", bold)?;
        for (i, (region, value)) in SALES.iter().enumerate() {
            let row = i + 2;
            sheet.set_value(format!("A{row}").as_str(), *region)?;
            sheet.set_value(format!("B{row}").as_str(), *value)?;
        }
        let total = SALES.len() + 2;
        sheet.set_value(format!("A{total}").as_str(), "Total")?;
        sheet.set_formula(format!("B{total}").as_str(), &format!("SUM(B2:B{})", total - 1))?;
    }
    wb.save(dir.join("sales.xlsx"))?;

    // PowerPoint
    let mut deck = Presentation::new();
    {
        let mut title = deck.add_slide(LayoutKind::Title)?;
        title.set_title("Sales by region")?;
        title.set_subtitle("Second quarter")?;
    }
    {
        let mut slide = deck.add_slide(LayoutKind::TitleOnly)?;
        slide.set_title("Figures")?;
        let mut t = slide.add_table(
            SALES.len() + 1,
            2,
            Length::cm(3.0),
            Length::cm(5.0),
            Length::cm(16.0),
            Length::cm(6.0),
        )?;
        t.set_cell_text(0, 0, "Region")?;
        t.set_cell_text(0, 1, "Sales")?;
        for (i, (region, value)) in SALES.iter().enumerate() {
            t.set_cell_text(i + 1, 0, region)?;
            t.set_cell_text(i + 1, 1, &value.to_string())?;
        }
        slide
            .add_text_box(
                Length::cm(3.0),
                Length::cm(12.0),
                Length::cm(16.0),
                Length::cm(2.0),
                "Generated with openxml",
            )
            .font_size(FontSize(14.0));
    }
    deck.save(dir.join("sales.pptx"))?;

    println!("wrote sales.docx, sales.xlsx and sales.pptx to {}", dir.display());
    Ok(())
}
