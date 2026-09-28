# Apache POI test documents

The workbooks in this directory are copied unmodified from the
`test-data/spreadsheet` directory of the Apache POI project
(<https://github.com/apache/poi/tree/trunk/test-data>), which is distributed
under the Apache License, Version 2.0
(<https://www.apache.org/licenses/LICENSE-2.0>).

Apache POI — Copyright The Apache Software Foundation.

They are used only as test inputs for `tests/feature_fixtures.rs`:

| File | Used for |
| --- | --- |
| `NewStyleConditionalFormattings.xlsx` | cell-value rules, data bars, color scales, icon sets, dxf styles |
| `WithConditionalFormatting.xlsx` | expression rules, `stopIfTrue`, text operands |
| `workbookProtection-sheet_password-2013.xlsx` | SHA-512 sheet password (`pwd`) |
| `sheetProtection_allLocked.xlsx` | sheet protection flags |
| `SheetTabColors.xlsx` | indexed and RGB tab colours |
| `53282.xlsx` | external hyperlinks |
| `49156.xlsx` | manual row breaks, calculation chain |
| `absolute-anchor-over-empty-sheet.xlsx` | absolute picture anchors |
| `56502.xlsx` | outline summary settings |
