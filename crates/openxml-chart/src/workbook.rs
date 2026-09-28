//! The workbook embedded next to a chart in Word and PowerPoint documents,
//! holding the chart data so that "Edit Data" works in Office.

use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::sml::{
    self, CT_Cell, CT_Row, CT_Rst, CT_Sheet, CT_SheetData, CT_Sheets, CT_Workbook, CT_Worksheet, ST_CellType,
};

use crate::build::{DATA_SHEET, column_name, scatter_x};
use crate::spec::{Chart, ChartKind};

/// A minimal stylesheet (one font, the two mandatory fills, one border and
/// one cell format).
const STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    "\r\n",
    r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts>"#,
    r#"<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>"#,
    r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
    r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
    r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
    r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
    r#"</styleSheet>"#
);

fn text_cell(r: &str, text: &str) -> CT_Cell {
    CT_Cell {
        r: Some(r.to_owned()),
        t: Some(ST_CellType::InlineStr),
        is: Some(Box::new(CT_Rst {
            t: Some(text.to_owned()),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn number_cell(r: &str, v: f64) -> Option<CT_Cell> {
    v.is_finite().then(|| CT_Cell {
        r: Some(r.to_owned()),
        v: Some(format!("{v}")),
        ..Default::default()
    })
}

/// The data grid of a chart: a header row with the series names and one row
/// per point. Category charts put the categories in column A and series in
/// B, C, …; scatter charts put each series in a pair of columns (x, y).
pub fn data_sheet(chart: &Chart) -> CT_Worksheet {
    let points = chart.point_count();
    let mut rows: Vec<CT_Row> = (1..=points as u32 + 1)
        .map(|r| CT_Row {
            r: Some(r),
            ..Default::default()
        })
        .collect();
    let at = |col: u32, row: u32| format!("{}{row}", column_name(col));
    if chart.kind == ChartKind::Scatter {
        for (i, s) in chart.series.iter().enumerate() {
            let (xc, yc) = (2 * i as u32 + 1, 2 * i as u32 + 2);
            rows[0].c.push(text_cell(&at(xc, 1), "X"));
            rows[0].c.push(text_cell(&at(yc, 1), &s.name));
            for (p, x) in scatter_x(s).iter().enumerate() {
                rows[p + 1].c.extend(number_cell(&at(xc, p as u32 + 2), *x));
            }
            for (p, y) in s.values.iter().enumerate() {
                rows[p + 1].c.extend(number_cell(&at(yc, p as u32 + 2), *y));
            }
        }
    } else {
        for (i, s) in chart.series.iter().enumerate() {
            rows[0].c.push(text_cell(&at(i as u32 + 2, 1), &s.name));
        }
        for (p, row) in rows.iter_mut().skip(1).enumerate() {
            if let Some(label) = chart.categories.get(p) {
                row.c.push(text_cell(&at(1, p as u32 + 2), label));
            }
            for (i, s) in chart.series.iter().enumerate() {
                if let Some(v) = s.values.get(p) {
                    row.c.extend(number_cell(&at(i as u32 + 2, p as u32 + 2), *v));
                }
            }
        }
    }
    CT_Worksheet {
        sheet_data: Some(Box::new(CT_SheetData {
            row: rows,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Builds the `.xlsx` package that holds the chart data.
pub fn embedded_workbook(chart: &Chart) -> Vec<u8> {
    let mut pkg = Package::new();
    let workbook = PartName::new("/xl/workbook.xml").expect("valid name");
    let sheet = PartName::new("/xl/worksheets/sheet1.xml").expect("valid name");
    let styles = PartName::new("/xl/styles.xml").expect("valid name");
    let wb = CT_Workbook {
        sheets: Some(Box::new(CT_Sheets {
            sheet: vec![CT_Sheet {
                name: Some(DATA_SHEET.into()),
                sheet_id: Some(1),
                r_id: Some("rId1".into()),
                ..Default::default()
            }],
            ..Default::default()
        })),
        ..Default::default()
    };
    pkg.add_part(
        workbook.clone(),
        ct::SML_WORKBOOK,
        sml::elements::WORKBOOK.to_bytes(&wb),
    )
    .expect("new part");
    pkg.add_relationship(None, rel_types::OFFICE_DOCUMENT, &workbook)
        .expect("package rels");
    pkg.add_part(
        sheet.clone(),
        ct::SML_WORKSHEET,
        sml::elements::WORKSHEET.to_bytes(&data_sheet(chart)),
    )
    .expect("new part");
    let id = pkg
        .add_relationship(Some(&workbook), rel_types::WORKSHEET, &sheet)
        .expect("workbook rels");
    debug_assert_eq!(id, "rId1");
    pkg.add_part(styles.clone(), ct::SML_STYLES, STYLES.as_bytes().to_vec())
        .expect("new part");
    pkg.add_relationship(Some(&workbook), rel_types::STYLES, &styles)
        .expect("workbook rels");
    pkg.to_bytes().expect("in-memory package")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Series;

    #[test]
    fn category_layout() {
        let c = Chart::new(ChartKind::Column)
            .categories(["a", "b"])
            .series(Series::new("s1", [1.0, 2.0]))
            .series(Series::new("s2", [3.0, f64::NAN]));
        let ws = data_sheet(&c);
        let rows = &ws.sheet_data.as_ref().unwrap().row;
        assert_eq!(rows.len(), 3);
        let refs: Vec<_> = rows
            .iter()
            .flat_map(|r| r.c.iter().map(|c| c.r.clone().unwrap()))
            .collect();
        assert_eq!(
            refs,
            ["B1", "C1", "A2", "B2", "C2", "A3", "B3"],
            "missing points leave empty cells"
        );
        assert_eq!(rows[1].c[1].v.as_deref(), Some("1"));
    }

    #[test]
    fn scatter_layout() {
        let c = Chart::new(ChartKind::Scatter).series(Series::new("p", [5.0, 6.0]).x_values([0.5, 1.5]));
        let ws = data_sheet(&c);
        let rows = &ws.sheet_data.as_ref().unwrap().row;
        assert_eq!(rows[1].c[0].r.as_deref(), Some("A2"));
        assert_eq!(rows[1].c[0].v.as_deref(), Some("0.5"));
        assert_eq!(rows[1].c[1].v.as_deref(), Some("5"));
    }

    #[test]
    fn workbook_package_structure() {
        let c = Chart::new(ChartKind::Pie)
            .categories(["x"])
            .series(Series::new("s", [1.0]));
        let pkg = Package::from_bytes(&embedded_workbook(&c)).unwrap();
        let main = pkg.main_part().unwrap();
        assert_eq!(main.as_str(), "/xl/workbook.xml");
        assert_eq!(pkg.related_parts(Some(&main), rel_types::WORKSHEET).len(), 1);
        assert!(pkg.related_part(Some(&main), rel_types::STYLES).is_some());
    }
}
