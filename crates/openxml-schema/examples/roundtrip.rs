//! Prints a part of an Office document after a typed read/write cycle.
//!
//! ```text
//! cargo run -p openxml-schema --example roundtrip -- file.docx /word/document.xml
//! ```

use openxml_opc::{Package, PartName};
use openxml_xml::decode_xml_bytes;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(file), Some(part)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: roundtrip <package> <part-name>");
        std::process::exit(2);
    };
    let pkg = Package::open_path(file).expect("open package");
    let name = PartName::new(part.as_str()).expect("part name");
    let data = pkg.part(&name).expect("part exists").data();
    let xml = decode_xml_bytes(data).expect("text");
    match openxml_schema::round_trip_xml(&xml) {
        Some(Ok(out)) => println!("{out}"),
        Some(Err(e)) => eprintln!("error: {e}"),
        None => eprintln!("the root element is not a global element of the schemas"),
    }
}
