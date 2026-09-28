//! Tables.

use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_schema::shared_types::ST_OnOff;
use openxml_schema::wml::{
    self, CT_TrPr_Choice, EG_BlockLevelElts, EG_ContentCellContent, EG_ContentRowContent,
    ST_MeasurementOrPercent, ST_Merge, ST_TblWidth, ST_VerticalJc,
};

use crate::document::Shared;
use crate::format::{
    CellMargins, HeightRule, TableAlignment, TableBorders, TableFormat, TableLayout, TableWidth,
    apply_table_format, cell_margins, dxa as format_dxa,
};
use crate::paragraph::{Paragraph, ParagraphMut};
use crate::text;
use crate::util::{decimal, hex_color, on, string_val, twips};

/// Vertical alignment of cell content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAlign {
    /// Top.
    Top,
    /// Centered.
    Center,
    /// Bottom.
    Bottom,
}

fn dxa(len: Length) -> Box<wml::CT_TblWidth> {
    Box::new(wml::CT_TblWidth {
        w: Some(ST_MeasurementOrPercent::DecimalNumberOrPercent(
            wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(len.as_twips().max(0)),
        )),
        type_: Some(ST_TblWidth::Dxa),
        ..Default::default()
    })
}

fn empty_cell(width: Length) -> wml::CT_Tc {
    wml::CT_Tc {
        tc_pr: Some(Box::new(wml::CT_TcPr {
            tc_w: Some(dxa(width)),
            ..Default::default()
        })),
        block_level_elts: vec![EG_BlockLevelElts::P(Box::default())],
        ..Default::default()
    }
}

fn new_row(widths: &[Length]) -> wml::CT_Row {
    wml::CT_Row {
        content_cell_content: widths
            .iter()
            .map(|w| EG_ContentCellContent::Tc(Box::new(empty_cell(*w))))
            .collect(),
        ..Default::default()
    }
}

/// Builds a table with equal columns filling `total_width`.
pub(crate) fn new_table(rows: usize, cols: usize, style_id: &str, total_width: Length) -> wml::CT_Tbl {
    let col_width = total_width / cols as i64;
    let widths = vec![col_width; cols];
    let look = wml::CT_TblLook {
        first_row: Some(ST_OnOff::Boolean(true)),
        last_row: Some(ST_OnOff::Boolean(false)),
        first_column: Some(ST_OnOff::Boolean(true)),
        last_column: Some(ST_OnOff::Boolean(false)),
        no_h_band: Some(ST_OnOff::Boolean(false)),
        no_v_band: Some(ST_OnOff::Boolean(true)),
        val: Some(openxml_xml::HexBinary(vec![0x04, 0xA0])),
        ..Default::default()
    };
    wml::CT_Tbl {
        tbl_pr: Some(Box::new(wml::CT_TblPr {
            tbl_style: Some(string_val(style_id)),
            tbl_w: Some(Box::new(wml::CT_TblWidth {
                w: Some(ST_MeasurementOrPercent::DecimalNumberOrPercent(
                    wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(0),
                )),
                type_: Some(ST_TblWidth::Auto),
                ..Default::default()
            })),
            tbl_look: Some(Box::new(look)),
            ..Default::default()
        })),
        tbl_grid: Some(Box::new(wml::CT_TblGrid {
            grid_col: widths
                .iter()
                .map(|w| wml::CT_TblGridCol {
                    w: Some(twips(*w)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })),
        content_row_content: (0..rows)
            .map(|_| EG_ContentRowContent::Tr(Box::new(new_row(&widths))))
            .collect(),
        ..Default::default()
    }
}

/// Read-only view of a table.
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    pub(crate) t: &'a wml::CT_Tbl,
}

impl<'a> Table<'a> {
    pub(crate) fn new(t: &'a wml::CT_Tbl) -> Self {
        Table { t }
    }

    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_Tbl {
        self.t
    }

    /// Rows.
    pub fn rows(&self) -> Vec<TableRow<'a>> {
        text::rows(self.t).into_iter().map(|r| TableRow { r }).collect()
    }

    /// Number of rows.
    pub fn row_count(&self) -> usize {
        text::rows(self.t).len()
    }

    /// Number of grid columns (`w:tblGrid`), or the largest number of cells in a row.
    pub fn column_count(&self) -> usize {
        let grid = self.t.tbl_grid.as_ref().map_or(0, |g| g.grid_col.len());
        if grid > 0 {
            grid
        } else {
            self.rows().iter().map(|r| r.cells().len()).max().unwrap_or(0)
        }
    }

    /// The cell at `row`, `col` (cell index within the row, not grid column).
    pub fn cell(&self, row: usize, col: usize) -> Option<TableCell<'a>> {
        self.rows().get(row)?.cells().get(col).copied()
    }

    /// Table style id.
    pub fn style_id(&self) -> Option<&'a str> {
        self.t.tbl_pr.as_deref()?.tbl_style.as_deref()?.val.as_deref()
    }

    /// Plain text: cells separated by tabs, rows by newlines.
    pub fn text(&self) -> String {
        text::table_text(self.t)
    }
}

/// Read-only view of a table row.
#[derive(Clone, Copy, Debug)]
pub struct TableRow<'a> {
    pub(crate) r: &'a wml::CT_Row,
}

impl<'a> TableRow<'a> {
    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_Row {
        self.r
    }

    /// Cells of the row.
    pub fn cells(&self) -> Vec<TableCell<'a>> {
        text::cells(self.r).into_iter().map(|c| TableCell { c }).collect()
    }

    /// Whether the row repeats as a header row on each page.
    pub fn is_header(&self) -> bool {
        self.r.tr_pr.as_deref().is_some_and(|p| {
            p.choice
                .iter()
                .any(|c| matches!(c, CT_TrPr_Choice::TblHeader(v) if crate::util::on_off_value(v)))
        })
    }
}

/// Read-only view of a table cell.
#[derive(Clone, Copy, Debug)]
pub struct TableCell<'a> {
    pub(crate) c: &'a wml::CT_Tc,
}

impl<'a> TableCell<'a> {
    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_Tc {
        self.c
    }

    /// Paragraphs of the cell (outside nested tables).
    pub fn paragraphs(&self) -> Vec<Paragraph<'a>> {
        text::blocks(&self.c.block_level_elts)
            .into_iter()
            .filter_map(|b| {
                if let text::BlockRef::P(p) = b {
                    Some(Paragraph::new(p))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Nested tables.
    pub fn tables(&self) -> Vec<Table<'a>> {
        text::blocks(&self.c.block_level_elts)
            .into_iter()
            .filter_map(|b| {
                if let text::BlockRef::Tbl(t) = b {
                    Some(Table::new(t))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Plain text (paragraphs separated by newlines).
    pub fn text(&self) -> String {
        text::cell_text(self.c)
    }

    /// Number of grid columns the cell spans.
    pub fn grid_span(&self) -> usize {
        self.c
            .tc_pr
            .as_deref()
            .and_then(|p| p.grid_span.as_deref())
            .and_then(|g| g.val)
            .map_or(1, |v| v.max(1) as usize)
    }

    /// Vertical merge state: `Some(true)` starts a merged region, `Some(false)` continues one.
    pub fn vertical_merge(&self) -> Option<bool> {
        let v = self.c.tc_pr.as_deref()?.v_merge.as_deref()?;
        Some(v.val == Some(ST_Merge::Restart))
    }
}

/// Mutable access to a table.
///
/// ```
/// use openxml_docx::Document;
///
/// let mut doc = Document::new();
/// let mut table = doc.add_table(2, 2)?;
/// table.cell(0, 0)?.set_text("Name");
/// table.cell(0, 1)?.set_text("Value");
/// table.set_header_row(0, true)?;
/// assert_eq!(doc.tables()[0].text(), "Name\tValue\n\t");
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Debug)]
pub struct TableMut<'a> {
    pub(crate) t: &'a mut wml::CT_Tbl,
    pub(crate) shared: &'a mut Shared,
    pub(crate) part: PartName,
}

impl<'a> TableMut<'a> {
    pub(crate) fn new(t: &'a mut wml::CT_Tbl, shared: &'a mut Shared, part: PartName) -> Self {
        TableMut { t, shared, part }
    }

    /// The underlying schema object.
    pub fn raw(&mut self) -> &mut wml::CT_Tbl {
        self.t
    }

    /// Read-only view.
    pub fn view(&self) -> Table<'_> {
        Table::new(self.t)
    }

    /// Number of rows.
    pub fn row_count(&self) -> usize {
        self.view().row_count()
    }

    /// Number of grid columns.
    pub fn column_count(&self) -> usize {
        self.view().column_count()
    }

    /// Mutable access to the cell at `row`, `col` (cell index within the row).
    pub fn cell(&mut self, row: usize, col: usize) -> Result<CellMut<'_>> {
        let rows = text::rows_mut(self.t);
        let row_count = rows.len();
        let r = rows
            .into_iter()
            .nth(row)
            .ok_or_else(|| Error::NotFound(format!("row {row} (the table has {row_count})")))?;
        let cells = text::cells_mut(r);
        let cell_count = cells.len();
        let c = cells
            .into_iter()
            .nth(col)
            .ok_or_else(|| Error::NotFound(format!("cell {col} of row {row} (the row has {cell_count})")))?;
        Ok(CellMut {
            c,
            shared: self.shared,
            part: self.part.clone(),
        })
    }

    /// Applies a table style (built-in `TableGrid` is added when missing).
    pub fn set_style(&mut self, style_id: &str) -> Result<&mut Self> {
        let id = self.shared.resolve_style(style_id)?;
        self.t.tbl_pr.get_or_insert_with(Default::default).tbl_style = Some(string_val(&id));
        Ok(self)
    }

    /// Sets the widths of the grid columns and of the cells in each column.
    pub fn set_column_widths(&mut self, widths: &[Length]) -> Result<&mut Self> {
        let grid = self.t.tbl_grid.get_or_insert_with(Default::default);
        if widths.len() != grid.grid_col.len() {
            return Err(Error::InvalidArgument(format!(
                "{} widths given for a table with {} columns",
                widths.len(),
                grid.grid_col.len()
            )));
        }
        for (col, w) in grid.grid_col.iter_mut().zip(widths) {
            col.w = Some(twips(*w));
        }
        for row in text::rows_mut(self.t) {
            let mut grid_col = 0;
            for cell in text::cells_mut(row) {
                let span = cell
                    .tc_pr
                    .as_deref()
                    .and_then(|p| p.grid_span.as_deref())
                    .and_then(|g| g.val)
                    .unwrap_or(1)
                    .max(1) as usize;
                let width = widths
                    .iter()
                    .skip(grid_col)
                    .take(span)
                    .fold(Length::ZERO, |a, b| a + *b);
                cell.tc_pr.get_or_insert_with(Default::default).tc_w = Some(dxa(width));
                grid_col += span;
            }
        }
        Ok(self)
    }

    /// Appends a row with one empty cell per grid column; returns its index.
    pub fn add_row(&mut self) -> usize {
        let widths: Vec<Length> = self
            .t
            .tbl_grid
            .as_ref()
            .map(|g| {
                g.grid_col
                    .iter()
                    .map(|c| {
                        c.w.as_ref()
                            .and_then(crate::util::twips_value)
                            .unwrap_or(Length::ZERO)
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.t
            .content_row_content
            .push(EG_ContentRowContent::Tr(Box::new(new_row(&widths))));
        self.row_count() - 1
    }

    /// Marks a row as a header row repeated at the top of each page.
    pub fn set_header_row(&mut self, row: usize, value: bool) -> Result<&mut Self> {
        let mut rows = text::rows_mut(self.t);
        let count = rows.len();
        let r = rows
            .get_mut(row)
            .ok_or_else(|| Error::NotFound(format!("row {row} (the table has {count})")))?;
        let tr_pr = r.tr_pr.get_or_insert_with(Default::default);
        tr_pr
            .choice
            .retain(|c| !matches!(c, CT_TrPr_Choice::TblHeader(_)));
        if value {
            tr_pr.choice.push(CT_TrPr_Choice::TblHeader(on()));
        }
        Ok(self)
    }

    /// Merges cells `first..=last` of a row into one cell spanning their grid
    /// columns. The content of the merged cells is appended to the first one.
    pub fn merge_horizontally(&mut self, row: usize, first: usize, last: usize) -> Result<&mut Self> {
        if first >= last {
            return Err(Error::InvalidArgument(format!(
                "cannot merge cells {first}..={last}"
            )));
        }
        let mut rows = text::rows_mut(self.t);
        let count = rows.len();
        let r = rows
            .get_mut(row)
            .ok_or_else(|| Error::NotFound(format!("row {row} (the table has {count})")))?;
        // Only rows made of plain cells can be merged this way.
        let positions: Vec<usize> = r
            .content_cell_content
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, EG_ContentCellContent::Tc(_)))
            .map(|(i, _)| i)
            .collect();
        if last >= positions.len() {
            return Err(Error::NotFound(format!(
                "cell {last} of row {row} (the row has {})",
                positions.len()
            )));
        }
        let mut span = 0i64;
        let mut moved = Vec::new();
        for &pos in positions[first + 1..=last].iter().rev() {
            if let EG_ContentCellContent::Tc(c) = r.content_cell_content.remove(pos) {
                span += c
                    .tc_pr
                    .as_deref()
                    .and_then(|p| p.grid_span.as_deref())
                    .and_then(|g| g.val)
                    .unwrap_or(1)
                    .max(1);
                moved.push(c);
            }
        }
        moved.reverse();
        let EG_ContentCellContent::Tc(target) = &mut r.content_cell_content[positions[first]] else {
            unreachable!()
        };
        let own = target
            .tc_pr
            .as_deref()
            .and_then(|p| p.grid_span.as_deref())
            .and_then(|g| g.val)
            .unwrap_or(1)
            .max(1);
        let tc_pr = target.tc_pr.get_or_insert_with(Default::default);
        tc_pr.grid_span = Some(decimal(own + span));
        tc_pr.tc_w = None;
        for c in moved {
            for block in c.block_level_elts {
                let empty = matches!(&block, EG_BlockLevelElts::P(p) if text::paragraph_text(p).is_empty());
                if !empty {
                    target.block_level_elts.push(block);
                }
            }
        }
        Ok(self)
    }

    /// Merges the cells of column `col` (cell index) from `first` to `last`
    /// row vertically. The content of the continued cells is moved to the first.
    pub fn merge_vertically(&mut self, col: usize, first: usize, last: usize) -> Result<&mut Self> {
        if first >= last {
            return Err(Error::InvalidArgument(format!(
                "cannot merge rows {first}..={last}"
            )));
        }
        let mut rows = text::rows_mut(self.t);
        if last >= rows.len() {
            return Err(Error::NotFound(format!(
                "row {last} (the table has {})",
                rows.len()
            )));
        }
        let mut moved = Vec::new();
        for (i, r) in rows.iter_mut().enumerate().take(last + 1).skip(first) {
            let mut cells = text::cells_mut(r);
            let count = cells.len();
            let c = cells
                .get_mut(col)
                .ok_or_else(|| Error::NotFound(format!("cell {col} of row {i} (the row has {count})")))?;
            let restart = i == first;
            c.tc_pr.get_or_insert_with(Default::default).v_merge = Some(Box::new(wml::CT_VMerge {
                val: restart.then_some(ST_Merge::Restart).or(Some(ST_Merge::Continue)),
                ..Default::default()
            }));
            if !restart {
                let blocks = std::mem::replace(
                    &mut c.block_level_elts,
                    vec![EG_BlockLevelElts::P(Box::default())],
                );
                moved.extend(
                    blocks.into_iter().filter(
                        |b| !matches!(b, EG_BlockLevelElts::P(p) if text::paragraph_text(p).is_empty()),
                    ),
                );
            }
        }
        let mut cells = text::cells_mut(&mut *rows[first]);
        cells[col].block_level_elts.extend(moved);
        Ok(self)
    }
}

/// Mutable access to a table cell.
#[derive(Debug)]
pub struct CellMut<'a> {
    pub(crate) c: &'a mut wml::CT_Tc,
    pub(crate) shared: &'a mut Shared,
    pub(crate) part: PartName,
}

impl<'a> CellMut<'a> {
    /// The underlying schema object.
    pub fn raw(&mut self) -> &mut wml::CT_Tc {
        self.c
    }

    /// Read-only view.
    pub fn view(&self) -> TableCell<'_> {
        TableCell { c: self.c }
    }

    /// Replaces the cell content with one paragraph containing `text`.
    pub fn set_text(&mut self, text: &str) -> ParagraphMut<'_> {
        self.c.block_level_elts = vec![EG_BlockLevelElts::P(Box::default())];
        let EG_BlockLevelElts::P(p) = &mut self.c.block_level_elts[0] else {
            unreachable!()
        };
        let mut handle = ParagraphMut::new(p, self.shared, self.part.clone());
        if !text.is_empty() {
            handle.add_text(text);
        }
        handle
    }

    /// Appends a paragraph. A cell created empty contains one empty paragraph,
    /// which is reused by the first call.
    pub fn add_paragraph(&mut self, text: &str) -> ParagraphMut<'_> {
        let reuse =
            matches!(self.c.block_level_elts.as_slice(), [EG_BlockLevelElts::P(p)] if p.p_content.is_empty());
        if !reuse {
            self.c.block_level_elts.push(EG_BlockLevelElts::P(Box::default()));
        }
        let Some(EG_BlockLevelElts::P(p)) = self.c.block_level_elts.last_mut() else {
            unreachable!()
        };
        let mut handle = ParagraphMut::new(p, self.shared, self.part.clone());
        if !text.is_empty() {
            handle.add_text(text);
        }
        handle
    }

    /// Sets the cell background color (`RRGGBB`).
    pub fn set_shading(&mut self, hex: &str) -> Result<&mut Self> {
        let fill = hex_color(hex)?;
        self.c.tc_pr.get_or_insert_with(Default::default).shd = Some(Box::new(wml::CT_Shd {
            val: Some(wml::ST_Shd::Clear),
            color: Some(hex_color("auto")?),
            fill: Some(fill),
            ..Default::default()
        }));
        Ok(self)
    }

    /// Sets the preferred cell width.
    pub fn set_width(&mut self, width: Length) -> &mut Self {
        self.c.tc_pr.get_or_insert_with(Default::default).tc_w = Some(dxa(width));
        self
    }

    /// Sets the vertical alignment of the content.
    pub fn set_vertical_alignment(&mut self, align: CellAlign) -> &mut Self {
        let val = match align {
            CellAlign::Top => ST_VerticalJc::Top,
            CellAlign::Center => ST_VerticalJc::Center,
            CellAlign::Bottom => ST_VerticalJc::Bottom,
        };
        self.c.tc_pr.get_or_insert_with(Default::default).v_align = Some(Box::new(wml::CT_VerticalJc {
            val: Some(val),
            ..Default::default()
        }));
        self
    }
}

impl TableMut<'_> {
    fn tbl_pr(&mut self) -> &mut wml::CT_TblPr {
        self.t.tbl_pr.get_or_insert_with(Default::default)
    }

    fn row_pr(&mut self, row: usize) -> Result<&mut wml::CT_TrPr> {
        let mut rows = text::rows_mut(self.t);
        let count = rows.len();
        if row >= count {
            return Err(Error::NotFound(format!("row {row} (the table has {count})")));
        }
        Ok(rows.swap_remove(row).tr_pr.get_or_insert_with(Default::default))
    }

    /// Applies table formatting (fields left at `None` are unchanged).
    pub fn set_format(&mut self, format: &TableFormat) -> Result<&mut Self> {
        apply_table_format(self.tbl_pr(), format)?;
        Ok(self)
    }

    /// Sets the table borders.
    pub fn set_borders(&mut self, borders: &TableBorders) -> Result<&mut Self> {
        self.tbl_pr().tbl_borders = Some(Box::new(borders.to_table()?));
        Ok(self)
    }

    /// Sets the default cell margins.
    pub fn set_cell_margins(&mut self, margins: &CellMargins) -> &mut Self {
        self.tbl_pr().tbl_cell_mar = Some(Box::new(cell_margins(margins)));
        self
    }

    /// Sets the alignment of the table between the margins.
    pub fn set_alignment(&mut self, alignment: TableAlignment) -> &mut Self {
        let f = TableFormat {
            alignment: Some(alignment),
            ..Default::default()
        };
        apply_table_format(self.tbl_pr(), &f).expect("alignment cannot fail");
        self
    }

    /// Sets the indentation from the leading margin.
    pub fn set_indent(&mut self, indent: Length) -> &mut Self {
        self.tbl_pr().tbl_ind = Some(format_dxa(indent));
        self
    }

    /// Sets the layout: fixed column widths or autofit to the content.
    pub fn set_layout(&mut self, layout: TableLayout) -> &mut Self {
        let f = TableFormat {
            layout: Some(layout),
            ..Default::default()
        };
        apply_table_format(self.tbl_pr(), &f).expect("layout cannot fail");
        self
    }

    /// Sets the preferred table width.
    pub fn set_width(&mut self, width: TableWidth) -> &mut Self {
        let f = TableFormat {
            width: Some(width),
            ..Default::default()
        };
        apply_table_format(self.tbl_pr(), &f).expect("width cannot fail");
        self
    }

    /// Sets the height of a row.
    pub fn set_row_height(&mut self, row: usize, height: Length, rule: HeightRule) -> Result<&mut Self> {
        let pr = self.row_pr(row)?;
        pr.choice.retain(|c| !matches!(c, CT_TrPr_Choice::TrHeight(_)));
        pr.choice.push(CT_TrPr_Choice::TrHeight(Box::new(wml::CT_Height {
            val: Some(twips(height)),
            h_rule: Some(match rule {
                HeightRule::AtLeast => wml::ST_HeightRule::AtLeast,
                HeightRule::Exact => wml::ST_HeightRule::Exact,
            }),
            ..Default::default()
        })));
        Ok(self)
    }

    /// Prevents a row from breaking across pages.
    pub fn set_cant_split(&mut self, row: usize, value: bool) -> Result<&mut Self> {
        let pr = self.row_pr(row)?;
        pr.choice.retain(|c| !matches!(c, CT_TrPr_Choice::CantSplit(_)));
        if value {
            pr.choice.push(CT_TrPr_Choice::CantSplit(on()));
        }
        Ok(self)
    }

    /// Marks the first `count` rows as header rows repeated on each page.
    pub fn set_header_rows(&mut self, count: usize) -> Result<&mut Self> {
        let rows = self.row_count();
        for row in 0..rows {
            self.set_header_row(row, row < count)?;
        }
        Ok(self)
    }
}

impl CellMut<'_> {
    /// Sets the borders of the cell (the inside borders are ignored).
    pub fn set_borders(&mut self, borders: &TableBorders) -> Result<&mut Self> {
        let b = TableBorders {
            inside_horizontal: None,
            inside_vertical: None,
            ..borders.clone()
        };
        self.c.tc_pr.get_or_insert_with(Default::default).tc_borders = Some(Box::new(b.to_cell()?));
        Ok(self)
    }

    /// Sets the margins of the cell.
    pub fn set_margins(&mut self, margins: &CellMargins) -> &mut Self {
        let m = cell_margins(margins);
        self.c.tc_pr.get_or_insert_with(Default::default).tc_mar = Some(Box::new(wml::CT_TcMar {
            top: m.top,
            left: m.left,
            bottom: m.bottom,
            right: m.right,
            ..Default::default()
        }));
        self
    }

    /// Appends a nested table with `rows` × `cols` empty cells filling the
    /// cell width. The cell keeps a paragraph after the table, as required.
    pub fn add_table(&mut self, rows: usize, cols: usize) -> Result<TableMut<'_>> {
        if rows == 0 || cols == 0 {
            return Err(Error::InvalidArgument(
                "a table needs at least one row and one column".into(),
            ));
        }
        let style = self.shared.resolve_style("TableGrid")?;
        let width = self
            .c
            .tc_pr
            .as_deref()
            .and_then(|p| p.tc_w.as_deref())
            .and_then(|w| match (&w.w, w.type_) {
                (
                    Some(ST_MeasurementOrPercent::DecimalNumberOrPercent(
                        wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(v),
                    )),
                    Some(ST_TblWidth::Dxa),
                ) => Some(Length::twips(*v)),
                _ => None,
            })
            .map(|w| w - Length::twips(216))
            .filter(|w| w.as_emu() > 0)
            .unwrap_or(Length::inches(2.0));
        let table = new_table(rows, cols, &style, width);
        // A cell must end with a paragraph. A fresh cell's empty paragraph
        // follows the table; otherwise the table is appended with a new one.
        let fresh =
            matches!(self.c.block_level_elts.as_slice(), [EG_BlockLevelElts::P(p)] if p.p_content.is_empty());
        let at = if fresh {
            0
        } else {
            self.c.block_level_elts.push(EG_BlockLevelElts::P(Box::default()));
            self.c.block_level_elts.len() - 1
        };
        self.c
            .block_level_elts
            .insert(at, EG_BlockLevelElts::Tbl(Box::new(table)));
        let EG_BlockLevelElts::Tbl(t) = &mut self.c.block_level_elts[at] else {
            unreachable!("a table was just inserted")
        };
        Ok(TableMut::new(t, self.shared, self.part.clone()))
    }
}

impl<'a> Table<'a> {
    fn props(&self) -> Option<&'a wml::CT_TblPr> {
        self.t.tbl_pr.as_deref()
    }

    /// Borders applied directly to the table.
    pub fn borders(&self) -> Option<TableBorders> {
        Some(TableBorders::from_table(self.props()?.tbl_borders.as_deref()?))
    }

    /// Alignment between the margins.
    pub fn alignment(&self) -> Option<TableAlignment> {
        Some(match self.props()?.jc.as_deref()?.val? {
            wml::ST_JcTable::Center => TableAlignment::Center,
            wml::ST_JcTable::Right | wml::ST_JcTable::End => TableAlignment::Right,
            wml::ST_JcTable::Left | wml::ST_JcTable::Start => TableAlignment::Left,
        })
    }

    /// Layout algorithm.
    pub fn layout(&self) -> Option<TableLayout> {
        Some(match self.props()?.tbl_layout.as_deref()?.type_? {
            wml::ST_TblLayoutType::Fixed => TableLayout::Fixed,
            wml::ST_TblLayoutType::Autofit => TableLayout::Autofit,
        })
    }
}

impl TableRow<'_> {
    /// Height of the row, when specified.
    pub fn height(&self) -> Option<(Length, HeightRule)> {
        self.r.tr_pr.as_deref()?.choice.iter().find_map(|c| match c {
            CT_TrPr_Choice::TrHeight(h) => Some((
                h.val.as_ref().and_then(crate::util::twips_value)?,
                match h.h_rule {
                    Some(wml::ST_HeightRule::Exact) => HeightRule::Exact,
                    _ => HeightRule::AtLeast,
                },
            )),
            _ => None,
        })
    }

    /// Whether the row may not break across pages.
    pub fn cant_split(&self) -> bool {
        self.r.tr_pr.as_deref().is_some_and(|p| {
            p.choice
                .iter()
                .any(|c| matches!(c, CT_TrPr_Choice::CantSplit(v) if crate::util::on_off_value(v)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_has_grid_rows_and_paragraphs_in_every_cell() {
        let t = new_table(3, 4, "TableGrid", Length::inches(6.0));
        let view = Table::new(&t);
        assert_eq!(view.row_count(), 3);
        assert_eq!(view.column_count(), 4);
        assert_eq!(view.style_id(), Some("TableGrid"));
        for row in view.rows() {
            assert_eq!(row.cells().len(), 4);
            for cell in row.cells() {
                assert_eq!(cell.paragraphs().len(), 1);
                assert_eq!(cell.grid_span(), 1);
                assert_eq!(cell.vertical_merge(), None);
                assert!(cell.tables().is_empty());
            }
            assert!(!row.is_header());
        }
        let col = &t.tbl_grid.as_ref().unwrap().grid_col[0];
        assert_eq!(col.w, Some(twips(Length::inches(1.5))));
        assert!(view.cell(3, 0).is_none());
        assert!(view.cell(0, 4).is_none());
    }
}
