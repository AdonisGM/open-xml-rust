//! End-to-end tests of the `openxml` binary.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_openxml"))
}

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .display()
        .to_string()
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = bin().args(args).output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

#[test]
fn usage_errors() {
    let (ok, _, err) = run(&[]);
    assert!(!ok);
    assert!(err.contains("usage:"));
    let (ok, out, _) = run(&["--help"]);
    assert!(ok && out.contains("roundtrip"));
    let (ok, _, err) = run(&["info", "/nonexistent.docx"]);
    assert!(!ok && err.contains("error:"));
}

#[test]
fn info_lists_parts_and_relationships() {
    let (ok, out, _) = run(&["info", &fixture("ecma/PivotTableFormats.xlsx")]);
    assert!(ok);
    assert!(out.contains("main part: /xl/workbook.xml"));
    assert!(out.contains("/xl/worksheets/sheet1.xml"));
    assert!(out.contains("--worksheet-->"));
}

#[test]
fn cat_prints_a_part() {
    let (ok, out, _) = run(&["cat", &fixture("poi/sample.docx"), "/word/document.xml"]);
    assert!(ok);
    assert!(out.contains("w:document"));
    let (ok, _, err) = run(&["cat", &fixture("poi/sample.docx"), "/missing.xml"]);
    assert!(!ok && err.contains("no part"));
}

#[test]
fn roundtrip_reports_and_writes_package() {
    let out_path = std::env::temp_dir().join(format!("openxml-cli-{}.xlsx", std::process::id()));
    let (ok, out, err) = run(&[
        "roundtrip",
        &fixture("ecma/PivotTableFormats.xlsx"),
        out_path.to_str().unwrap(),
    ]);
    assert!(ok, "{out}\n{err}");
    assert!(out.contains("0 failures, 0 with differences"));
    let (ok, out, _) = run(&["info", out_path.to_str().unwrap()]);
    assert!(ok && out.contains("/xl/workbook.xml"));
    std::fs::remove_file(out_path).unwrap();
}

#[test]
fn validate_checks_parts_against_the_schemas() {
    if Command::new("xmllint").arg("--version").output().is_err() {
        eprintln!("xmllint not installed; skipping");
        return;
    }
    let (_, out, err) = run(&["validate", &fixture("ecma/PivotTableFormats.xlsx")]);
    assert!(out.contains("valid    /xl/workbook.xml"), "{out}\n{err}");
    assert!(out.contains(" valid, "));
    let (ok, _, err) = run(&[
        "validate",
        &fixture("ecma/PivotTableFormats.xlsx"),
        "--schemas",
        "/nonexistent",
    ]);
    assert!(!ok && err.contains("schema driver not found"));
}

#[test]
fn text_extracts_words_cells_and_slides() {
    let (ok, out, err) = run(&["text", &fixture("poi/sample.docx")]);
    assert!(ok, "{err}");
    assert!(!out.trim().is_empty());
    let (ok, out, err) = run(&["text", &fixture("ecma/PivotTableFormats.xlsx")]);
    assert!(ok, "{err}");
    assert!(out.starts_with("== "), "{out}");
    let (ok, out, err) = run(&["text", &fixture("poi/SampleShow.pptx")]);
    assert!(ok, "{err}");
    assert!(out.contains("--- slide 1"), "{out}");
}
