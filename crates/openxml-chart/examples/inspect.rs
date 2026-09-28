//! Prints the charts of an Office document.
//!
//! ```text
//! cargo run -p openxml-chart --example inspect -- file.pptx
//! ```

use openxml_opc::Package;

fn main() {
    let path = std::env::args().nth(1).expect("usage: inspect <file>");
    let pkg = Package::open_path(&path).expect("open package");
    for (name, part) in pkg.parts() {
        if part.content_type() != openxml_opc::known::content_types::CHART {
            continue;
        }
        match openxml_chart::read_chart(part.data()) {
            Ok(info) => {
                println!("{name}: title {:?}", info.title);
                for plot in &info.plots {
                    println!("  {} ({:?})", plot.element, plot.kind);
                    for s in &plot.series {
                        println!("    {:?}: {:?} -> {:?}", s.name, s.categories, s.values);
                    }
                }
            }
            Err(e) => println!("{name}: {e}"),
        }
    }
}
