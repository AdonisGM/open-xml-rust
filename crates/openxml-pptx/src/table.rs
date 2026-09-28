//! Tables: DrawingML `a:tbl` hosted by a graphic frame.

use openxml_core::{Error, Length, Result};
use openxml_schema::{dml, pml};
use openxml_xml::{Ns, RawElement};

use crate::shape::{self, TABLE_URI, coord};
use crate::template::DEFAULT_TABLE_STYLE;
use crate::text;

/// Reads the table hosted by a graphic frame.
pub(crate) fn frame_table(frame: &pml::CT_GraphicalObjectFrame) -> Option<dml::CT_Table> {
    let data = frame.graphic.as_ref()?.graphic_data.as_ref()?;
    if data.uri.as_deref() != Some(TABLE_URI) {
        return None;
    }
    let raw = data.any.iter().find(|e| e.name.is(Ns::A, "tbl"))?;
    raw.to_typed().ok()
}

/// Stores `table` into a graphic frame (replacing the previous table).
pub(crate) fn store_table(frame: &mut pml::CT_GraphicalObjectFrame, table: &dml::CT_Table) {
    let graphic = frame.graphic.get_or_insert_with(Box::default);
    let data = graphic.graphic_data.get_or_insert_with(Box::default);
    data.uri = Some(TABLE_URI.to_owned());
    let raw = RawElement::from_typed(table, Ns::A, "tbl");
    match data.any.iter_mut().find(|e| e.name.is(Ns::A, "tbl")) {
        Some(slot) => *slot = raw,
        None => data.any.push(raw),
    }
}

/// Text of a table: cells separated by `\t`, rows by `\n`.
pub(crate) fn table_text(table: &dml::CT_Table) -> String {
    table
        .tr
        .iter()
        .map(|row| {
            row.tc
                .iter()
                .map(|c| c.tx_body.as_deref().map(text::body_text).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn empty_cell() -> dml::CT_TableCell {
    dml::CT_TableCell {
        tx_body: Some(Box::new(text::text_body(Vec::new()))),
        tc_pr: Some(Box::default()),
        ..Default::default()
    }
}

/// A graphic frame holding an empty `rows` × `cols` table with the default table style.
pub(crate) fn new_table_frame(
    id: u32,
    rows: usize,
    cols: usize,
    x: Length,
    y: Length,
    w: Length,
    h: Length,
) -> Result<pml::CT_GraphicalObjectFrame> {
    if rows == 0 || cols == 0 {
        return Err(Error::InvalidArgument(
            "a table needs at least one row and one column".into(),
        ));
    }
    let col_w = w / cols as i64;
    let row_h = h / rows as i64;
    let table = dml::CT_Table {
        tbl_pr: Some(Box::new(dml::CT_TableProperties {
            first_row: Some(true),
            band_row: Some(true),
            choice: Some(dml::CT_TableProperties_Choice::TableStyleId(
                DEFAULT_TABLE_STYLE.to_owned(),
            )),
            ..Default::default()
        })),
        tbl_grid: Some(Box::new(dml::CT_TableGrid {
            grid_col: (0..cols)
                .map(|_| dml::CT_TableCol {
                    w: Some(coord(col_w)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })),
        tr: (0..rows)
            .map(|_| dml::CT_TableRow {
                h: Some(coord(row_h)),
                tc: (0..cols).map(|_| empty_cell()).collect(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let mut frame = pml::CT_GraphicalObjectFrame {
        nv_graphic_frame_pr: Some(Box::new(pml::CT_GraphicalObjectFrameNonVisual {
            c_nv_pr: Some(Box::new(shape::nv_props(
                id,
                &format!("Table {}", id.saturating_sub(1)),
            ))),
            c_nv_graphic_frame_pr: Some(Box::new(dml::CT_NonVisualGraphicFrameProperties {
                graphic_frame_locks: Some(Box::new(dml::CT_GraphicalObjectFrameLocking {
                    no_grp: Some(true),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            nv_pr: Some(Box::default()),
            ..Default::default()
        })),
        xfrm: Some(Box::new(shape::transform(x, y, w, h))),
        ..Default::default()
    };
    store_table(&mut frame, &table);
    Ok(frame)
}

/// Mutable access to a table on a slide.
///
/// The table is stored as DrawingML inside a graphic frame; each operation
/// reads, edits and stores it back.
pub struct TableMut<'a> {
    frame: &'a mut pml::CT_GraphicalObjectFrame,
}

impl<'a> TableMut<'a> {
    pub(crate) fn new(frame: &'a mut pml::CT_GraphicalObjectFrame) -> Self {
        TableMut { frame }
    }

    /// Identifier of the graphic frame.
    pub fn id(&self) -> u32 {
        self.frame
            .nv_graphic_frame_pr
            .as_ref()
            .and_then(|n| n.c_nv_pr.as_ref())
            .and_then(|c| c.id)
            .unwrap_or(0)
    }

    fn table(&self) -> dml::CT_Table {
        frame_table(self.frame).unwrap_or_default()
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.table().tr.len()
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.table().tbl_grid.map(|g| g.grid_col.len()).unwrap_or(0)
    }

    /// Text of a cell.
    pub fn cell_text(&self, row: usize, col: usize) -> Option<String> {
        let t = self.table();
        let cell = t.tr.get(row)?.tc.get(col)?;
        Some(cell.tx_body.as_deref().map(text::body_text).unwrap_or_default())
    }

    /// Edits the table with a closure (escape hatch for anything not covered here).
    pub fn with_table<R>(&mut self, f: impl FnOnce(&mut dml::CT_Table) -> R) -> R {
        let mut t = self.table();
        let result = f(&mut t);
        store_table(self.frame, &t);
        result
    }

    /// Sets the text of a cell (one paragraph per line).
    pub fn set_cell_text(&mut self, row: usize, col: usize, value: &str) -> Result<&mut Self> {
        self.with_table(|t| {
            let cell =
                t.tr.get_mut(row)
                    .and_then(|r| r.tc.get_mut(col))
                    .ok_or_else(|| Error::NotFound(format!("table cell ({row}, {col})")))?;
            let body = cell
                .tx_body
                .get_or_insert_with(|| Box::new(text::text_body(Vec::new())));
            text::set_paragraphs(body, text::paragraphs_from_text(value));
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Fills the table row by row from nested iterators; extra values are an error.
    pub fn set_values<R, C, S>(&mut self, rows: R) -> Result<&mut Self>
    where
        R: IntoIterator<Item = C>,
        C: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for (r, row) in rows.into_iter().enumerate() {
            for (c, value) in row.into_iter().enumerate() {
                self.set_cell_text(r, c, value.as_ref())?;
            }
        }
        Ok(self)
    }

    /// Sets the width of a column.
    pub fn set_column_width(&mut self, col: usize, width: Length) -> Result<&mut Self> {
        self.with_table(|t| {
            let grid = t.tbl_grid.get_or_insert_with(Box::default);
            let c = grid
                .grid_col
                .get_mut(col)
                .ok_or_else(|| Error::NotFound(format!("table column {col}")))?;
            c.w = Some(coord(width));
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Turns the special formatting of the first (header) row on or off.
    pub fn set_header_row(&mut self, on: bool) -> &mut Self {
        self.with_table(|t| t.tbl_pr.get_or_insert_with(Box::default).first_row = Some(on));
        self
    }

    /// Text of the whole table (cells separated by `\t`, rows by `\n`).
    pub fn text(&self) -> String {
        table_text(&self.table())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> pml::CT_GraphicalObjectFrame {
        new_table_frame(
            4,
            2,
            3,
            Length::cm(1.0),
            Length::cm(1.0),
            Length::cm(9.0),
            Length::cm(2.0),
        )
        .unwrap()
    }

    #[test]
    fn new_tables_have_the_requested_shape() {
        let f = frame();
        let t = frame_table(&f).unwrap();
        assert_eq!(t.tr.len(), 2);
        assert_eq!(t.tr[0].tc.len(), 3);
        assert_eq!(t.tbl_grid.as_ref().unwrap().grid_col.len(), 3);
        assert_eq!(
            t.tbl_grid.as_ref().unwrap().grid_col[0].w,
            Some(coord(Length::cm(3.0)))
        );
        let pr = t.tbl_pr.as_ref().unwrap();
        assert_eq!(pr.first_row, Some(true));
        assert!(
            matches!(&pr.choice, Some(dml::CT_TableProperties_Choice::TableStyleId(id)) if id == DEFAULT_TABLE_STYLE)
        );
        assert!(new_table_frame(1, 0, 1, Length::ZERO, Length::ZERO, Length::ZERO, Length::ZERO).is_err());
    }

    #[test]
    fn editing_cells() {
        let mut f = frame();
        let mut t = TableMut::new(&mut f);
        assert_eq!(t.id(), 4);
        assert_eq!((t.rows(), t.cols()), (2, 3));
        t.set_values([["a", "b", "c"], ["d", "e", "f"]]).unwrap();
        t.set_cell_text(1, 2, "multi\nline").unwrap();
        assert_eq!(t.cell_text(0, 1).as_deref(), Some("b"));
        assert_eq!(t.cell_text(1, 2).as_deref(), Some("multi\nline"));
        assert_eq!(t.cell_text(5, 0), None);
        assert!(matches!(t.set_cell_text(2, 0, "x"), Err(Error::NotFound(_))));
        assert!(t.set_values([["1", "2", "3", "4"]]).is_err());
        t.set_column_width(0, Length::cm(5.0)).unwrap();
        assert!(t.set_column_width(9, Length::cm(5.0)).is_err());
        t.set_header_row(false);
        assert_eq!(t.text(), "1\t2\t3\nd\te\tmulti\nline");
        let n = t.with_table(|tbl| tbl.tr.len());
        assert_eq!(n, 2);
        let stored = frame_table(&f).unwrap();
        assert_eq!(stored.tbl_pr.as_ref().unwrap().first_row, Some(false));
        assert_eq!(
            stored.tbl_grid.as_ref().unwrap().grid_col[0].w,
            Some(coord(Length::cm(5.0)))
        );
    }

    #[test]
    fn frames_without_tables() {
        let f = pml::CT_GraphicalObjectFrame::default();
        assert!(frame_table(&f).is_none());
        let mut chart = frame();
        chart.graphic.as_mut().unwrap().graphic_data.as_mut().unwrap().uri = Some(shape::CHART_URI.into());
        assert!(frame_table(&chart).is_none());
    }
}
