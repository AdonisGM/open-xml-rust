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
* **document APIs** for Word (`.docx`), Excel (`.xlsx`) and PowerPoint (`.pptx`),
  including pictures, charts, comments, footnotes, tracked changes, tables,
  shapes, data validation, conditional formatting, animations and more;
* a command-line tool to inspect, round-trip and validate packages.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design.

## Crates

| Crate | Purpose |
|-------|---------|
| `openxml` | Facade re-exporting everything below |
| `openxml-docx` | Word documents |
| `openxml-xlsx` | Excel workbooks |
| `openxml-pptx` | PowerPoint presentations |
| `openxml-chart` | DrawingML charts shared by the three formats |
| `openxml-core` | Shared errors, typed part I/O, units, image detection, document properties |
| `openxml-schema` | Generated schema types (`wml`, `sml`, `pml`, `dml`, …) |
| `openxml-codegen` | The generator (XSD → Rust) |
| `openxml-opc` | Packaging (ECMA-376 Part 2) |
| `openxml-xml` | Namespace-aware XML reader/writer, raw nodes, semantic diff |
| `openxml-cli` | The `openxml` command-line tool |
| `openxml-testkit` | Test helpers (XSD validation with `xmllint`) |

## Quick start

```rust
use openxml::docx::{Document, ListKind};
use openxml::xlsx::{CellStyle, Workbook};
use openxml::pptx::{LayoutKind, Presentation};
use openxml::chart::{Chart, ChartKind, Series};
use openxml::Length;

// Word
let mut doc = Document::new();
doc.add_heading("Quarterly report", 1)?;
doc.add_paragraph("Revenue grew by ").add_run("12%").bold(true);
doc.add_list_item("New product line", ListKind::Bullet, 0)?;
doc.save("report.docx")?;

// Excel
let mut wb = Workbook::new();
let bold = wb.add_style(&CellStyle::new().bold());
let mut sheet = wb.worksheet_mut("Sheet1")?;
sheet.set_value("A1", "Total")?;
sheet.set_cell_style("A1", bold)?;
sheet.set_formula("B1", "SUM(B2:B10)")?;
wb.save("report.xlsx")?;

// PowerPoint
let mut deck = Presentation::new();
let mut slide = deck.add_slide(LayoutKind::Title)?;
slide.set_title("Project Aurora")?;
deck.save("deck.pptx")?;

// Charts (the same description works in all three formats)
let chart = Chart::new(ChartKind::Column)
    .title("Revenue")
    .categories(["Q1", "Q2", "Q3"])
    .series(Series::new("2024", [10.0, 12.5, 9.0]));
doc.add_chart(&chart, Length::cm(15.0), Length::cm(8.0))?;

// Reading
let text = Document::open("report.docx")?.text();
```

Every API keeps the whole package: parts it does not manage are saved
byte-for-byte, and the typed schema objects are always reachable
(`document_mut()`, `raw_mut()`, `package_mut()`) for anything the
convenience methods do not cover. Runnable examples:

```text
cargo run -p openxml --example all_formats -- out/
cargo run -p openxml-docx --example create_report -- out/report.docx
cargo run -p openxml-xlsx --example create_workbook -- out/report.xlsx
cargo run -p openxml-pptx --example create_deck -- out/deck.pptx
```

## Features

| Area | Word | Excel | PowerPoint |
|------|------|-------|------------|
| Text & formatting | runs, paragraphs, styles, lists, tabs, borders | rich text, cell styles, number formats | runs, bullets, fonts, spacing |
| Tables | styles, merges, nesting | tables, auto filter, sorting state | merges, borders, styles |
| Pictures | inline and floating | anchored to cells | crop, effects, replace |
| Charts | ✓ | ✓ (from ranges) | ✓ |
| Shapes | VML text boxes, shapes, watermark | — | presets, freeforms, connectors, groups |
| Comments | ✓ (replies, ranges) | notes | legacy comments |
| Notes | footnotes, endnotes | — | speaker notes |
| Review | tracked changes, protection | sheet/workbook protection | — |
| Links | hyperlinks, bookmarks, fields, TOC | hyperlinks, defined names | hyperlinks, actions |
| Layout | sections, columns, headers/footers | print setup, views, freeze panes | masters, layouts, themes |
| Data | content controls, equations | validation, conditional formats, formulas | transitions, animations, media |
| Properties | core, app, custom | core, app, custom | core, app, custom |

Anything not listed is still reachable through the typed schema objects.

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
cargo run -p openxml-cli -- text report.xlsx          # plain text of any docx/xlsx/pptx
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
