//! Round-trips the XML examples printed in ECMA-376 Part 1.
//!
//! `schemas/spec-examples.json` is extracted from the specification by
//! `tools/spec_examples.py`: every example fragment whose root is the element
//! described by its section, together with that element's schema type. Each
//! fragment is read as that type, written back and compared semantically.

use std::path::PathBuf;

use openxml_xml::RawElement;
use openxml_xml::compare::{DiffKind, semantic_diff};
use serde_json::Value;

fn module_for(prefix: &str) -> Option<&'static str> {
    Some(match prefix {
        "w" => "wml",
        "x" => "sml",
        "p" => "pml",
        "a" => "dml",
        "pic" => "dml_picture",
        "c" => "dml_chart",
        "cdr" => "dml_chart_drawing",
        "dgm" => "dml_diagram",
        "lc" => "dml_locked_canvas",
        "wp" => "dml_wordprocessing_drawing",
        "xdr" => "dml_spreadsheet_drawing",
        "m" => "shared_math",
        "ep" => "shared_extended_properties",
        "op" => "shared_custom_properties",
        "vt" => "shared_variant_types",
        "ds" => "shared_custom_xml_data_properties",
        "b" => "shared_bibliography",
        "ac" => "shared_additional_characteristics",
        "sl" => "shared_custom_xml_schema_properties",
        "s" => "shared_types",
        _ => return None,
    })
}

const MODULES: &[&str] = &[
    "dml",
    "wml",
    "sml",
    "pml",
    "dml_chart",
    "dml_diagram",
    "dml_picture",
    "dml_wordprocessing_drawing",
    "dml_spreadsheet_drawing",
    "dml_chart_drawing",
    "dml_locked_canvas",
    "shared_math",
    "shared_types",
    "vml",
    "vml_office",
];

#[test]
fn specification_examples_round_trip() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/spec-examples.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let namespaces = data["namespaces"].as_object().unwrap();
    let defaults = data["default_namespaces"].as_object().unwrap();
    let decls: String = namespaces
        .iter()
        .map(|(p, u)| format!(r#" xmlns:{p}="{}""#, u.as_str().unwrap()))
        .collect();

    let examples = data["examples"].as_array().unwrap();
    let (mut ok, mut simple, mut failures) = (0, 0, Vec::new());
    for ex in examples {
        let ns = ex["ns"].as_str().unwrap();
        let ty = ex["type"].as_str().unwrap();
        let section = ex["section"].as_str().unwrap();
        let module = module_for(ns).unwrap();
        let default = defaults
            .get(ns)
            .map(|u| format!(r#" xmlns="{}""#, u.as_str().unwrap()))
            .unwrap_or_default();
        let xml = format!("<root{decls}{default}>{}</root>", ex["xml"].as_str().unwrap());
        let original = RawElement::parse(&xml).unwrap();
        let original = original.elements().next().unwrap().clone();
        if ty.starts_with("ST_") {
            // Elements with a simple type have no struct of their own.
            simple += 1;
            continue;
        }
        // The type usually lives in the element's schema; shared DrawingML types live in `dml`.
        let found = std::iter::once(module)
            .chain(MODULES.iter().copied())
            .find_map(|m| openxml_schema::round_trip_fragment(m, ty, &xml));
        let out = match found {
            Some(Ok(out)) => out,
            Some(Err(e)) => {
                failures.push(format!("§{section} {ty}: read error {e}"));
                continue;
            }
            None => {
                failures.push(format!("§{section} {module}::{ty}: no such type"));
                continue;
            }
        };
        let back = RawElement::parse(&out).unwrap();
        let diffs: Vec<_> = semantic_diff(&original, &back)
            .into_iter()
            .filter(|d| d.kind != DiffKind::Reordered)
            .collect();
        if diffs.is_empty() {
            ok += 1;
        } else {
            failures.push(format!(
                "§{section} {ty}: {}",
                diffs.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")
            ));
        }
    }
    println!(
        "{ok} of {} specification examples round-trip ({simple} simple-typed elements skipped)",
        examples.len()
    );
    assert!(examples.len() > 400);
    assert_eq!(ok + simple + failures.len(), examples.len());
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
