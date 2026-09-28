//! Creates a small sales report workbook: styles, formulas, a table,
//! conditional formatting, data validation, a comment, a hyperlink, a
//! picture, rich text, print settings and sheet protection.
//!
//! ```text
//! cargo run -p openxml-xlsx --example create_workbook -- report.xlsx
//! ```

use openxml_opc::CoreProperties;
use openxml_xlsx::print::{hf, paper};
use openxml_xlsx::{
    Border, BorderStyle, CellRange, CellStyle, CellValue, CfOperator, Color, ConditionalFormat,
    DataValidation, DateTime, HeaderFooter, HorizontalAlignment, Image, Length, LinkTarget, NumberFormat,
    Orientation, PageSetup, RichText, SheetProtection, Table, Workbook,
};

fn main() -> openxml_xlsx::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "report.xlsx".into());
    let mut wb = Workbook::new();
    wb.set_core_properties(&CoreProperties {
        title: Some("Monthly sales".into()),
        creator: Some("openxml-rust example".into()),
        ..Default::default()
    })?;

    let title = wb.add_style(&CellStyle::new().bold().font_size(16.0));
    let header = wb.add_style(
        &CellStyle::new()
            .bold()
            .font_color(Color::Rgb(0xFF, 0xFF, 0xFF))
            .fill_color(Color::Rgb(0x44, 0x72, 0xC4))
            .border(Border::all(BorderStyle::Thin, None))
            .horizontal(HorizontalAlignment::Center),
    );
    let money = wb.add_style(&CellStyle::new().number_format(NumberFormat::custom("#,##0.00")));
    let total = wb.add_style(
        &CellStyle::new()
            .bold()
            .number_format(NumberFormat::custom("#,##0.00")),
    );

    let rows = [
        ("Coffee", DateTime::from_ymd(2024, 5, 2), 12, 3.5),
        ("Tea", DateTime::from_ymd(2024, 5, 3), 30, 2.25),
        ("Cake", DateTime::from_ymd(2024, 5, 7), 8, 4.75),
        ("Juice", DateTime::from_ymd(2024, 5, 9), 15, 3.0),
    ];

    {
        let mut sheet = wb.worksheet_mut("Sheet1")?;
        sheet.set_value("A1", "Sales report — May 2024")?;
        sheet.set_cell_style("A1", title)?;
        sheet.merge_cells(CellRange::parse("A1:E1")?)?;
        for (i, h) in ["Product", "Date", "Quantity", "Unit price", "Amount"]
            .iter()
            .enumerate()
        {
            let cell = (3, i as u32 + 1);
            sheet.set_value(cell, *h)?;
            sheet.set_cell_style(cell, header)?;
        }
        for (i, (product, date, qty, price)) in rows.iter().enumerate() {
            let r = i as u32 + 4;
            sheet.set_value((r, 1), *product)?;
            sheet.set_value((r, 2), date.expect("valid date"))?;
            sheet.set_value((r, 3), *qty)?;
            sheet.set_value((r, 4), *price)?;
            sheet.set_cell_style((r, 4), money)?;
            sheet.set_value(
                (r, 5),
                CellValue::formula_with_result(format!("C{r}*D{r}"), f64::from(*qty) * price),
            )?;
            sheet.set_cell_style((r, 5), money)?;
        }
        let last = rows.len() as u32 + 3;
        sheet.set_value((last + 1, 4), "Total")?;
        sheet.set_formula((last + 1, 5), &format!("SUM(E4:E{last})"))?;
        sheet.set_cell_style((last + 1, 5), total)?;
        for (col, width) in [(1, 16.0), (2, 12.0), (3, 10.0), (4, 12.0), (5, 14.0)] {
            sheet.set_column_width(col, width)?;
        }
        sheet.freeze_panes("A4")?;

        // The data as a table with a style and filter buttons.
        sheet.add_table(
            "A3:E7",
            &Table::new().name("Sales").style(Some("TableStyleMedium9")),
        )?;
        // Highlight large quantities and draw bars for the amounts.
        let green = CellStyle::new()
            .bold()
            .font_color(Color::Rgb(0x00, 0x61, 0x00))
            .fill_color(Color::Rgb(0xC6, 0xEF, 0xCE));
        sheet.add_conditional_format(
            "C4:C7",
            &ConditionalFormat::cell_is(CfOperator::GreaterThan, "10").style(green),
        )?;
        sheet.add_conditional_format(
            "E4:E7",
            &ConditionalFormat::data_bar(Color::Rgb(0x63, 0x8E, 0xC6)),
        )?;
        // Only known products can be typed into the product column.
        sheet.add_data_validation(
            "A4:A7",
            &DataValidation::list(["Coffee", "Tea", "Cake", "Juice"])?
                .input_message("Product", "Pick a product"),
        )?;
        sheet.add_comment(
            (last + 1, 5),
            "openxml-rust",
            "Sum of the Amount column (cached value computed by Workbook::calculate).",
        )?;
        sheet.set_rich_text(
            "A2",
            &RichText::new()
                .push("Prepared with ")
                .bold("openxml-rust")
                .italic(" (example)"),
        )?;
        sheet.set_link(
            (last + 3, 1),
            "ECMA-376 (Office Open XML)",
            LinkTarget::Url(
                "https://ecma-international.org/publications-and-standards/standards/ecma-376/".into(),
            ),
        )?;
        // A picture (a generated 120 × 40 PNG) to the right of the table.
        let png = openxml_core::image::tiny_png(120, 40);
        sheet.add_image_with(
            &png,
            &Image::at("G3")?
                .width(Length::cm(3.2))
                .description("Logo placeholder"),
        )?;
        sheet.set_tab_color(Some(Color::Rgb(0x44, 0x72, 0xC4)));
        // Print on one landscape A4 page with headers and footers.
        sheet.set_page_setup(&PageSetup {
            orientation: Some(Orientation::Landscape),
            paper_size: Some(paper::A4),
            ..PageSetup::default()
        })?;
        sheet.fit_to_pages(1, 0)?;
        sheet.set_header_footer(
            &HeaderFooter::new()
                .header(hf::sections("", "&BSales report", hf::DATE))
                .footer(hf::sections(
                    hf::SHEET,
                    "",
                    &format!("Page {} of {}", hf::PAGE, hf::PAGES),
                )),
        )?;
        sheet.set_print_titles(Some((3, 3)), None)?;
    }
    wb.rename_worksheet("Sheet1", "May")?;
    wb.set_defined_name("Amounts", "May!$E$4:$E$7", None)?;

    // A second, larger sheet written row by row.
    let mut log = wb.add_streaming_worksheet("Log")?;
    log.write_row(["#", "value"])?;
    for i in 1..=1000 {
        log.write_row([CellValue::from(i), CellValue::from(f64::from(i).sqrt())])?;
    }
    log.finish()?;
    wb.worksheet_mut("Log")?.protect(&SheetProtection::new())?;

    // Store computed results for the formulas (Excel recalculates anyway).
    let report = wb.calculate()?;
    println!("calculated {} formulas", report.calculated);

    wb.save(&path)?;
    println!("wrote {path}");
    Ok(())
}
