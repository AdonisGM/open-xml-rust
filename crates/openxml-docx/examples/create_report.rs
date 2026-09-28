//! Builds a sample report with most features of the API.
//!
//! ```text
//! cargo run -p openxml-docx --example create_report -- report.docx
//! ```

use openxml_docx::{
    Alignment, CellAlign, CoreProperties, Document, FontSize, HighlightColor, Length, ListKind, PageSetup,
    UnderlineStyle,
};

fn main() -> openxml_docx::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "report.docx".to_owned());
    let mut doc = Document::new();
    doc.set_page_setup(&PageSetup::a4());
    doc.set_core_properties(&CoreProperties {
        title: Some("Quarterly report".into()),
        creator: Some("openxml-rust".into()),
        ..doc.core_properties()?
    })?;

    doc.set_header("ACME Corporation — internal")?
        .set_alignment(Alignment::Right);
    doc.set_footer("Generated with openxml-docx")?
        .set_alignment(Alignment::Center);

    doc.add_heading("Quarterly report", 0)?;
    doc.add_heading("Summary", 1)?;
    let mut p = doc.add_paragraph("Revenue grew by ");
    p.add_run("12%").bold(true).color("2E7D32")?;
    p.add_text(" compared to the previous quarter. ");
    p.add_run("Costs")
        .italic(true)
        .underline(Some(UnderlineStyle::Single));
    p.add_text(" stayed ");
    p.add_run("flat").highlight(Some(HighlightColor::Yellow));
    p.add_text(".");
    p.set_alignment(Alignment::Justify);

    doc.add_heading("Highlights", 2)?;
    for item in [
        "New product line launched",
        "Two new markets",
        "Customer satisfaction at 94%",
    ] {
        doc.add_list_item(item, ListKind::Bullet, 0)?;
    }
    doc.add_list_item("Details are in the appendix", ListKind::Bullet, 1)?;

    doc.add_heading("Figures", 2)?;
    let data = [
        ("Region", "Q1", "Q2"),
        ("North", "120", "134"),
        ("South", "98", "110"),
        ("Total", "218", "244"),
    ];
    let mut table = doc.add_table(data.len(), 3)?;
    for (r, (a, b, c)) in data.iter().enumerate() {
        for (col, text) in [a, b, c].into_iter().enumerate() {
            let mut cell = table.cell(r, col)?;
            let mut para = cell.set_text(text);
            if col > 0 {
                para.set_alignment(Alignment::Right);
            }
            if r == 0 {
                cell.set_shading("D9E2F3")?
                    .set_vertical_alignment(CellAlign::Center);
            }
        }
    }
    table.set_header_row(0, true)?;
    table.set_column_widths(&[Length::cm(6.0), Length::cm(4.0), Length::cm(4.0)])?;

    doc.add_heading("Next steps", 2)?;
    for step in ["Hire two engineers", "Open the Hanoi office", "Review pricing"] {
        doc.add_list_item(step, ListKind::Numbered, 0)?;
    }
    let mut p = doc.add_paragraph("Questions: see ");
    p.add_hyperlink("the intranet", "https://intranet.example.com/reports")?;
    p.add_run(" or write to the finance team.").size(FontSize(9.0));

    doc.add_page_break();
    doc.add_heading("Appendix", 1)?;
    doc.add_paragraph("Company logo:");
    doc.add_picture(&openxml_core::image::tiny_png(120, 60), Length::cm(4.0))?;

    doc.save(&path)?;
    println!(
        "wrote {path} ({} paragraphs, {} tables)",
        doc.paragraphs().len(),
        doc.tables().len()
    );
    Ok(())
}
