//! Measures parsing and serialization throughput of the generated types.
//!
//! ```text
//! cargo run --release -p openxml-schema --example bench
//! ```

use std::fmt::Write as _;
use std::time::Instant;

use openxml_schema::{sml, wml};

fn worksheet_xml(rows: usize, cols: usize) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for r in 1..=rows {
        let _ = write!(s, r#"<row r="{r}">"#);
        for c in 0..cols {
            let col = (b'A' + c as u8) as char;
            if c % 2 == 0 {
                let _ = write!(s, r#"<c r="{col}{r}"><v>{}</v></c>"#, r * 10 + c);
            } else {
                let _ = write!(s, r#"<c r="{col}{r}" t="s"><v>{}</v></c>"#, r % 100);
            }
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData></worksheet>");
    s
}

fn document_xml(paragraphs: usize) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for i in 0..paragraphs {
        let _ = write!(
            s,
            r#"<w:p><w:pPr><w:jc w:val="both"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="24"/></w:rPr><w:t xml:space="preserve">Paragraph {i} with some text </w:t></w:r><w:r><w:t>and a second run.</w:t></w:r></w:p>"#
        );
    }
    s.push_str("</w:body></w:document>");
    s
}

fn measure<T>(label: &str, xml: &str, parse: impl Fn(&str) -> T, write: impl Fn(&T) -> String) {
    let mb = xml.len() as f64 / 1e6;
    let t = Instant::now();
    let value = parse(xml);
    let parse_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let out = write(&value);
    let write_s = t.elapsed().as_secs_f64();
    println!(
        "{label:<34} {mb:>7.1} MB   parse {parse_s:>6.3} s ({:>6.1} MB/s)   write {write_s:>6.3} s ({:>6.1} MB/s)",
        mb / parse_s,
        out.len() as f64 / 1e6 / write_s
    );
}

fn main() {
    let ws = worksheet_xml(100_000, 10);
    measure(
        "worksheet 100k rows x 10 cells",
        &ws,
        |x| sml::elements::WORKSHEET.parse(x).unwrap(),
        |v| sml::elements::WORKSHEET.to_xml(v),
    );
    let doc = document_xml(100_000);
    measure(
        "document 100k paragraphs",
        &doc,
        |x| wml::elements::DOCUMENT.parse(x).unwrap(),
        |v| wml::elements::DOCUMENT.to_xml(v),
    );
}
