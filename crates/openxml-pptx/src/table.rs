//! Tables: DrawingML `a:tbl` hosted by a graphic frame.

use openxml_core::{Error, Length, Result};
use openxml_schema::{dml, pml};
use openxml_xml::{Ns, RawElement};

use crate::drawing::{Fill, Line};
use crate::format::TextAnchor;
use crate::paragraph::ParagraphMut;
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

/// Built-in table styles of PowerPoint, identified by GUID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TableStyle {
    /// Medium Style 2 – Accent 1 (the default of new tables).
    MediumStyle2Accent1,
    /// Medium Style 2.
    MediumStyle2,
    /// Medium Style 1 – Accent 1.
    MediumStyle1Accent1,
    /// Light Style 1 – Accent 1.
    LightStyle1Accent1,
    /// Light Style 2 – Accent 1.
    LightStyle2Accent1,
    /// Themed Style 1 – Accent 1.
    ThemedStyle1Accent1,
    /// Dark Style 1.
    DarkStyle1,
    /// No Style, No Grid.
    NoStyleNoGrid,
    /// No Style, Table Grid.
    NoStyleTableGrid,
    /// Any other style, by GUID (`{…}`).
    Custom(String),
}

impl TableStyle {
    const KNOWN: [(TableStyle, &'static str); 9] = [
        (
            TableStyle::MediumStyle2Accent1,
            "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}",
        ),
        (TableStyle::MediumStyle2, "{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}"),
        (
            TableStyle::MediumStyle1Accent1,
            "{B301B821-A1FF-4177-AEE7-76D212191A09}",
        ),
        (
            TableStyle::LightStyle1Accent1,
            "{3B4B98B0-60AC-42C2-AFA5-B58CD77FA1E5}",
        ),
        (
            TableStyle::LightStyle2Accent1,
            "{69012ECD-51FC-41F1-AA8D-1B2483CD663E}",
        ),
        (
            TableStyle::ThemedStyle1Accent1,
            "{3C2FFA5D-87B4-456A-9821-1D502468CF0F}",
        ),
        (TableStyle::DarkStyle1, "{E8034E78-7F5D-4C2E-B375-FC64B27BC917}"),
        (
            TableStyle::NoStyleNoGrid,
            "{2D5ABB26-0587-4C30-8999-92F81FD0307C}",
        ),
        (
            TableStyle::NoStyleTableGrid,
            "{5940675A-B579-460E-94D1-54222C63F5DA}",
        ),
    ];

    /// The style's GUID.
    pub fn guid(&self) -> &str {
        match self {
            TableStyle::Custom(g) => g,
            known => TableStyle::KNOWN
                .iter()
                .find(|(s, _)| s == known)
                .map(|(_, g)| *g)
                .expect("every named style has a GUID"),
        }
    }

    /// The style with this GUID (case-insensitive).
    pub fn from_guid(guid: &str) -> TableStyle {
        TableStyle::KNOWN
            .iter()
            .find(|(_, g)| g.eq_ignore_ascii_case(guid))
            .map(|(s, _)| s.clone())
            .unwrap_or_else(|| TableStyle::Custom(guid.to_owned()))
    }
}

/// Which parts of the table the style emphasises ("Table Style Options").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TableFlags {
    /// Header row.
    pub first_row: bool,
    /// Total row.
    pub last_row: bool,
    /// First column.
    pub first_column: bool,
    /// Last column.
    pub last_column: bool,
    /// Banded rows.
    pub banded_rows: bool,
    /// Banded columns.
    pub banded_columns: bool,
}

/// A border of a table cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellBorder {
    /// Left edge.
    Left,
    /// Right edge.
    Right,
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
    /// Diagonal from the top-left to the bottom-right corner.
    DiagonalDown,
    /// Diagonal from the bottom-left to the top-right corner.
    DiagonalUp,
}

fn cell_mut(t: &mut dml::CT_Table, row: usize, col: usize) -> Result<&mut dml::CT_TableCell> {
    t.tr.get_mut(row)
        .and_then(|r| r.tc.get_mut(col))
        .ok_or_else(|| Error::NotFound(format!("table cell ({row}, {col})")))
}

fn cell_pr(t: &mut dml::CT_Table, row: usize, col: usize) -> Result<&mut dml::CT_TableCellProperties> {
    Ok(cell_mut(t, row, col)?.tc_pr.get_or_insert_with(Box::default))
}

fn coord_len(c: Option<&dml::ST_Coordinate>) -> Length {
    c.and_then(shape::coordinate).unwrap_or(Length::ZERO)
}

fn coord32(v: Length) -> dml::ST_Coordinate32 {
    dml::ST_Coordinate32::Coordinate32Unqualified(v.as_emu().clamp(0, i64::from(i32::MAX)) as i32)
}

impl TableMut<'_> {
    /// Keeps the frame size equal to the sum of the column widths and row heights.
    fn sync_frame_size(&mut self) {
        let t = self.table();
        let w: i64 = t
            .tbl_grid
            .iter()
            .flat_map(|g| &g.grid_col)
            .map(|c| coord_len(c.w.as_ref()).as_emu())
            .sum();
        let h: i64 = t.tr.iter().map(|r| coord_len(r.h.as_ref()).as_emu()).sum();
        let xfrm = self.frame.xfrm.get_or_insert_with(Box::default);
        xfrm.ext = Some(Box::new(dml::CT_PositiveSize2D {
            cx: Some(w),
            cy: Some(h),
            ..Default::default()
        }));
    }

    /// Merges the cells of the rectangle from `(first_row, first_col)` to
    /// `(last_row, last_col)` (inclusive) into its top-left cell.
    pub fn merge_cells(
        &mut self,
        first_row: usize,
        first_col: usize,
        last_row: usize,
        last_col: usize,
    ) -> Result<&mut Self> {
        if last_row < first_row || last_col < first_col {
            return Err(Error::InvalidArgument("empty merge range".into()));
        }
        self.with_table(|t| {
            cell_mut(t, last_row, last_col)?;
            // Merged areas may not overlap.
            for r in first_row..=last_row {
                for c in first_col..=last_col {
                    let cell = cell_mut(t, r, c)?;
                    if cell.grid_span.unwrap_or(1) > 1
                        || cell.row_span.unwrap_or(1) > 1
                        || cell.h_merge == Some(true)
                        || cell.v_merge == Some(true)
                    {
                        return Err(Error::InvalidArgument(format!(
                            "cell ({r}, {c}) is already merged"
                        )));
                    }
                }
            }
            for r in first_row..=last_row {
                for c in first_col..=last_col {
                    let cell = cell_mut(t, r, c)?;
                    if r == first_row && c == first_col {
                        cell.grid_span = (last_col > first_col).then(|| (last_col - first_col + 1) as i32);
                        cell.row_span = (last_row > first_row).then(|| (last_row - first_row + 1) as i32);
                    } else {
                        cell.h_merge = (c > first_col).then_some(true);
                        cell.v_merge = (r > first_row).then_some(true);
                    }
                }
            }
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Splits the merged area anchored at `(row, col)` back into single cells.
    pub fn split_cell(&mut self, row: usize, col: usize) -> Result<&mut Self> {
        self.with_table(|t| {
            let anchor = cell_mut(t, row, col)?;
            let cols = anchor.grid_span.unwrap_or(1).max(1) as usize;
            let rows = anchor.row_span.unwrap_or(1).max(1) as usize;
            for r in row..row + rows {
                for c in col..col + cols {
                    if let Ok(cell) = cell_mut(t, r, c) {
                        cell.grid_span = None;
                        cell.row_span = None;
                        cell.h_merge = None;
                        cell.v_merge = None;
                    }
                }
            }
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Merge information of a cell: `(row span, column span, covered)`, where
    /// `covered` means the cell is hidden by a merged neighbour.
    pub fn cell_span(&self, row: usize, col: usize) -> Option<(u32, u32, bool)> {
        let t = self.table();
        let cell = t.tr.get(row)?.tc.get(col)?;
        Some((
            cell.row_span.unwrap_or(1).max(1) as u32,
            cell.grid_span.unwrap_or(1).max(1) as u32,
            cell.h_merge == Some(true) || cell.v_merge == Some(true),
        ))
    }

    /// Sets the fill of a cell.
    pub fn set_cell_fill(&mut self, row: usize, col: usize, fill: Fill) -> Result<&mut Self> {
        self.with_table(|t| {
            cell_pr(t, row, col)?.fill_properties = Some(fill.to_dml());
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// The fill of a cell, when set on the cell.
    pub fn cell_fill(&self, row: usize, col: usize) -> Option<Fill> {
        let t = self.table();
        Fill::from_dml(
            t.tr.get(row)?
                .tc
                .get(col)?
                .tc_pr
                .as_ref()?
                .fill_properties
                .as_ref()?,
        )
    }

    /// Sets one border of a cell.
    pub fn set_cell_border(
        &mut self,
        row: usize,
        col: usize,
        border: CellBorder,
        line: Line,
    ) -> Result<&mut Self> {
        self.with_table(|t| {
            let pr = cell_pr(t, row, col)?;
            let slot = match border {
                CellBorder::Left => &mut pr.ln_l,
                CellBorder::Right => &mut pr.ln_r,
                CellBorder::Top => &mut pr.ln_t,
                CellBorder::Bottom => &mut pr.ln_b,
                CellBorder::DiagonalDown => &mut pr.ln_tl_to_br,
                CellBorder::DiagonalUp => &mut pr.ln_bl_to_tr,
            };
            *slot = Some(Box::new(line.to_dml()));
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// One border of a cell, when set on the cell.
    pub fn cell_border(&self, row: usize, col: usize, border: CellBorder) -> Option<Line> {
        let t = self.table();
        let pr = t.tr.get(row)?.tc.get(col)?.tc_pr.as_deref()?;
        let ln = match border {
            CellBorder::Left => pr.ln_l.as_deref(),
            CellBorder::Right => pr.ln_r.as_deref(),
            CellBorder::Top => pr.ln_t.as_deref(),
            CellBorder::Bottom => pr.ln_b.as_deref(),
            CellBorder::DiagonalDown => pr.ln_tl_to_br.as_deref(),
            CellBorder::DiagonalUp => pr.ln_bl_to_tr.as_deref(),
        }?;
        Some(Line::from_dml(ln))
    }

    /// Sets all four outer borders of a cell.
    pub fn set_cell_borders(&mut self, row: usize, col: usize, line: Line) -> Result<&mut Self> {
        for b in [
            CellBorder::Left,
            CellBorder::Right,
            CellBorder::Top,
            CellBorder::Bottom,
        ] {
            self.set_cell_border(row, col, b, line.clone())?;
        }
        Ok(self)
    }

    /// Sets the inner margins of a cell.
    pub fn set_cell_margins(
        &mut self,
        row: usize,
        col: usize,
        left: Length,
        top: Length,
        right: Length,
        bottom: Length,
    ) -> Result<&mut Self> {
        self.with_table(|t| {
            let pr = cell_pr(t, row, col)?;
            pr.mar_l = Some(coord32(left));
            pr.mar_t = Some(coord32(top));
            pr.mar_r = Some(coord32(right));
            pr.mar_b = Some(coord32(bottom));
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Inner margins `(left, top, right, bottom)` of a cell, defaults applied
    /// (0.1" left/right, 0.05" top/bottom).
    pub fn cell_margins(&self, row: usize, col: usize) -> Option<(Length, Length, Length, Length)> {
        let t = self.table();
        let pr = t.tr.get(row)?.tc.get(col)?.tc_pr.clone().unwrap_or_default();
        let get = |v: Option<&dml::ST_Coordinate32>, d: i64| {
            v.and_then(crate::format::coord32_value).unwrap_or(Length::emu(d))
        };
        Some((
            get(pr.mar_l.as_ref(), 91_440),
            get(pr.mar_t.as_ref(), 45_720),
            get(pr.mar_r.as_ref(), 91_440),
            get(pr.mar_b.as_ref(), 45_720),
        ))
    }

    /// Sets the vertical placement of the text in a cell.
    pub fn set_cell_anchor(&mut self, row: usize, col: usize, anchor: TextAnchor) -> Result<&mut Self> {
        self.with_table(|t| {
            cell_pr(t, row, col)?.anchor = Some(anchor.to_dml());
            Ok::<(), Error>(())
        })?;
        Ok(self)
    }

    /// Vertical placement of the text in a cell, when set.
    pub fn cell_anchor(&self, row: usize, col: usize) -> Option<TextAnchor> {
        let t = self.table();
        t.tr.get(row)?
            .tc
            .get(col)?
            .tc_pr
            .as_ref()?
            .anchor
            .map(TextAnchor::from_dml)
    }

    /// Formats paragraph `paragraph` of a cell with a closure.
    pub fn with_cell_paragraph<R>(
        &mut self,
        row: usize,
        col: usize,
        paragraph: usize,
        f: impl FnOnce(ParagraphMut<'_>) -> R,
    ) -> Result<R> {
        self.with_table(|t| {
            let cell = cell_mut(t, row, col)?;
            let body = cell
                .tx_body
                .get_or_insert_with(|| Box::new(text::text_body(Vec::new())));
            let p = body
                .p
                .get_mut(paragraph)
                .ok_or_else(|| Error::NotFound(format!("paragraph {paragraph} of cell ({row}, {col})")))?;
            Ok(f(ParagraphMut::new(p)))
        })
    }

    /// Applies a table style.
    pub fn set_style(&mut self, style: TableStyle) -> &mut Self {
        let guid = style.guid().to_owned();
        self.with_table(|t| {
            t.tbl_pr.get_or_insert_with(Box::default).choice =
                Some(dml::CT_TableProperties_Choice::TableStyleId(guid));
        });
        self
    }

    /// The table style, when one is referenced by GUID.
    pub fn style(&self) -> Option<TableStyle> {
        match self.table().tbl_pr?.choice? {
            dml::CT_TableProperties_Choice::TableStyleId(g) => Some(TableStyle::from_guid(&g)),
            _ => None,
        }
    }

    /// Sets the table style options.
    pub fn set_flags(&mut self, flags: TableFlags) -> &mut Self {
        self.with_table(|t| {
            let pr = t.tbl_pr.get_or_insert_with(Box::default);
            let on = |b: bool| b.then_some(true);
            pr.first_row = on(flags.first_row);
            pr.last_row = on(flags.last_row);
            pr.first_col = on(flags.first_column);
            pr.last_col = on(flags.last_column);
            pr.band_row = on(flags.banded_rows);
            pr.band_col = on(flags.banded_columns);
        });
        self
    }

    /// The table style options.
    pub fn flags(&self) -> TableFlags {
        let pr = self.table().tbl_pr.unwrap_or_default();
        let get = |v: Option<bool>| v.unwrap_or(false);
        TableFlags {
            first_row: get(pr.first_row),
            last_row: get(pr.last_row),
            first_column: get(pr.first_col),
            last_column: get(pr.last_col),
            banded_rows: get(pr.band_row),
            banded_columns: get(pr.band_col),
        }
    }

    /// Sets the height of a row (the frame grows or shrinks accordingly).
    pub fn set_row_height(&mut self, row: usize, height: Length) -> Result<&mut Self> {
        self.with_table(|t| {
            t.tr.get_mut(row)
                .ok_or_else(|| Error::NotFound(format!("table row {row}")))?
                .h = Some(coord(height));
            Ok::<(), Error>(())
        })?;
        self.sync_frame_size();
        Ok(self)
    }

    /// Height of a row.
    pub fn row_height(&self, row: usize) -> Option<Length> {
        Some(coord_len(self.table().tr.get(row)?.h.as_ref()))
    }

    /// Width of a column.
    pub fn column_width(&self, col: usize) -> Option<Length> {
        Some(coord_len(self.table().tbl_grid?.grid_col.get(col)?.w.as_ref()))
    }

    /// Inserts an empty row at `at` (with the height of its neighbour).
    pub fn insert_row(&mut self, at: usize) -> Result<&mut Self> {
        self.with_table(|t| {
            if at > t.tr.len() {
                return Err(Error::NotFound(format!("table row {at}")));
            }
            let cols = t.tbl_grid.as_ref().map_or(0, |g| g.grid_col.len());
            let h = t.tr.get(at).or_else(|| t.tr.last()).and_then(|r| r.h.clone());
            t.tr.insert(
                at,
                dml::CT_TableRow {
                    h,
                    tc: (0..cols).map(|_| empty_cell()).collect(),
                    ..Default::default()
                },
            );
            Ok(())
        })?;
        self.sync_frame_size();
        Ok(self)
    }

    /// Removes a row (a table keeps at least one).
    pub fn remove_row(&mut self, row: usize) -> Result<&mut Self> {
        self.with_table(|t| {
            if row >= t.tr.len() || t.tr.len() == 1 {
                return Err(Error::InvalidArgument(format!("cannot remove table row {row}")));
            }
            t.tr.remove(row);
            Ok(())
        })?;
        self.sync_frame_size();
        Ok(self)
    }

    /// Inserts an empty column at `at` (with the width of its neighbour).
    pub fn insert_column(&mut self, at: usize) -> Result<&mut Self> {
        self.with_table(|t| {
            let grid = t.tbl_grid.get_or_insert_with(Box::default);
            if at > grid.grid_col.len() {
                return Err(Error::NotFound(format!("table column {at}")));
            }
            let w = grid
                .grid_col
                .get(at)
                .or_else(|| grid.grid_col.last())
                .and_then(|c| c.w.clone());
            grid.grid_col.insert(
                at,
                dml::CT_TableCol {
                    w,
                    ..Default::default()
                },
            );
            for r in &mut t.tr {
                let pos = at.min(r.tc.len());
                r.tc.insert(pos, empty_cell());
            }
            Ok(())
        })?;
        self.sync_frame_size();
        Ok(self)
    }

    /// Removes a column (a table keeps at least one).
    pub fn remove_column(&mut self, col: usize) -> Result<&mut Self> {
        self.with_table(|t| {
            let grid = t.tbl_grid.get_or_insert_with(Box::default);
            if col >= grid.grid_col.len() || grid.grid_col.len() == 1 {
                return Err(Error::InvalidArgument(format!(
                    "cannot remove table column {col}"
                )));
            }
            grid.grid_col.remove(col);
            for r in &mut t.tr {
                if col < r.tc.len() {
                    r.tc.remove(col);
                }
            }
            Ok(())
        })?;
        self.sync_frame_size();
        Ok(self)
    }

    /// Sets the alternative text of the table.
    pub fn set_alt_text(&mut self, title: Option<&str>, description: &str) -> &mut Self {
        let nv = self
            .frame
            .nv_graphic_frame_pr
            .get_or_insert_with(Box::default)
            .c_nv_pr
            .get_or_insert_with(Box::default);
        nv.title = title.map(str::to_owned);
        nv.descr = Some(description.to_owned());
        self
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
    fn styles_flags_and_structure() {
        for (style, guid) in TableStyle::KNOWN {
            assert_eq!(style.guid(), guid);
            assert_eq!(TableStyle::from_guid(&guid.to_lowercase()), style);
        }
        assert_eq!(TableStyle::from_guid("{X}"), TableStyle::Custom("{X}".into()));
        assert_eq!(TableStyle::Custom("{X}".into()).guid(), "{X}");
        let mut f = frame();
        let mut t = TableMut::new(&mut f);
        assert_eq!(t.style(), Some(TableStyle::MediumStyle2Accent1));
        t.set_style(TableStyle::DarkStyle1);
        assert_eq!(t.style(), Some(TableStyle::DarkStyle1));
        let flags = TableFlags {
            first_row: false,
            last_row: true,
            first_column: true,
            last_column: true,
            banded_rows: false,
            banded_columns: true,
        };
        t.set_flags(flags);
        assert_eq!(t.flags(), flags);
        t.insert_row(1).unwrap().insert_column(3).unwrap();
        assert_eq!((t.rows(), t.cols()), (3, 4));
        assert!(t.insert_row(9).is_err());
        assert!(t.insert_column(9).is_err());
        t.set_row_height(0, Length::cm(2.0)).unwrap();
        assert_eq!(t.row_height(0), Some(Length::cm(2.0)));
        assert_eq!(t.column_width(3), Some(Length::cm(3.0)));
        let ext = |f: &pml::CT_GraphicalObjectFrame| {
            let e = f.xfrm.as_ref().unwrap().ext.as_ref().unwrap();
            (e.cx.unwrap(), e.cy.unwrap())
        };
        let mut t = TableMut::new(&mut f);
        t.remove_row(1).unwrap().remove_column(0).unwrap();
        assert!(t.set_row_height(7, Length::cm(1.0)).is_err());
        assert_eq!(ext(&f), (Length::cm(9.0).as_emu(), Length::cm(3.0).as_emu()));
        let mut single = new_table_frame(
            1,
            1,
            1,
            Length::ZERO,
            Length::ZERO,
            Length::cm(1.0),
            Length::cm(1.0),
        )
        .unwrap();
        let mut t = TableMut::new(&mut single);
        assert!(t.remove_row(0).is_err());
        assert!(t.remove_column(0).is_err());
    }

    #[test]
    fn merging_and_cell_formatting() {
        let mut f = new_table_frame(
            1,
            3,
            3,
            Length::ZERO,
            Length::ZERO,
            Length::cm(9.0),
            Length::cm(3.0),
        )
        .unwrap();
        let mut t = TableMut::new(&mut f);
        t.merge_cells(0, 0, 1, 1).unwrap();
        assert_eq!(t.cell_span(0, 0), Some((2, 2, false)));
        assert_eq!(t.cell_span(0, 1), Some((1, 1, true)));
        assert_eq!(t.cell_span(1, 1), Some((1, 1, true)));
        assert!(
            t.merge_cells(1, 1, 2, 2).is_err(),
            "overlapping merges are refused"
        );
        assert!(t.merge_cells(2, 2, 1, 1).is_err());
        assert!(t.merge_cells(0, 0, 5, 5).is_err());
        t.split_cell(0, 0).unwrap();
        assert_eq!(t.cell_span(1, 1), Some((1, 1, false)));
        t.merge_cells(2, 0, 2, 2).unwrap();
        assert_eq!(t.cell_span(2, 0), Some((1, 3, false)));
        t.set_cell_fill(0, 0, Fill::solid(crate::text::Rgb(1, 2, 3)))
            .unwrap();
        assert!(matches!(t.cell_fill(0, 0), Some(Fill::Solid(_))));
        assert_eq!(t.cell_fill(0, 1), None);
        t.set_cell_borders(0, 0, Line::solid(crate::text::Rgb::BLACK, Length::pt(1.0)))
            .unwrap();
        t.set_cell_border(0, 0, CellBorder::DiagonalDown, Line::none())
            .unwrap();
        assert_eq!(
            t.cell_border(0, 0, CellBorder::Top).unwrap().width,
            Length::pt(1.0)
        );
        assert_eq!(t.cell_border(0, 0, CellBorder::DiagonalDown).unwrap().color, None);
        assert_eq!(t.cell_border(0, 0, CellBorder::DiagonalUp), None);
        assert_eq!(t.cell_margins(1, 1).unwrap().0, Length::inches(0.1));
        t.set_cell_margins(1, 1, Length::ZERO, Length::pt(2.0), Length::ZERO, Length::pt(2.0))
            .unwrap();
        assert_eq!(t.cell_margins(1, 1).unwrap().1, Length::pt(2.0));
        t.set_cell_anchor(1, 1, TextAnchor::Bottom).unwrap();
        assert_eq!(t.cell_anchor(1, 1), Some(TextAnchor::Bottom));
        assert!(t.set_cell_fill(9, 9, Fill::None).is_err());
        t.set_cell_text(1, 1, "x").unwrap();
        let bold = t
            .with_cell_paragraph(1, 1, 0, |mut p| {
                p.set_alignment(crate::text::Alignment::Center);
                p.run_mut(0).map(|mut r| r.bold(true).is_bold())
            })
            .unwrap();
        assert_eq!(bold, Some(true));
        assert!(t.with_cell_paragraph(1, 1, 3, |_| ()).is_err());
        t.set_alt_text(Some("Budget"), "Budget table");
        assert_eq!(
            f.nv_graphic_frame_pr
                .as_ref()
                .unwrap()
                .c_nv_pr
                .as_ref()
                .unwrap()
                .descr
                .as_deref(),
            Some("Budget table")
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
