//! Helpers shared by the worksheet features: sheet-qualified references,
//! `sqref` lists, sheet-local defined names and cell geometry.

use openxml_opc::{Package, PartName};
use openxml_schema::sml;
use openxml_xml::XmlList;

use crate::cell_ref::{CellRange, CellRef, MAX_COL, MAX_ROW, column_name};

/// EMUs per pixel at 96 dpi.
pub(crate) const EMU_PER_PX: f64 = 9525.0;

/// Width in pixels of a column without an explicit width (Calibri 11).
const DEFAULT_COLUMN_PX: f64 = 64.0;

/// Maximum digit width of the default font in pixels (Calibri 11).
const MAX_DIGIT_WIDTH: f64 = 7.0;

/// Whether a sheet name must be quoted in formulas.
fn needs_quotes(name: &str) -> bool {
    let plain = name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '.');
    // Names that read as references (A1, R1C1, XFD1) must be quoted too.
    let looks_like_ref = CellRef::parse(name).is_ok()
        || (name.len() >= 2
            && (name.starts_with(['R', 'r', 'C', 'c']))
            && name[1..]
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, 'C' | 'c')));
    !plain || looks_like_ref || name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false")
}

/// A sheet name as written before `!` in formulas: `Sheet1` or `'My Sheet'`.
pub(crate) fn quote_sheet(name: &str) -> String {
    if needs_quotes(name) {
        format!("'{}'", name.replace('\'', "''"))
    } else {
        name.to_owned()
    }
}

/// `$A$1`.
pub(crate) fn absolute_ref(r: CellRef) -> String {
    format!("${}${}", column_name(r.col()), r.row())
}

/// `$A$1:$B$5` (or `$A$1` for a single cell).
pub(crate) fn absolute_range(r: CellRange) -> String {
    if r.start() == r.end() {
        absolute_ref(r.start())
    } else {
        format!("{}:{}", absolute_ref(r.start()), absolute_ref(r.end()))
    }
}

/// `'Sheet'!$A$1:$B$5`.
pub(crate) fn qualified_range(sheet: &str, r: CellRange) -> String {
    format!("{}!{}", quote_sheet(sheet), absolute_range(r))
}

/// A `sqref` list.
pub(crate) fn to_sqref(ranges: &[CellRange]) -> XmlList<String> {
    XmlList(ranges.iter().map(ToString::to_string).collect())
}

/// Ranges of a `sqref` list (unparsable items are skipped).
pub(crate) fn from_sqref(list: &XmlList<String>) -> Vec<CellRange> {
    list.0.iter().filter_map(|s| CellRange::parse(s).ok()).collect()
}

/// A sheet-local defined name (such as `_xlnm.Print_Area`).
pub(crate) fn local_name<'w>(wb: &'w sml::CT_Workbook, name: &str, sheet: usize) -> Option<&'w str> {
    wb.defined_names
        .as_ref()?
        .defined_name
        .iter()
        .find(|n| {
            n.local_sheet_id == Some(sheet as u32)
                && n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name))
        })
        .map(|n| n.value.as_str())
}

/// Adds or replaces a sheet-local defined name.
pub(crate) fn set_local_name(
    wb: &mut sml::CT_Workbook,
    name: &str,
    sheet: usize,
    formula: String,
    hidden: bool,
) {
    let names = wb.defined_names.get_or_insert_with(Box::default);
    let sheet = Some(sheet as u32);
    match names.defined_name.iter_mut().find(|n| {
        n.local_sheet_id == sheet && n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name))
    }) {
        Some(n) => {
            n.value = formula;
            n.hidden = hidden.then_some(true);
        }
        None => names.defined_name.push(sml::CT_DefinedName {
            value: formula,
            name: Some(name.to_owned()),
            local_sheet_id: sheet,
            hidden: hidden.then_some(true),
            ..Default::default()
        }),
    }
}

/// Removes a sheet-local defined name. Returns whether it existed.
pub(crate) fn remove_local_name(wb: &mut sml::CT_Workbook, name: &str, sheet: usize) -> bool {
    let Some(names) = wb.defined_names.as_mut() else {
        return false;
    };
    let before = names.defined_name.len();
    names.defined_name.retain(|n| {
        !(n.local_sheet_id == Some(sheet as u32)
            && n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name)))
    });
    let removed = before != names.defined_name.len();
    if names.defined_name.is_empty() {
        wb.defined_names = None;
    }
    removed
}

/// Width of a column in pixels (the rendering Excel uses for a stored width).
pub(crate) fn column_px(ws: &sml::CT_Worksheet, col: u32) -> f64 {
    let explicit = ws
        .cols
        .iter()
        .flat_map(|c| c.col.iter())
        .find(|c| c.min.unwrap_or(0) <= col && col <= c.max.unwrap_or(0));
    if let Some(c) = explicit {
        if c.hidden == Some(true) {
            return 0.0;
        }
        if let Some(w) = c.width {
            return width_to_px(w);
        }
    }
    match ws.sheet_format_pr.as_ref().and_then(|f| f.default_col_width) {
        Some(w) => width_to_px(w),
        None => DEFAULT_COLUMN_PX,
    }
}

fn width_to_px(width: f64) -> f64 {
    (((256.0 * width + (128.0 / MAX_DIGIT_WIDTH).trunc()) / 256.0) * MAX_DIGIT_WIDTH).trunc()
}

/// Height of a row in pixels.
pub(crate) fn row_px(ws: &sml::CT_Worksheet, row: u32) -> f64 {
    let rows = ws.sheet_data.as_ref().map(|d| d.row.as_slice()).unwrap_or(&[]);
    let explicit = rows
        .binary_search_by_key(&row, |r| r.r.unwrap_or(0))
        .ok()
        .map(|i| &rows[i]);
    if let Some(r) = explicit {
        if r.hidden == Some(true) {
            return 0.0;
        }
        if let Some(ht) = r.ht {
            return ht * 96.0 / 72.0;
        }
    }
    let default_pt = ws
        .sheet_format_pr
        .as_ref()
        .and_then(|f| f.default_row_height)
        .unwrap_or(15.0);
    default_pt * 96.0 / 72.0
}

/// Left edge of column `col` and top edge of row `row` in EMU.
pub(crate) fn cell_origin_emu(ws: &sml::CT_Worksheet, cell: CellRef) -> (i64, i64) {
    let x: f64 = (1..cell.col()).map(|c| column_px(ws, c)).sum();
    let y: f64 = (1..cell.row()).map(|r| row_px(ws, r)).sum();
    ((x * EMU_PER_PX) as i64, (y * EMU_PER_PX) as i64)
}

/// The cell (1-based) and offset in EMU reached by moving `dx` EMU right
/// and `dy` EMU down from the top-left corner of `start`.
pub(crate) fn walk_emu(ws: &sml::CT_Worksheet, start: CellRef, dx: i64, dy: i64) -> (u32, i64, u32, i64) {
    let (mut col, mut rest_x) = (start.col(), dx.max(0));
    while col < MAX_COL {
        let w = (column_px(ws, col) * EMU_PER_PX) as i64;
        if rest_x < w {
            break;
        }
        rest_x -= w;
        col += 1;
    }
    let (mut row, mut rest_y) = (start.row(), dy.max(0));
    while row < MAX_ROW {
        let h = (row_px(ws, row) * EMU_PER_PX) as i64;
        if rest_y < h {
            break;
        }
        rest_y -= h;
        row += 1;
    }
    (col, rest_x, row, rest_y)
}

/// Whether any relationship in the package (other than those of `except`)
/// targets `target`.
pub(crate) fn is_referenced(pkg: &Package, target: &PartName, except: Option<&PartName>) -> bool {
    let hits = |source: Option<&PartName>| {
        pkg.relationships(source).is_some_and(|rels| {
            rels.iter()
                .any(|r| !r.is_external() && PartName::resolve(source, &r.target).is_ok_and(|t| &t == target))
        })
    };
    if hits(None) {
        return true;
    }
    pkg.parts()
        .map(|(name, _)| name)
        .filter(|name| Some(*name) != except)
        .any(|name| hits(Some(name)))
}

/// Removes relationship `rel_id` of `source`; the target part is removed
/// too (recursively) when nothing else references it.
pub(crate) fn remove_relationship_and_orphans(
    pkg: &mut Package,
    source: &PartName,
    rel_id: &str,
) -> Vec<PartName> {
    let target = pkg.relationship_target(Some(source), rel_id);
    if let Some(rels) = pkg.relationships_mut(Some(source)) {
        rels.remove(rel_id);
    }
    let mut removed = Vec::new();
    if let Some(t) = target {
        remove_if_orphan(pkg, &t, &mut removed);
    }
    removed
}

/// Removes `part` if no relationship targets it, then its own orphans.
pub(crate) fn remove_if_orphan(pkg: &mut Package, part: &PartName, removed: &mut Vec<PartName>) {
    if !pkg.contains(part) || is_referenced(pkg, part, None) {
        return;
    }
    let children: Vec<PartName> = pkg
        .relationships(Some(part))
        .map(|rels| {
            rels.iter()
                .filter(|r| !r.is_external())
                .filter_map(|r| PartName::resolve(Some(part), &r.target).ok())
                .collect()
        })
        .unwrap_or_default();
    pkg.remove_part(part);
    removed.push(part.clone());
    for child in children {
        remove_if_orphan(pkg, &child, removed);
    }
}

/// Escapes a text for use inside a formula string literal.
pub(crate) fn formula_string(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names_are_quoted_when_needed() {
        assert_eq!(quote_sheet("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet("Data_2024"), "Data_2024");
        assert_eq!(quote_sheet("My Sheet"), "'My Sheet'");
        assert_eq!(quote_sheet("O'Brien"), "'O''Brien'");
        assert_eq!(quote_sheet("2024"), "'2024'");
        assert_eq!(quote_sheet("A1"), "'A1'", "names that read as references");
        assert_eq!(quote_sheet("R1C1"), "'R1C1'");
        assert_eq!(quote_sheet("Dữ liệu"), "'Dữ liệu'");
        assert_eq!(quote_sheet("Tổng"), "Tổng");
        assert_eq!(quote_sheet("a-b"), "'a-b'");
    }

    #[test]
    fn references() {
        let r = CellRange::parse("B2:D5").unwrap();
        assert_eq!(absolute_range(r), "$B$2:$D$5");
        assert_eq!(absolute_range(CellRange::parse("C3").unwrap()), "$C$3");
        assert_eq!(qualified_range("My Sheet", r), "'My Sheet'!$B$2:$D$5");
        let sq = to_sqref(&[r, CellRange::parse("F1").unwrap()]);
        assert_eq!(sq.0, ["B2:D5", "F1"]);
        assert_eq!(from_sqref(&sq), [r, CellRange::parse("F1").unwrap()]);
        assert_eq!(formula_string("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn local_names() {
        let mut wb = sml::CT_Workbook::default();
        set_local_name(&mut wb, "_xlnm.Print_Area", 1, "S!$A$1".into(), false);
        set_local_name(&mut wb, "_xlnm.Print_Area", 1, "S!$A$1:$B$2".into(), false);
        set_local_name(&mut wb, "_xlnm._FilterDatabase", 1, "S!$A$1".into(), true);
        assert_eq!(local_name(&wb, "_xlnm.print_area", 1), Some("S!$A$1:$B$2"));
        assert_eq!(local_name(&wb, "_xlnm.Print_Area", 0), None);
        assert_eq!(wb.defined_names.as_ref().unwrap().defined_name.len(), 2);
        assert!(remove_local_name(&mut wb, "_xlnm.Print_Area", 1));
        assert!(!remove_local_name(&mut wb, "_xlnm.Print_Area", 1));
        assert!(remove_local_name(&mut wb, "_xlnm._FilterDatabase", 1));
        assert!(wb.defined_names.is_none());
    }

    #[test]
    fn geometry() {
        let mut ws = sml::CT_Worksheet::default();
        assert_eq!(column_px(&ws, 1), 64.0);
        assert_eq!(row_px(&ws, 1), 20.0);
        ws.cols.push(sml::CT_Cols {
            col: vec![sml::CT_Col {
                min: Some(2),
                max: Some(3),
                width: Some(9.140625),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(column_px(&ws, 2), 64.0, "the stored width of a 64 px column");
        ws.cols[0].col[0].width = Some(20.0);
        assert_eq!(column_px(&ws, 3), 140.0);
        let origin = cell_origin_emu(&ws, CellRef::parse("C3").unwrap());
        assert_eq!(origin, ((64.0 + 140.0) as i64 * 9525, 40 * 9525));
        // 100 px right of B1: 140 px wide column B holds it.
        let (col, dx, row, dy) = walk_emu(&ws, CellRef::parse("B1").unwrap(), 100 * 9525, 30 * 9525);
        assert_eq!((col, dx, row, dy), (2, 100 * 9525, 2, 10 * 9525));
        let (col, dx, ..) = walk_emu(&ws, CellRef::parse("B1").unwrap(), 150 * 9525, 0);
        assert_eq!((col, dx), (3, 10 * 9525));
    }
}
