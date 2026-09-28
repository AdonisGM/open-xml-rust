//! Prints the text, headers/footers and tables of `.docx` files.
//!
//! ```text
//! cargo run -p openxml-docx --example dump_text -- file.docx [more.docx…]
//! ```

use openxml_docx::Document;

fn main() {
    for path in std::env::args().skip(1) {
        println!("===== {path}");
        match Document::open(&path) {
            Ok(doc) => {
                println!(
                    "paragraphs: {}, tables: {}",
                    doc.paragraphs().len(),
                    doc.tables().len()
                );
                for hf in doc.headers_and_footers() {
                    println!("[{:?} {}] {:?}", hf.kind(), hf.part_name(), hf.text());
                }
                if let Some(n) = doc.numbering() {
                    println!(
                        "numbering: {} abstract, {} num",
                        n.abstract_num.len(),
                        n.num.len()
                    );
                }
                println!("{:?}", doc.text());
            }
            Err(e) => println!("error: {e}"),
        }
    }
}
