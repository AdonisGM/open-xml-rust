//! Charts on worksheets (DrawingML charts from `openxml-chart`).

use openxml_chart::{Chart, ChartData, ChartInfo, ChartKind, Series, graphic_data, insert_chart, read_chart};
use openxml_core::{Error, Result};
use openxml_opc::known::rel_types;

use crate::cell_ref::{CellRef, ToCellRange};
use crate::drawing::Anchor;
use crate::worksheet::{Worksheet, WorksheetMut};

/// A sheet name as written in formulas (quoted when needed).
pub fn formula_sheet_name(name: &str) -> String {
    let plain = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn absolute(sheet: &str, from: CellRef, to: CellRef) -> String {
    let (a, b) = (from.column_name(), to.column_name());
    if from == to {
        format!("{sheet}!${a}${}", from.row())
    } else {
        format!("{sheet}!${a}${}:${b}${}", from.row(), to.row())
    }
}

fn cell(row: u32, col: u32) -> Result<CellRef> {
    CellRef::new(row, col)
}

impl Worksheet<'_> {
    /// Builds a chart from a block of cells: the first row holds the series
    /// names, the first column the categories (x values for scatter charts)
    /// and every other column one series. The series reference the cells, so
    /// the chart follows later edits in Excel.
    pub fn chart_from_range(&self, kind: ChartKind, range: impl ToCellRange) -> Result<Chart> {
        let range = range.to_cell_range()?;
        let (start, end) = (range.start(), range.end());
        if end.row() <= start.row() || end.col() <= start.col() {
            return Err(Error::InvalidArgument(format!(
                "a chart range needs a header row and a label column: {range:?}"
            )));
        }
        let sheet = formula_sheet_name(self.name());
        let first_data_row = start.row() + 1;
        let label_col = start.col();
        let labels = (first_data_row..=end.row())
            .map(|r| Ok(self.cell(cell(r, label_col)?)?.result().to_string()))
            .collect::<Result<Vec<String>>>()?;
        let labels_ref = absolute(
            &sheet,
            cell(first_data_row, label_col)?,
            cell(end.row(), label_col)?,
        );
        let mut chart = Chart::new(kind);
        if kind == ChartKind::Scatter {
            chart.categories = Vec::new();
        } else {
            chart = chart
                .categories(labels.clone())
                .categories_ref(labels_ref.clone());
        }
        let xs: Vec<f64> = labels.iter().map(|l| l.parse().unwrap_or(f64::NAN)).collect();
        for col in label_col + 1..=end.col() {
            let name = self.cell(cell(start.row(), col)?)?.result().to_string();
            let values = (first_data_row..=end.row())
                .map(|r| Ok(self.cell(cell(r, col)?)?.result().as_f64().unwrap_or(f64::NAN)))
                .collect::<Result<Vec<f64>>>()?;
            let name_ref = absolute(&sheet, cell(start.row(), col)?, cell(start.row(), col)?);
            let values_ref = absolute(&sheet, cell(first_data_row, col)?, cell(end.row(), col)?);
            let mut series = Series::new(name, values);
            if kind == ChartKind::Scatter {
                series =
                    series
                        .x_values(xs.clone())
                        .references(Some(&name_ref), &values_ref, Some(&labels_ref));
            } else {
                series = series.references(Some(&name_ref), &values_ref, None);
            }
            chart = chart.series(series);
        }
        Ok(chart)
    }

    /// The charts drawn on this sheet.
    pub fn charts(&self) -> Result<Vec<ChartInfo>> {
        let Some(drawing) = self.drawing_part() else {
            return Ok(Vec::new());
        };
        let pkg = self.env.package;
        pkg.related_parts(Some(&drawing), rel_types::CHART)
            .iter()
            .map(|part| {
                let data = pkg
                    .part(part)
                    .ok_or_else(|| Error::MissingPart(part.to_string()))?;
                read_chart(data.data())
            })
            .collect()
    }
}

impl WorksheetMut<'_> {
    /// Builds a chart from a block of cells (see [`Worksheet::chart_from_range`]).
    pub fn chart_from_range(&self, kind: ChartKind, range: impl ToCellRange) -> Result<Chart> {
        self.as_view().chart_from_range(kind, range)
    }

    /// Draws a chart on the sheet at `anchor`. Returns the drawing object id.
    ///
    /// ```
    /// use openxml_xlsx::{Anchor, AnchorPoint, CellRef, ChartKind, EditAs, Workbook};
    ///
    /// let mut wb = Workbook::new();
    /// let mut sheet = wb.worksheet_mut("Sheet1")?;
    /// sheet.set_value("B1", "Sales")?;
    /// for (i, (month, v)) in [("Jan", 10.0), ("Feb", 14.0), ("Mar", 12.0)].into_iter().enumerate() {
    ///     sheet.set_value(format!("A{}", i + 2).as_str(), month)?;
    ///     sheet.set_value(format!("B{}", i + 2).as_str(), v)?;
    /// }
    /// let chart = sheet.chart_from_range(ChartKind::Column, "A1:B4")?.title("Sales");
    /// let anchor = Anchor::TwoCell {
    ///     from: AnchorPoint::at(CellRef::parse("D2")?),
    ///     to: AnchorPoint::at(CellRef::parse("K16")?),
    ///     edit_as: EditAs::TwoCell,
    /// };
    /// sheet.add_chart(&chart, &anchor)?;
    /// let wb = Workbook::from_bytes(&wb.to_bytes()?)?;
    /// let charts = wb.worksheet("Sheet1")?.charts()?;
    /// assert_eq!(charts[0].plots[0].series[0].values_ref.as_deref(), Some("Sheet1!$B$2:$B$4"));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_chart(&mut self, chart: &Chart, anchor: &Anchor) -> Result<u32> {
        let drawing = self.drawing_part()?;
        let inserted = insert_chart(self.package, &drawing, "/xl", chart, ChartData::InChart)?;
        let count = self.package.related_parts(Some(&drawing), rel_types::CHART).len();
        self.add_graphic_frame(anchor, &format!("Chart {count}"), graphic_data(&inserted.rel_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names_in_formulas() {
        assert_eq!(formula_sheet_name("Sheet1"), "Sheet1");
        assert_eq!(formula_sheet_name("My Data"), "'My Data'");
        assert_eq!(formula_sheet_name("O'Brien"), "'O''Brien'");
        assert_eq!(formula_sheet_name("2024"), "'2024'");
        assert_eq!(formula_sheet_name("Bảng"), "'Bảng'");
    }

    #[test]
    fn absolute_references() {
        let a = CellRef::parse("B2").unwrap();
        let b = CellRef::parse("B9").unwrap();
        assert_eq!(absolute("S", a, b), "S!$B$2:$B$9");
        assert_eq!(absolute("S", a, a), "S!$B$2");
    }
}
