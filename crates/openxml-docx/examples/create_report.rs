//! Builds a sample report with most features of the API.
//!
//! ```text
//! cargo run -p openxml-docx --example create_report -- report.docx
//! ```

use openxml_docx::{
    Alignment, Border, CellAlign, CellMargins, Columns, ContentControl, ContentControlKind, CoreProperties,
    Document, Field, Floating, FontSize, HeaderFooterType, HighlightColor, HorizontalAlignment,
    HorizontalAnchor, HorizontalPosition, Length, LineSpacing, LinkTarget, ListDefinition, ListItem,
    ListKind, ListLevel, Math, NewComment, NoteProperties, NumberFormat, PageBorders, PageNumbering,
    PageSetup, ParagraphBorders, ParagraphFormat, PictureOptions, PropertyValue, Protection, RevisionInfo,
    RunFormat, SectionBreak, ShapeKind, ShapeOptions, StyleDefinition, TabAlignment, TabLeader, TabStop,
    TableAlignment, TableBorders, TableOfContents, TextSpan, UnderlineStyle, VerticalAnchor,
    VerticalPosition, Wrap,
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
    doc.set_custom_property("Department", PropertyValue::Text("Finance".into()))?;
    doc.set_custom_property("Quarter", PropertyValue::Integer(2))?;
    doc.set_zoom(110)?;

    // Custom styles.
    let mut callout = StyleDefinition::paragraph("Callout", "Callout");
    callout.paragraph = ParagraphFormat {
        borders: Some(ParagraphBorders::around(
            Border::single(1.0, "2F5496").with_space(4),
        )),
        shading: Some("DEEAF6".into()),
        ..Default::default()
    };
    callout.run = RunFormat {
        italic: Some(true),
        ..Default::default()
    };
    doc.add_style(&callout)?;

    // Header, footer with page numbers, watermark.
    doc.set_header("ACME Corporation — internal")?
        .set_alignment(Alignment::Right);
    let mut footer = doc.set_footer("Page ")?;
    footer.set_alignment(Alignment::Center);
    footer.add_field(&Field::Page, "1");
    footer.add_text(" of ");
    footer.add_field(&Field::NumPages, "1");
    doc.set_watermark("DRAFT")?;

    doc.add_heading("Quarterly report", 0)?;
    doc.add_paragraph("Prepared on ")
        .add_simple_field(&Field::Date(Some("d MMMM yyyy".into())), "1 July 2024")
        .add_run(" (preliminary)")
        .size(FontSize(9.0));

    doc.add_heading("Summary", 1)?;
    let mut p = doc.add_paragraph("Revenue grew by ");
    p.add_run("12%").bold(true).color("2E7D32")?;
    p.add_text(" compared to the previous quarter");
    p.add_footnote("Unaudited figures.")?;
    p.add_text(". ");
    p.add_run("Costs")
        .italic(true)
        .underline(Some(UnderlineStyle::Single));
    p.add_text(" stayed ");
    p.add_run("flat").highlight(Some(HighlightColor::Yellow));
    p.add_text(".");
    p.set_alignment(Alignment::Justify)
        .set_line_spacing(LineSpacing::Multiple(1.15));
    let summary = doc.paragraphs().len() - 1;
    doc.add_comment(
        summary,
        TextSpan::Text("12%"),
        &NewComment::new("Ann Lee", "Check with finance."),
    )?;
    doc.add_paragraph("Figures are preliminary.")
        .set_style("Callout")?;

    // A logo floating at the right margin.
    let mut logo = PictureOptions::new(Length::cm(3.0));
    logo.description = Some("ACME logo".into());
    logo.floating = Some(Floating::new(
        HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Right),
        VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::ZERO),
        Wrap::Square,
    ));
    doc.add_paragraph("Our logo floats next to this paragraph, text wraps around it.")
        .add_picture_with(&openxml_core::image::tiny_png(120, 60), &logo)?;

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
    table
        .set_borders(&TableBorders::all(Border::single(0.5, "8EAADB")))?
        .set_cell_margins(&CellMargins::all(Length::pt(3.0)))
        .set_alignment(TableAlignment::Center);

    doc.add_heading("Growth model", 2)?;
    doc.add_paragraph("Compound growth over n quarters:");
    doc.add_equation(&Math::row([
        Math::text("V="),
        Math::subscript(Math::text("V"), Math::text("0")),
        Math::superscript(
            Math::parens(Math::row([
                Math::text("1+"),
                Math::frac(Math::text("r"), Math::text("100")),
            ])),
            Math::text("n"),
        ),
    ]));

    doc.add_heading("Next steps", 2)?;
    let mut steps = ListDefinition::numbered();
    steps.levels[0] = ListLevel::numbered(NumberFormat::UpperLetter, "%1)", 0);
    let steps = doc.add_list_definition(&steps)?;
    for step in ["Hire two engineers", "Open the Hanoi office", "Review pricing"] {
        doc.add_list_paragraph(step, steps, 0)?;
    }
    let mut p = doc.add_paragraph("Owner:\t");
    p.add_tab_stop(TabStop::new(Length::cm(4.0), TabAlignment::Left).with_leader(TabLeader::Dot));
    let owner = ContentControl::new(
        ContentControlKind::DropDown(vec![
            ListItem::new("Finance", "fin"),
            ListItem::new("Sales", "sales"),
        ]),
        "owner",
    );
    p.add_content_control(&owner, "fin")?;
    let mut p = doc.add_paragraph("Approved ");
    p.add_checkbox("Approved", false)?;
    let mut p = doc.add_paragraph("Questions: see ");
    p.add_hyperlink("the intranet", "https://intranet.example.com/reports")?;
    p.add_text(" or ");
    p.add_link(
        "write to the finance team",
        &LinkTarget::Email {
            address: "finance@example.com".into(),
            subject: Some("Quarterly report".into()),
        },
        Some("Send an e-mail"),
    )?;
    let mut p = doc.add_paragraph("Revised: ");
    p.add_deletion("next week", &RevisionInfo::new("Bob"));
    p.add_insertion("tomorrow", &RevisionInfo::new("Bob"));

    // Appendix in its own section: landscape, two columns, roman numbering.
    let mut appendix = doc.add_section(SectionBreak::NextPage);
    appendix
        .set_page_setup(&PageSetup::a4().landscape())
        .set_columns(&Columns {
            count: 2,
            spacing: Length::cm(1.0),
            separator: true,
        })
        .set_page_numbering(&PageNumbering {
            format: Some(NumberFormat::LowerRoman),
            start: Some(1),
        })
        .set_page_borders(Some(&PageBorders::around(
            Border::single(0.5, "BFBFBF").with_space(24),
        )))?;
    appendix.set_header(HeaderFooterType::Default, "Appendix")?;
    doc.add_heading("Appendix", 1)?;
    doc.add_paragraph("Company logo:");
    doc.add_picture(&openxml_core::image::tiny_png(120, 60), Length::cm(4.0))?;
    let mut note = ShapeOptions::new(Length::cm(6.0), Length::cm(2.0));
    note.fill = Some("FFF2CC".into());
    doc.add_paragraph("")
        .add_text_box(&note, |content| {
            content.add_paragraph("Text boxes hold ordinary paragraphs.");
            Ok(())
        })?
        .add_shape(
            ShapeKind::Ellipse,
            &ShapeOptions::new(Length::cm(1.0), Length::cm(1.0)),
        )?;

    doc.insert_table_of_contents(1, &TableOfContents::default())?;
    doc.set_footnote_properties(&NoteProperties {
        number_format: Some(NumberFormat::LowerLetter),
        ..Default::default()
    })?;
    doc.protect(Protection::TrackedChanges, None)?;
    doc.update_statistics()?;

    doc.save(&path)?;
    println!(
        "wrote {path} ({} paragraphs, {} tables, {} sections, {} comments)",
        doc.paragraphs().len(),
        doc.tables().len(),
        doc.sections().len(),
        doc.comments().len()
    );
    Ok(())
}
