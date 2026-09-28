# Architecture

`open-xml-rust` implements Office Open XML (ECMA-376) in Rust from scratch.
The object model is **generated from the official ECMA-376 XML Schemas**
rather than written by hand, and every layer is designed so that a document
read and written back loses nothing.

```
                ┌──────────────────────────────────────────────────────┐
  applications  │ openxml (facade)            openxml-cli (`openxml`)  │
                ├──────────────────┬──────────────────┬────────────────┤
  document APIs │ openxml-docx     │ openxml-xlsx     │ openxml-pptx   │
                ├──────────────────┴──────────────────┴────────────────┤
  shared        │ openxml-core  (errors, typed part I/O, units, images)│
                ├──────────────────────────────┬───────────────────────┤
  typed model   │ openxml-schema  ◀─generated─ │ openxml-codegen       │
                │ (27 modules, ~190k lines)    │ (XSD → Rust)          │
                ├──────────────────────────────┴───────────────────────┤
  packaging     │ openxml-opc   (ZIP, parts, content types, relationships)
                ├──────────────────────────────────────────────────────┤
  XML           │ openxml-xml   (namespaces, reader, writer, raw nodes)│
                └──────────────────────────────────────────────────────┘
  test support: openxml-testkit (XSD validation via xmllint, corpus walking)
```

Each layer only depends on the layers below it. Lower layers never know
about Word, Excel or PowerPoint.

## Inputs: the specification

| Input | Source | Used for |
|-------|--------|----------|
| `schemas/transitional/*.xsd` | ECMA-376 Part 4 (2016) | the generated object model |
| `schemas/strict/*.xsd` | ECMA-376 Part 1 (2016) | cross-checks (Strict ⊂ Transitional) |
| `schemas/opc/*.xsd` | ECMA-376 Part 2 (2021) | reference for `openxml-opc` |
| `schemas/spec-index.json` | extracted from the Part 1 PDF by `tools/spec_index.py` | documentation of generated code |
| `schemas/validation/*.xsd` | driver schema importing all Transitional schemas | XSD validation in tests and the CLI |

**Why Transitional?** Office applications write Transitional documents, and
the Transitional schemas are a superset of Strict (legacy features plus
simple types widened to accept both lexical forms). Strict documents are
read into the same model: `Ns::from_uri` maps every Strict namespace URI to
its Transitional twin, and `openxml-opc` maps Strict relationship types.
Documents are always written as Transitional.

The spec index (2 862 entries: every element and simple type of clauses
17–23) gives each generated item a doc comment with the section number,
title and first paragraph of the specification, so the API can be browsed
side by side with the PDF.

## Layer 1 — `openxml-xml`

* **`Ns`** — a compact `u16` identifier for each of the 36 namespaces in
  play (all ECMA-376 namespaces, VML, markup compatibility, OPC, Dublin
  Core). Generated code matches on `(Ns, &str)` pairs instead of comparing
  URIs. `Ns::NONE` is "no namespace", `Ns::OTHER` a namespace outside the
  registry (its URI travels alongside).
* **`XmlReader`** — a pull reader on top of `quick-xml`'s `NsReader`:
  resolves namespaces, merges text/CDATA/entity pieces, normalises line
  endings and attribute whitespace, skips comments/PIs, enforces a nesting
  limit (`MAX_DEPTH`), decodes UTF-8/UTF-16 with BOMs. Start tags keep
  attribute names and values as ranges into the input, so reading does not
  copy unless escapes must be decoded.
* **`XmlWriter`** — keeps a stack of namespace bindings and chooses prefixes
  itself: reuses visible bindings, declares missing ones on demand, avoids
  prefix clashes, emits `xmlns=""` when an unqualified element sits inside a
  default namespace. Declarations captured from a source document are
  re-emitted verbatim, so prefixes referenced from attribute values
  (`mc:Ignorable="w14 wp14"`) remain valid. Text and attribute escaping
  round-trips `\t`, `\n`, `\r` and drops characters XML 1.0 cannot represent.
* **Raw nodes** (`RawElement`, `RawAttribute`, `RawNode`) — schema-less XML
  used to preserve everything the typed model does not describe.
  `RawElement::from_typed` / `to_typed` convert between raw and typed forms
  (needed for `xsd:any` content such as `a:graphicData`).
* **Traits** — `XmlValue` (simple types ⇄ lexical form), `XmlRead` /
  `XmlWrite` (complex types), `ElementDef<T>` (a global element: parse a
  whole part / write it with root namespace declarations).
* **`compare::semantic_diff`** — structural comparison that ignores
  prefixes, attribute order, indentation and equivalent lexical forms
  (`1`/`true`, `1.0`/`1`, `00ab`/`00AB`). Used to prove round-trip fidelity.

## Layer 2 — `openxml-opc`

Open Packaging Conventions (Part 2):

* `PartName` — validation per §6.2.2, ASCII case-insensitive identity,
  resolution of relative relationship targets (`../`, percent-encoding,
  fragments) and computation of relative references.
* `ContentTypes` — `[Content_Types].xml` defaults and overrides. On save the
  table is rebuilt from the parts: an extension gets a `Default` when all its
  parts agree, otherwise parts get `Override`s.
* `Relationships` — per-part and package relationships, internal/external
  targets, fresh `rIdN` identifiers; Strict relationship types are
  canonicalised.
* `Package` — parts are kept as **bytes**. Higher layers parse only the
  parts they understand and write back only the parts they changed, so
  unknown parts (VBA projects, ActiveX, custom XML, printer settings, …)
  survive untouched. Saving is deterministic (fixed ZIP timestamps, stable
  ordering).
* `CoreProperties` — `docProps/core.xml` (Dublin Core), unknown elements kept.

## Layer 3 — `openxml-codegen` → `openxml-schema`

The generator is a normal Rust binary (`cargo run -p openxml-codegen`). Its
output is committed, so users never run it; a test fails if the committed
code is stale.

Pipeline: **parse XSD** (`xsd.rs`, roxmltree) → **index** (`registry.rs`) →
**lower** to an IR of structs/enums/fields (`lower.rs`) → **emit** Rust
(`emit.rs`) → rustfmt.

### Mapping rules

| XML Schema | Rust |
|------------|------|
| `simpleType` with enumerations | `enum` + `as_str()`, `ALL`, `XmlValue`, `Display` |
| `simpleType` restriction without enumerations | `type` alias of the base |
| `union` | `enum` with one variant per member; parsing tries non-string members first |
| `list` | `XmlList<T>` |
| built-ins | `String`, `bool`, `i8`…`u64`, `f32`/`f64`, `HexBinary`, `Base64Binary` |
| `complexType` | `struct` implementing `XmlRead` + `XmlWrite` |
| attribute | `Option<T>` field (lenient: required attributes too) |
| element particle, at most once | `Option<Box<T>>` (`Option<T>` for simple types) |
| element particle, repeating | `Vec<T>` |
| choice / repeating sequence / choice group | a **choice enum** (`Option<E>` or `Vec<E>`) |
| named model group used as a choice | a shared enum named after the group (`EG_PContent`) |
| `xsd:any` | `RawElement` fields |
| `simpleContent` | `value: T` field |
| `mixed="true"` | `children: Vec<RawNode>` |
| global element | `elements::NAME: ElementDef<CT_…>` constant |

Content models are flattened: the top-level sequence becomes the list of
fields, nested non-repeating sequences and sequence groups are inlined, and
everything that can occur in varying order becomes a choice enum whose
variants are all reachable elements. Extension types prepend the base
content and attributes. Names keep their schema spelling (`CT_P`, `ST_Jc`,
`EG_PContent`) so they can be searched in the specification; fields and
variants are converted to `snake_case` / `UpperCamelCase`.

### Reading and writing

* **Lenient reading** — elements are dispatched by name to their field in any
  order. When the same element name belongs to several fields (e.g.
  `w:bookmarkStart` before and after `w:tblPr`), the reader picks the first
  candidate at or after the current position.
* **Canonical writing** — fields are written in schema order, which repairs
  documents produced out of order.
* **Nothing is lost** — unknown attributes (including `xmlns` declarations)
  go to `extra_attrs`; unknown child elements go to the `Other` variant of
  the current choice or to `extra_children` with an anchor recording the
  field (and item index for repeated fields) they preceded; values that do
  not parse as their schema type are kept as raw attributes/elements. This
  is what keeps `mc:AlternateContent` wrappers, `w14:*` extensions and
  vendor markup in place.

### Validation

Every generated type also implements `Validate`: required attributes
(`use="required"`) and required child elements (`minOccurs ≥ 1`, propagated
through nested sequences and choices) must be present. Element order, value
spaces and enumerations are already guaranteed by the types, so together
they cover most of what an XML Schema validator checks — without leaving
Rust. Problems are reported with a path and distinguish *missing* items
from *invalid* ones (whose raw form was preserved):

```text
/c:chartSpace/c:chart/c:plotArea/c:valAx[1]/c:axId: invalid value "-1884097184" for required attribute val
/w:document/w:body/w:tbl[2]: missing required child element w:tblGrid
```

`ElementDef::validate(&value)` checks one document; the generated
`validate_xml(xml)` dispatches on the root element of any part.

## Layer 4 — `openxml-core` and the document APIs

`openxml-core` holds what the three document APIs share: the `Error` type,
typed part I/O (`part::read_part`, `write_part`, `read_related`,
`add_related_part`), `Length` (EMU, with twip/point/inch/cm conversions) and
`FontSize`, and image detection (`sniff_image`: PNG, JPEG, GIF, BMP, TIFF,
EMF, WMF — format, pixel size and resolution).

`openxml-docx`, `openxml-xlsx` and `openxml-pptx` follow the same design:

* **The package is the source of truth.** `Document` / `Workbook` /
  `Presentation` own the `Package`; the parts they manage are parsed into
  generated schema types (lazily for worksheets) and tracked with dirty
  flags. Saving rewrites only changed parts; every other part is written
  back byte-for-byte, so an untouched file round-trips byte-identically and
  an edited one loses nothing the API does not understand.
* **Views over typed data.** Read-only views (`Paragraph`, `Worksheet`,
  `Slide`, …) and mutable views (`ParagraphMut`, `WorksheetMut`,
  `SlideMut`, …) wrap the generated structs. Mutable views know the part
  they belong to, so new relationships (hyperlinks, images) land in the
  right part.
* **Escape hatches everywhere.** Each view exposes the underlying schema
  object (`raw()` / `raw_mut()`, `document_mut()`, `stylesheet_mut()`), and
  every API exposes `package_mut()`.
* **New documents are schema-valid.** Templates (styles, stylesheet, slide
  master with six layouts and a theme) are built so that every part
  validates against the ECMA-376 schemas and follows the conventions of the
  Office applications (ID ranges, required parts, `docProps`).

| API | Highlights |
|-----|------------|
| docx | paragraphs, runs (bold/italic/underline/size/color/font/highlight/…), headings and built-in styles resolved against the document's own styles, bullet and numbered lists, tables (styles, widths, merges, shading), inline pictures, hyperlinks, headers/footers, page setup, text extraction and replacement |
| xlsx | cell values (strings via the shared-string table, numbers, booleans, errors, dates in both date systems, formulas with cached results, shared-formula expansion), styles with deduplication, merges, column widths, row heights, frozen panes, defined names, streaming writer and row-by-row reader |
| pptx | 16:9 template, add/remove/move slides, placeholders (title, subtitle, body levels), text boxes with formatting, pictures, tables, speaker notes, backgrounds, text extraction |

Files produced by the examples were cross-checked with independent
readers available on macOS: `textutil` (Apple's DOCX importer) extracts the
full text including list bullets, and Quick Look renders all three formats.

## Test strategy

| Level | What is checked |
|-------|-----------------|
| unit tests | every module of every crate |
| specification examples | 479 XML examples extracted from the Part 1 PDF (`schemas/spec-examples.json`), each read as the type its section names and written back: 478 round-trip semantically (the last is a simple-typed element) |
| Strict ⊂ Transitional | every Strict type, group, element, attribute and simple type exists in the Transitional schemas — the premise of reading Strict files with the Transitional model |
| robustness | 2 000 deterministic mutations of real parts and 300 of a whole package: errors, never panics; accepted input is written as well-formed XML |
| validator | all fixture parts are checked; the only issues are genuine schema violations in a POI-written chart |
| schema behaviour | typed construction → XSD-valid XML; Strict→Transitional; MCE preservation; ordering; invalid values |
| corpus round trip | every XML part of every fixture: typed read → write → semantic diff = ∅, and writing twice is byte-identical |
| XSD validation | documents produced by the APIs validate against the ECMA schemas with `xmllint` |
| generated code freshness | regenerating from the XSDs reproduces the committed sources |
| document APIs | per crate: unit tests, create → save → reopen for every feature, XSD validation of every produced part, reading real fixtures with asserted content, byte-identical untouched round trips and semantically equal forced rewrites |
| facade | the three formats produced through `openxml` validate against the XSDs and the Rust validator, and read back |
| CLI | end-to-end runs of the binary |

`OPENXML_CORPUS=<dir> cargo test --release -p openxml-schema --test corpus --
--ignored --nocapture` runs the round trip over any directory of documents.
On 545 real documents from the Apache POI test suite (6 024 XML parts, 5 292
of them covered by the schemas), there are no failures and no differences;
the only reorderings are documents whose producer violated the schema order
(and `xsd:all` content, whose order is free).

## Performance

Release build, Apple silicon, single thread (`cargo run --release -p
openxml-schema --example bench`):

| Workload | Size | Parse | Write |
|----------|------|-------|-------|
| worksheet, 1 000 000 cells | 34 MB | ~70 MB/s | ~340 MB/s |
| document, 100 000 paragraphs | 20 MB | ~95 MB/s | ~230 MB/s |
