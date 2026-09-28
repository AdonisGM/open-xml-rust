//! Helpers shared by the test suites of the workspace.
//!
//! * [`validate_xml`] checks a part against the ECMA-376 Transitional XML
//!   Schemas with `xmllint` (libxml2), using `schemas/validation/transitional-all.xsd`;
//! * [`validate_package`] validates every XML part of a package;
//! * [`office_files`] lists the Office documents of a directory tree.
//!
//! When `xmllint` is not installed validation is skipped (with a message on
//! stderr) unless `OPENXML_REQUIRE_XMLLINT=1` is set, in which case it fails.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use openxml_opc::Package;
use openxml_xml::decode_xml_bytes;

/// Root of the workspace.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Path of a file under `fixtures/`.
pub fn fixture(name: &str) -> PathBuf {
    workspace_root().join("fixtures").join(name)
}

fn xmllint_available() -> bool {
    Command::new("xmllint")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Outcome of a schema validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// The document is valid.
    Valid,
    /// Validation could not run (no `xmllint`).
    Skipped,
}

/// Validates an XML document against the Transitional schemas.
///
/// Returns `Err` with the validator's messages when the document is invalid.
pub fn validate_xml(xml: &str) -> Result<Validation, String> {
    if !xmllint_available() {
        if std::env::var("OPENXML_REQUIRE_XMLLINT").is_ok_and(|v| v == "1") {
            return Err("xmllint is required but not installed".into());
        }
        eprintln!("note: xmllint not found, schema validation skipped");
        return Ok(Validation::Skipped);
    }
    let schema = workspace_root().join("schemas/validation/transitional-all.xsd");
    let mut child = Command::new("xmllint")
        .arg("--noout")
        .arg("--nonet")
        .arg("--schema")
        .arg(&schema)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = xml.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    writer.join().expect("writer thread").map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(Validation::Valid)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Validates every XML part of a package whose root namespace is covered by
/// the ECMA-376 schemas. Returns `(part name, messages)` for invalid parts.
pub fn validate_package(pkg: &Package) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    for (name, part) in pkg.parts() {
        let Ok(text) = decode_xml_bytes(part.data()) else {
            continue;
        };
        let Ok((ns, _)) = openxml_xml::root_name(&text) else {
            continue;
        };
        // Only parts whose root belongs to an ECMA-376 schema namespace.
        if !ns.is_known()
            || matches!(
                ns,
                openxml_xml::Ns::CP | openxml_xml::Ns::CT | openxml_xml::Ns::PR
            )
        {
            continue;
        }
        if let Err(e) = validate_xml(&text) {
            failures.push((name.to_string(), e));
        }
    }
    failures
}

/// Lists `.docx`, `.xlsx`, `.pptx` (and macro-enabled variants) under `dir`, sorted.
pub fn office_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                matches!(
                    x.to_ascii_lowercase().as_str(),
                    "docx" | "xlsx" | "pptx" | "docm" | "xlsm" | "pptm"
                )
            }) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_good_and_rejects_bad_documents() {
        let good = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t xml:space="preserve"> x </w:t></w:r></w:p></w:body></w:document>"#;
        let bad = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:r/></w:body></w:document>"#;
        match validate_xml(good) {
            Ok(Validation::Skipped) => return,
            Ok(Validation::Valid) => {}
            Err(e) => panic!("{e}"),
        }
        let err = validate_xml(bad).unwrap_err();
        assert!(err.contains("not expected"), "{err}");
    }

    #[test]
    fn lists_fixture_documents() {
        let files = office_files(&workspace_root().join("fixtures"));
        assert!(files.len() >= 40);
        assert!(files.iter().any(|f| f.extension().unwrap() == "docx"));
        assert!(fixture("ecma/PivotTableFormats.xlsx").exists());
    }
}
