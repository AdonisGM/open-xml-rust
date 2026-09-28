//! `openxml-codegen` — regenerates `crates/openxml-schema/src/generated`.
//!
//! ```text
//! cargo run -p openxml-codegen            # write the generated sources
//! cargo run -p openxml-codegen -- --check # fail if they are out of date
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openxml_codegen::format_source as format;

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let check = std::env::args().any(|a| a == "--check");
    let schemas = root.join("schemas/transitional");
    let spec = root.join("schemas/spec-index.json");
    let out_dir = root.join("crates/openxml-schema/src/generated");

    let files = match openxml_codegen::generate(&schemas, Some(&spec)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut stale = Vec::new();
    for (name, source) in &files {
        let formatted = match format(source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error formatting {name}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let path: &Path = &out_dir.join(name);
        if check {
            if std::fs::read_to_string(path).ok().as_deref() != Some(formatted.as_str()) {
                stale.push(name.clone());
            }
        } else {
            std::fs::create_dir_all(&out_dir).expect("create output directory");
            std::fs::write(path, formatted).expect("write generated file");
        }
    }
    if check && !stale.is_empty() {
        eprintln!("generated sources are out of date: {}", stale.join(", "));
        eprintln!("run `cargo run -p openxml-codegen` to regenerate them");
        return ExitCode::FAILURE;
    }
    let total: usize = files.iter().map(|(_, s)| s.lines().count()).sum();
    println!(
        "{} {} files ({} lines before formatting)",
        if check { "checked" } else { "generated" },
        files.len(),
        total
    );
    ExitCode::SUCCESS
}
