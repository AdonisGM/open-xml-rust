//! Compares raw quick-xml event throughput with `XmlReader`.
//!
//! ```text
//! cargo run --release -p openxml-xml --example reader_bench
//! ```

use std::fmt::Write as _;
use std::time::Instant;

use openxml_xml::{Event, XmlReader};

fn main() {
    let mut s = String::from(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for r in 1..=100_000 {
        let _ = write!(s, r#"<row r="{r}">"#);
        for c in 0..10u8 {
            let _ = write!(
                s,
                r#"<c r="{}{r}" t="s"><v>{}</v></c>"#,
                (b'A' + c) as char,
                r % 100
            );
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData></worksheet>");
    let mb = s.len() as f64 / 1e6;

    let t = Instant::now();
    let mut r = quick_xml::NsReader::from_str(&s);
    let mut n = 0usize;
    loop {
        match r.read_event().unwrap() {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                let _ = r.resolver().resolve_element(e.name());
                for a in e.attributes() {
                    let a = a.unwrap();
                    n += a.value.len();
                }
            }
            _ => {}
        }
    }
    let raw = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let mut r = XmlReader::new(&s);
    let mut m = 0usize;
    loop {
        match r.next_event().unwrap() {
            Event::Eof => break,
            Event::Start(tag) => {
                for a in tag.attributes() {
                    m += a.value.len();
                }
            }
            _ => {}
        }
    }
    let ours = t.elapsed().as_secs_f64();
    assert_eq!(n, m);
    println!(
        "{mb:.1} MB: quick-xml {:.0} MB/s, XmlReader {:.0} MB/s",
        mb / raw,
        mb / ours
    );
}
