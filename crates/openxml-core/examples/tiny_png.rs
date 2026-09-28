//! Writes a placeholder PNG produced by `openxml_core::image::tiny_png` to stdout.
use std::io::Write;

fn main() {
    let png = openxml_core::image::tiny_png(40, 30);
    std::io::stdout().write_all(&png).unwrap();
}
