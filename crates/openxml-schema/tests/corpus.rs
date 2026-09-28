//! Round-trip tests of every XML part of real Office documents.
//!
//! Each part is parsed into the generated types, written back and compared
//! with the original using a semantic XML comparison. Writing must also be
//! idempotent.
//!
//! The committed fixtures under `fixtures/` are always tested. A larger local
//! corpus can be tested with `OPENXML_CORPUS=/path/to/dir cargo test -p
//! openxml-schema --test corpus -- --ignored --nocapture`.

use std::path::{Path, PathBuf};

use openxml_opc::Package;
use openxml_schema::round_trip_xml;
use openxml_xml::compare::{DiffKind, semantic_diff};
use openxml_xml::{RawElement, decode_xml_bytes};

#[derive(Default, Debug)]
struct Report {
    files: usize,
    unreadable_files: Vec<String>,
    xml_parts: usize,
    typed_parts: usize,
    unknown_roots: Vec<String>,
    failures: Vec<String>,
    diffs: Vec<String>,
    reordered: Vec<String>,
}

fn office_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            office_files(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
            matches!(
                x.to_ascii_lowercase().as_str(),
                "docx" | "xlsx" | "pptx" | "docm" | "xlsm" | "pptm"
            )
        }) {
            out.push(p);
        }
    }
    out.sort();
}

fn check_file(path: &Path, report: &mut Report) {
    report.files += 1;
    let pkg = match Package::open_path(path) {
        Ok(p) => p,
        Err(e) => {
            report.unreadable_files.push(format!("{}: {e}", path.display()));
            return;
        }
    };
    let file = path.file_name().unwrap().to_string_lossy().to_string();
    for (name, part) in pkg.parts() {
        let Ok(text) = decode_xml_bytes(part.data()) else {
            continue;
        };
        if !text.trim_start().starts_with('<') {
            continue;
        }
        let Ok(original) = RawElement::parse(&text) else {
            continue;
        };
        report.xml_parts += 1;
        let at = format!("{file}{name}");
        match round_trip_xml(&text) {
            None => report
                .unknown_roots
                .push(format!("{at} <{}>", original.name.local)),
            Some(Err(e)) => report.failures.push(format!("{at}: {e}")),
            Some(Ok(out)) => {
                report.typed_parts += 1;
                let back = match RawElement::parse(&out) {
                    Ok(b) => b,
                    Err(e) => {
                        report
                            .failures
                            .push(format!("{at}: output is not well-formed: {e}"));
                        continue;
                    }
                };
                for d in semantic_diff(&original, &back) {
                    if d.kind == DiffKind::Reordered {
                        report.reordered.push(format!("{at}: {}", d.path));
                    } else {
                        report.diffs.push(format!("{at}: {d}"));
                    }
                }
                match round_trip_xml(&out) {
                    Some(Ok(again)) if again == out => {}
                    Some(Ok(_)) => report.failures.push(format!("{at}: writing is not idempotent")),
                    other => report
                        .failures
                        .push(format!("{at}: re-reading output failed: {other:?}")),
                }
            }
        }
    }
}

fn run(dir: &Path) -> Report {
    let mut files = Vec::new();
    office_files(dir, &mut files);
    let mut report = Report::default();
    for f in &files {
        check_file(f, &mut report);
    }
    report
}

fn print(report: &Report) {
    println!(
        "files: {} (unreadable {}), xml parts: {}, typed: {}, unknown roots: {}, failures: {}, diffs: {}, reordered: {}",
        report.files,
        report.unreadable_files.len(),
        report.xml_parts,
        report.typed_parts,
        report.unknown_roots.len(),
        report.failures.len(),
        report.diffs.len(),
        report.reordered.len()
    );
    for (title, list) in [
        ("unreadable", &report.unreadable_files),
        ("failures", &report.failures),
        ("diffs", &report.diffs),
        ("unknown roots", &report.unknown_roots),
        ("reordered", &report.reordered),
    ] {
        if !list.is_empty() {
            let limit: usize = std::env::var("OPENXML_REPORT_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(40);
            println!("--- {title} (first {limit} of {})", list.len());
            for l in list.iter().take(limit) {
                println!("  {l}");
            }
        }
    }
}

#[test]
fn committed_fixtures_round_trip_losslessly() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let report = run(&dir);
    print(&report);
    assert!(report.files > 0, "no fixtures found");
    assert!(report.unreadable_files.is_empty());
    assert!(report.failures.is_empty());
    assert!(report.diffs.is_empty());
    assert!(report.typed_parts > 0);
}

#[test]
#[ignore = "set OPENXML_CORPUS to a directory of Office documents"]
fn external_corpus_round_trips() {
    let Some(dir) = std::env::var_os("OPENXML_CORPUS") else {
        return;
    };
    let report = run(Path::new(&dir));
    print(&report);
    assert!(report.failures.is_empty() && report.diffs.is_empty());
}

/// Validates the typed model of every fixture part; documents written by
/// Office applications must not report missing required content.
#[test]
fn committed_fixtures_have_required_content() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut files = Vec::new();
    office_files(&dir, &mut files);
    let mut checked = 0;
    let mut issues = Vec::new();
    for f in &files {
        let pkg = Package::open_path(f).unwrap();
        for (name, part) in pkg.parts() {
            let Ok(text) = decode_xml_bytes(part.data()) else {
                continue;
            };
            if let Some(result) = openxml_schema::validate_xml(&text) {
                checked += 1;
                for i in result.unwrap() {
                    issues.push(format!("{}{name}: {i}", f.file_name().unwrap().to_string_lossy()));
                }
            }
        }
    }
    println!("{checked} parts validated, {} issues", issues.len());
    for i in issues.iter().take(50) {
        println!("  {i}");
    }
    assert!(checked > 500);
    // The only documents with problems are known to be invalid: bar-chart.pptx
    // (written by Apache POI) stores negative axis ids in xsd:unsignedInt attributes.
    let unexpected: Vec<_> = issues
        .iter()
        .filter(|i| !i.starts_with("bar-chart.pptx/ppt/charts/chart1.xml"))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:#?}");
    assert!(issues.iter().all(|i| i.contains("invalid value \"-18")));
}
