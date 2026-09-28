//! Code generator for the `openxml-schema` crate.
//!
//! Pipeline: [`xsd`] parses the ECMA-376 XML Schemas, [`registry`] indexes the
//! definitions, [`lower`] turns them into the [`ir`] (Rust structs, enums and
//! fields) and [`emit`] prints Rust source. Documentation comments are taken
//! from the ECMA-376 Part 1 section index ([`spec`]).

#![warn(missing_docs)]

pub mod emit;
pub mod ir;
pub mod lower;
pub mod names;
pub mod registry;
pub mod spec;
pub mod xsd;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Parses every `.xsd` file of a directory, sorted by file name.
pub fn load_schemas(dir: &Path) -> Result<Vec<xsd::Schema>, String> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "xsd"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            xsd::parse_schema(name, &text).map_err(|e| format!("{name}: {e}"))
        })
        .collect()
}

/// Runs the whole pipeline and returns the generated files (unformatted).
pub fn generate(schema_dir: &Path, spec_index: Option<&Path>) -> Result<Vec<(String, String)>, String> {
    let schemas = load_schemas(schema_dir)?;
    let reg = registry::Registry::new(schemas);
    let spec = match spec_index {
        Some(p) => spec::SpecIndex::load(p)?,
        None => spec::SpecIndex::empty(),
    };
    let krate = lower::lower(&reg, &spec);
    Ok(emit::emit_crate(&krate))
}

/// Formats Rust source with `rustfmt` (edition 2024, workspace settings).
pub fn format_source(source: &str) -> Result<String, String> {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rustfmt.toml");
    let mut child = Command::new("rustfmt")
        .args([
            "--edition",
            "2024",
            "--emit",
            "stdout",
            "--quiet",
            "--config-path",
        ])
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run rustfmt: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let src = source.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(src.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    writer.join().expect("writer thread").map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}
