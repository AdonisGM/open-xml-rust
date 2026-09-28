# open-xml-rust

Office Open XML (ECMA-376) for Rust, built from the specification up:

* a **typed object model generated from the official ECMA-376 XML Schemas**
  (every element and simple type of WordprocessingML, SpreadsheetML,
  PresentationML, DrawingML, VML and the shared schemas — 27 modules),
  documented with the section numbers and descriptions of the specification;
* **lossless round trips**: anything the schemas do not describe
  (`mc:AlternateContent`, `w14:*` extensions, vendor markup, unknown parts)
  is preserved in place;
* **Open Packaging Conventions** (ZIP container, content types,
  relationships, core properties);
* **document APIs** for Word (`.docx`), Excel (`.xlsx`) and PowerPoint (`.pptx`);
* a command-line tool to inspect, round-trip and validate packages.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design.

## Crates

| Crate | Purpose |
|-------|---------|
| `openxml` | Facade re-exporting everything below |
| `openxml-docx` | Word documents |
| `openxml-xlsx` | Excel workbooks |
| `openxml-pptx` | PowerPoint presentations |
| `openxml-core` | Shared errors, typed part I/O, units, image detection |
| `openxml-schema` | Generated schema types (`wml`, `sml`, `pml`, `dml`, …) |
| `openxml-codegen` | The generator (XSD → Rust) |
| `openxml-opc` | Packaging (ECMA-376 Part 2) |
| `openxml-xml` | Namespace-aware XML reader/writer, raw nodes, semantic diff |
| `openxml-cli` | The `openxml` command-line tool |
| `openxml-testkit` | Test helpers (XSD validation with `xmllint`) |

## Working with the schema types directly

```rust
use openxml_schema::wml;

let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#;
let doc = wml::elements::DOCUMENT.parse(xml)?;
let out = wml::elements::DOCUMENT.to_xml(&doc);
```

## Command-line tool

```text
cargo run -p openxml-cli -- info report.docx          # parts, content types, relationships
cargo run -p openxml-cli -- cat report.docx /word/document.xml
cargo run -p openxml-cli -- roundtrip report.docx out.docx
cargo run -p openxml-cli -- validate report.docx      # XSD validation (needs xmllint)
```

## Development

```text
scripts/check.sh                 # fmt, clippy, generated-code freshness, all tests
cargo run -p openxml-codegen     # regenerate crates/openxml-schema/src/generated
OPENXML_CORPUS=~/docs cargo test --release -p openxml-schema --test corpus -- --ignored --nocapture
```

Schema validation tests use `xmllint` (libxml2) when it is installed.

## Specification sources

The vendored schemas and data under `schemas/` come from the ECMA-376
distributions published by Ecma International (Parts 1, 2 and 4, 5th
edition). Test documents under `fixtures/poi` come from the Apache POI
project (Apache License 2.0).
