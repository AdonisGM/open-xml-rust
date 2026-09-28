# Test fixtures

| Directory | Contents |
|-----------|----------|
| `ecma/`   | `PivotTableFormats.xlsx` from the ECMA-376 Part 1 distribution (`OfficeOpenXML-SpreadsheetMLStyles.zip`). |
| `poi/`    | Real-world documents from Apache POI's test data (see `poi/NOTICE.md`). |

`crates/openxml-schema/tests/corpus.rs` round-trips every XML part of every
file here. Point `OPENXML_CORPUS` at a larger directory to run the same test
over a local corpus.
