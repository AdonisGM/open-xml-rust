//! The checked-in `openxml-schema` sources must match the generator's output.

use std::path::PathBuf;

#[test]
fn generated_sources_are_up_to_date() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = openxml_codegen::generate(
        &root.join("schemas/transitional"),
        Some(&root.join("schemas/spec-index.json")),
    )
    .unwrap();
    assert_eq!(files.len(), 27, "one module per schema file plus mod.rs");
    let out_dir = root.join("crates/openxml-schema/src/generated");
    let mut stale = Vec::new();
    for (name, source) in &files {
        let formatted = match openxml_codegen::format_source(source) {
            Ok(f) => f,
            Err(e) if e.contains("cannot run rustfmt") => {
                eprintln!("note: rustfmt not available, skipping comparison");
                return;
            }
            Err(e) => panic!("{name}: {e}"),
        };
        let current = std::fs::read_to_string(out_dir.join(name)).unwrap_or_default();
        if current != formatted {
            stale.push(name.clone());
        }
    }
    assert!(
        stale.is_empty(),
        "stale generated files {stale:?}; run `cargo run -p openxml-codegen`"
    );
}
