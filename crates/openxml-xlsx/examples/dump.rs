//! Prints the sheets and cell values of workbooks.
//!
//! ```text
//! cargo run -p openxml-xlsx --example dump -- [--rows N] book.xlsx [more.xlsx …]
//! ```

use openxml_xlsx::{CellValue, Workbook};

fn main() -> openxml_xlsx::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut max_rows = 20usize;
    if args.first().map(String::as_str) == Some("--rows") && args.len() > 1 {
        max_rows = args[1].parse().unwrap_or(20);
        args.drain(..2);
    }
    if args.is_empty() {
        eprintln!("usage: dump [--rows N] <file.xlsx>...");
        std::process::exit(2);
    }
    for path in &args {
        println!("######## {path}");
        let wb = match Workbook::open(path) {
            Ok(wb) => wb,
            Err(e) => {
                println!("error: {e}");
                continue;
            }
        };
        println!(
            "date system: {:?}, shared strings: {}",
            wb.date_system(),
            wb.shared_string_count()
        );
        for name in wb.sheet_names() {
            println!("== {name} ({:?})", wb.sheet_kind(&name).expect("listed"));
            let Ok(sheet) = wb.worksheet(&name) else { continue };
            let mut dates = 0;
            let mut formulas = 0;
            let mut cells = 0;
            for row in sheet.rows() {
                for (_, v) in row.cells() {
                    cells += 1;
                    if matches!(v.result(), CellValue::DateTime(_)) {
                        dates += 1;
                    }
                    if matches!(v, CellValue::Formula { .. }) {
                        formulas += 1;
                    }
                }
            }
            println!(
                "   dimension {:?}, used {:?}, cells {cells}, dates {dates}, formulas {formulas}, merges {}",
                sheet.dimension().map(|d| d.to_string()),
                sheet.used_range().map(|d| d.to_string()),
                sheet.merged_ranges().len()
            );
            for row in sheet.rows().take(max_rows) {
                let cells: Vec<String> = row.cells().map(|(r, v)| format!("{r}={v:?}")).collect();
                println!("   {:>4}: {}", row.index(), cells.join(" | "));
            }
        }
    }
    Ok(())
}
