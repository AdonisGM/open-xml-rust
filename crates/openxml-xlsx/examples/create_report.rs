//! Creates a small sales report workbook.
//!
//! ```text
//! cargo run -p openxml-xlsx --example create_report -- report.xlsx
//! ```

use openxml_opc::CoreProperties;
use openxml_xlsx::{
    Border, BorderStyle, CellRange, CellStyle, CellValue, Color, DateTime, HorizontalAlignment, NumberFormat,
    Workbook,
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

    wb.save(&path)?;
    println!("wrote {path}");
    Ok(())
}
