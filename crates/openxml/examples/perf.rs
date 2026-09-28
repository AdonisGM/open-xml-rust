//! Performance scenarios for the document APIs, one per process so that
//! peak memory can be measured externally (`/usr/bin/time -l` on macOS,
//! `/usr/bin/time -v` on Linux).
//!
//! ```text
//! cargo build --release -p openxml --example perf
//! /usr/bin/time -l target/release/examples/perf xlsx-write-api out/perf
//! ```

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Instant;

use openxml::docx::Document;
use openxml::pptx::{LayoutKind, Presentation};
use openxml::xlsx::{CellValue, Workbook};

const ROWS: u32 = 100_000;
const COLS: u32 = 10;
const PARAGRAPHS: usize = 100_000;
const SLIDES: usize = 500;

fn value(r: u32, c: u32) -> CellValue {
    if c.is_multiple_of(2) {
        CellValue::Number(f64::from(r) * 10.0 + f64::from(c))
    } else {
        CellValue::Text(format!("item {}", r % 1000))
    }
}

fn file_size(p: &Path) -> String {
    let bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn main() -> openxml::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let scenario = args.get(1).map(String::as_str).unwrap_or("");
    let dir = PathBuf::from(args.get(2).map(String::as_str).unwrap_or("out/perf"));
    std::fs::create_dir_all(&dir)?;
    let t = Instant::now();
    let detail = match scenario {
        "xlsx-write-api" => {
            let mut wb = Workbook::new();
            {
                let mut s = wb.worksheet_mut("Sheet1")?;
                for r in 1..=ROWS {
                    for c in 1..=COLS {
                        s.set_value((r, c), value(r, c))?;
                    }
                }
            }
            let p = dir.join("api.xlsx");
            wb.save(&p)?;
            format!("{} cells → {}", ROWS * COLS, file_size(&p))
        }
        "xlsx-write-stream" => {
            let mut wb = Workbook::new();
            {
                let mut s = wb.add_streaming_worksheet("Data")?;
                for r in 1..=ROWS {
                    s.write_row((1..=COLS).map(|c| value(r, c)))?;
                }
                s.finish()?;
            }
            let p = dir.join("stream.xlsx");
            wb.save(&p)?;
            format!("{} cells → {}", ROWS * COLS, file_size(&p))
        }
        "xlsx-read-model" => {
            let wb = Workbook::open(dir.join("api.xlsx"))?;
            let s = wb.worksheet("Sheet1")?;
            let (mut cells, mut sum) = (0usize, 0.0);
            for row in s.rows() {
                for (_, v) in row.cells() {
                    cells += 1;
                    if let CellValue::Number(n) = v {
                        sum += n;
                    }
                }
            }
            format!("{cells} cells, sum {sum}")
        }
        "xlsx-read-rows" => {
            let wb = Workbook::open(dir.join("stream.xlsx"))?;
            let (mut cells, mut sum) = (0usize, 0.0);
            wb.for_each_row("Data", |_, row| {
                for (_, v) in row {
                    cells += 1;
                    if let CellValue::Number(n) = v {
                        sum += n;
                    }
                }
                ControlFlow::Continue(())
            })?;
            format!("{cells} cells, sum {sum}")
        }
        "docx-write" => {
            let mut doc = Document::new();
            for i in 0..PARAGRAPHS {
                let mut p = doc.add_paragraph(&format!(
                    "Paragraph {i}: the quick brown fox jumps over the lazy dog. "
                ));
                p.add_run("Bold tail.").bold(true);
            }
            let p = dir.join("big.docx");
            doc.save(&p)?;
            format!("{PARAGRAPHS} paragraphs → {}", file_size(&p))
        }
        "docx-read-text" => {
            let doc = Document::open(dir.join("big.docx"))?;
            let text = doc.text();
            format!("{} characters", text.chars().count())
        }
        "pptx-write" => {
            let mut deck = Presentation::new();
            for i in 0..SLIDES {
                let mut s = deck.add_slide(LayoutKind::TitleAndContent)?;
                s.set_title(&format!("Slide {i}"))?;
                s.set_body_text(&["First point", "Second point", "Third point"])?;
            }
            let p = dir.join("big.pptx");
            deck.save(&p)?;
            format!("{SLIDES} slides → {}", file_size(&p))
        }
        "pptx-read-text" => {
            let deck = Presentation::open(dir.join("big.pptx"))?;
            let text = deck.text();
            format!(
                "{} slides, {} characters",
                deck.slide_count(),
                text.chars().count()
            )
        }
        "empty" => "baseline process".into(),
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    };
    println!("{scenario}: {:.2} s, {detail}", t.elapsed().as_secs_f64());
    Ok(())
}
