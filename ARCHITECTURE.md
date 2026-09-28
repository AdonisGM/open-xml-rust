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

* **`Ns`** — a compact `u16` identifier for each of the 38 namespaces in
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

## Test strategy

| Level | What is checked |
|-------|-----------------|
| unit tests | every module of every crate |
| schema behaviour | typed construction → XSD-valid XML; Strict→Transitional; MCE preservation; ordering; invalid values |
| corpus round trip | every XML part of every fixture: typed read → write → semantic diff = ∅, and writing twice is byte-identical |
| XSD validation | documents produced by the APIs validate against the ECMA schemas with `xmllint` |
| generated code freshness | regenerating from the XSDs reproduces the committed sources |
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
