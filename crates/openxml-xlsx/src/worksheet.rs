//! Worksheets: reading and editing cells, rows, columns, merges and panes.

use std::cell::OnceCell;
use std::collections::HashMap;

use openxml_core::part::read_part;
use openxml_core::{Error, Result};
use openxml_opc::{Package, PartName};
use openxml_schema::sml;
use openxml_xml::{Ns, XmlList};

use crate::cell_ref::{CellRange, CellRef, MAX_COL, MAX_ROW, ToCellRef, column_index};
use crate::date::{DateSystem, DateTime};
use crate::formula::shift_formula;
use crate::shared_strings::{SharedStrings, rst_text};
use crate::styles::{NumberFormat, StyleId, Styles};
use crate::value::{CellValue, MAX_TEXT_LEN, decode_xstring, encode_xstring, format_number};

/// What a `<sheet>` of the workbook points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    /// A worksheet (cells).
    Worksheet,
    /// A chart sheet.
    Chartsheet,
    /// A dialog sheet.
    Dialogsheet,
    /// A macro sheet or an unrecognised part.
    Other,
}

/// A sheet of the workbook and, for worksheets, its lazily parsed content.
#[derive(Debug)]
pub(crate) struct SheetEntry {
    pub name: String,
    pub rel_id: String,
    pub part: Option<PartName>,
    pub kind: SheetKind,
    pub data: OnceCell<sml::CT_Worksheet>,
    pub dirty: bool,
}

impl SheetEntry {
    /// Parses the worksheet part on first use.
    pub fn load(&self, pkg: &Package) -> Result<&sml::CT_Worksheet> {
        if let Some(d) = self.data.get() {
            return Ok(d);
        }
        let part = self
            .part
            .as_ref()
            .ok_or_else(|| Error::MissingPart(format!("sheet {}", self.name)))?;
        let mut ws = read_part(pkg, part, &sml::elements::WORKSHEET)?;
        normalize(&mut ws);
        let _ = self.data.set(ws);
        Ok(self.data.get().expect("just set"))
    }
}

/// Assigns explicit references to rows and cells that omit them and sorts
/// rows and cells into sheet order.
pub(crate) fn normalize(ws: &mut sml::CT_Worksheet) {
    let Some(data) = ws.sheet_data.as_mut() else {
        return;
    };
    let mut next_row = 1u32;
    for row in &mut data.row {
        next_row = normalize_row(row, next_row);
    }
    if !data.row.windows(2).all(|w| w[0].r <= w[1].r) {
        data.row.sort_by_key(|r| r.r);
    }
}

/// Gives a row (and its cells) explicit references; `next_row` is the number
/// the row gets if it has none. Returns the number of the following row.
pub(crate) fn normalize_row(row: &mut sml::CT_Row, next_row: u32) -> u32 {
    let r = *row.r.get_or_insert(next_row);
    let mut next_col = 1u32;
    for c in &mut row.c {
        let col = match c.r.as_deref().and_then(|s| CellRef::parse(s).ok()) {
            Some(cr) => cr.col(),
            None => {
                let col = next_col.min(MAX_COL);
                if let Ok(cr) = CellRef::new(r.clamp(1, MAX_ROW), col) {
                    c.r = Some(cr.to_string());
                }
                col
            }
        };
        next_col = col.saturating_add(1);
    }
    if !row.c.windows(2).all(|w| cell_col(&w[0]) <= cell_col(&w[1])) {
        row.c.sort_by_key(cell_col);
    }
    r.saturating_add(1)
}

fn cell_col(c: &sml::CT_Cell) -> u32 {
    c.r.as_deref()
        .map(|s| {
            let letters: String = s.chars().filter(char::is_ascii_alphabetic).collect();
            column_index(&letters).unwrap_or(0)
        })
        .unwrap_or(0)
}

fn find_row(rows: &[sml::CT_Row], r: u32) -> std::result::Result<usize, usize> {
    rows.binary_search_by_key(&r, |row| row.r.unwrap_or(0))
}

fn find_cell(cells: &[sml::CT_Cell], col: u32) -> std::result::Result<usize, usize> {
    cells.binary_search_by_key(&col, cell_col)
}

fn raw_cell_type(c: &sml::CT_Cell) -> Option<&str> {
    c.extra_attrs
        .iter()
        .find(|a| a.name.is(Ns::NONE, "t"))
        .map(|a| a.value.as_str())
}

/// Shared context for reading values.
#[derive(Clone, Copy)]
pub(crate) struct ReadCtx<'a> {
    pub sst: &'a SharedStrings,
    pub styles: &'a Styles,
    pub date_system: DateSystem,
}

fn plain_value(cell: &sml::CT_Cell, ctx: ReadCtx<'_>) -> CellValue {
    let v = cell.v.as_deref();
    match (cell.t, raw_cell_type(cell)) {
        (Some(sml::ST_CellType::S), _) => v
            .and_then(|v| v.trim().parse::<u32>().ok())
            .and_then(|i| ctx.sst.get(i))
            .map_or(CellValue::Empty, |s| CellValue::Text(s.to_owned())),
        (Some(sml::ST_CellType::InlineStr), _) => cell
            .is
            .as_deref()
            .map_or(CellValue::Text(String::new()), |is| CellValue::Text(rst_text(is))),
        (Some(sml::ST_CellType::Str), _) => v.map_or(CellValue::Text(String::new()), |s| {
            CellValue::Text(decode_xstring(s).into_owned())
        }),
        (Some(sml::ST_CellType::B), _) => match v.map(str::trim) {
            Some("1" | "true") => CellValue::Bool(true),
            Some("0" | "false") => CellValue::Bool(false),
            _ => CellValue::Empty,
        },
        (Some(sml::ST_CellType::E), _) => v.map_or(CellValue::Empty, |e| CellValue::Error(e.to_owned())),
        (None, Some("d")) => match v.and_then(DateTime::parse_iso) {
            Some(d) => CellValue::DateTime(d),
            None => v.map_or(CellValue::Empty, |s| CellValue::Text(s.to_owned())),
        },
        _ => match v.and_then(|v| v.trim().parse::<f64>().ok()) {
            Some(n) => {
                let date_styled = cell.s.is_some_and(|s| ctx.styles.is_date_style(s));
                match date_styled
                    .then(|| DateTime::from_serial(n, ctx.date_system))
                    .flatten()
                {
                    Some(d) => CellValue::DateTime(d),
                    None => CellValue::Number(n),
                }
            }
            None => CellValue::Empty,
        },
    }
}

/// Masters of shared formulas: shared index → (cell, formula text).
pub(crate) type SharedFormulas = HashMap<u32, (CellRef, String)>;

/// Records the shared-formula masters of a row.
pub(crate) fn collect_shared(row: &sml::CT_Row, map: &mut SharedFormulas) {
    for c in &row.c {
        let Some(f) = c.f.as_deref() else { continue };
        if f.t == Some(sml::ST_CellFormulaType::Shared)
            && !f.value.is_empty()
            && let (Some(si), Some(r)) = (f.si, c.r.as_deref().and_then(|r| CellRef::parse(r).ok()))
        {
            map.entry(si).or_insert((r, f.value.clone()));
        }
    }
}

fn shared_formulas(ws: &sml::CT_Worksheet) -> SharedFormulas {
    let mut map = HashMap::new();
    for row in ws.sheet_data.iter().flat_map(|d| d.row.iter()) {
        collect_shared(row, &mut map);
    }
    map
}

/// Non-empty values of a row.
pub(crate) fn row_values(
    row: &sml::CT_Row,
    ctx: ReadCtx<'_>,
    shared: &SharedFormulas,
) -> Vec<(CellRef, CellValue)> {
    row.c
        .iter()
        .filter_map(|c| {
            let r = CellRef::parse(c.r.as_deref()?).ok()?;
            let v = cell_value(c, ctx, &|| shared);
            (!v.is_empty()).then_some((r, v))
        })
        .collect()
}

fn cell_value<'m>(
    cell: &sml::CT_Cell,
    ctx: ReadCtx<'_>,
    shared: &dyn Fn() -> &'m SharedFormulas,
) -> CellValue {
    let plain = plain_value(cell, ctx);
    let Some(f) = cell.f.as_deref() else { return plain };
    let mut text = f.value.clone();
    if text.is_empty()
        && f.t == Some(sml::ST_CellFormulaType::Shared)
        && let (Some(si), Some(here)) = (f.si, cell.r.as_deref().and_then(|r| CellRef::parse(r).ok()))
        && let Some((master, formula)) = shared().get(&si)
    {
        text = shift_formula(
            formula,
            i64::from(here.row()) - i64::from(master.row()),
            i64::from(here.col()) - i64::from(master.col()),
        );
    }
    let cached = (!plain.is_empty()).then(|| Box::new(plain));
    CellValue::Formula {
        formula: decode_xstring(&text).into_owned(),
        cached,
    }
}

/// A read-only view of a worksheet.
pub struct Worksheet<'a> {
    name: &'a str,
    part: &'a PartName,
    data: &'a sml::CT_Worksheet,
    ctx: ReadCtx<'a>,
    shared: OnceCell<SharedFormulas>,
}

/// A row of a worksheet, see [`Worksheet::rows`].
pub struct Row<'s> {
    row: &'s sml::CT_Row,
    sheet: &'s Worksheet<'s>,
}

impl<'s> Row<'s> {
    /// Row number (1-based).
    pub fn index(&self) -> u32 {
        self.row.r.unwrap_or(0)
    }

    /// Custom height in points, if set.
    pub fn height(&self) -> Option<f64> {
        self.row.ht
    }

    /// Non-empty cells of the row with their values.
    pub fn cells(&self) -> impl Iterator<Item = (CellRef, CellValue)> + '_ {
        self.row.c.iter().filter_map(move |c| {
            let r = CellRef::parse(c.r.as_deref()?).ok()?;
            let v = self.sheet.value_of(c);
            (!v.is_empty()).then_some((r, v))
        })
    }

    /// The typed row.
    pub fn raw(&self) -> &sml::CT_Row {
        self.row
    }
}

impl<'a> Worksheet<'a> {
    pub(crate) fn new(
        name: &'a str,
        part: &'a PartName,
        data: &'a sml::CT_Worksheet,
        ctx: ReadCtx<'a>,
    ) -> Self {
        Worksheet {
            name,
            part,
            data,
            ctx,
            shared: OnceCell::new(),
        }
    }

    fn value_of(&self, c: &sml::CT_Cell) -> CellValue {
        cell_value(c, self.ctx, &|| {
            self.shared.get_or_init(|| shared_formulas(self.data))
        })
    }

    fn find(&self, r: CellRef) -> Option<&'a sml::CT_Cell> {
        let rows = &self.data.sheet_data.as_ref()?.row;
        let row = &rows[find_row(rows, r.row()).ok()?];
        row.c.get(find_cell(&row.c, r.col()).ok()?)
    }

    /// Sheet name.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Name of the worksheet part, e.g. `/xl/worksheets/sheet1.xml`.
    pub fn part_name(&self) -> &PartName {
        self.part
    }

    /// The typed worksheet.
    pub fn raw(&self) -> &sml::CT_Worksheet {
        self.data
    }

    /// Value of a cell (`Empty` if the cell does not exist).
    ///
    /// Shared strings are resolved, numbers with a date format become
    /// [`CellValue::DateTime`], and shared formulas are expanded for the cell.
    pub fn cell(&self, r: impl ToCellRef) -> Result<CellValue> {
        let r = r.to_cell_ref()?;
        Ok(self.find(r).map_or(CellValue::Empty, |c| self.value_of(c)))
    }

    /// Style of a cell, if the cell exists and has one.
    pub fn cell_style(&self, r: impl ToCellRef) -> Result<Option<StyleId>> {
        let r = r.to_cell_ref()?;
        Ok(self.find(r).and_then(|c| c.s).map(StyleId))
    }

    /// Rows that exist in the sheet, in order.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.data
            .sheet_data
            .iter()
            .flat_map(|d| d.row.iter())
            .map(move |row| Row { row, sheet: self })
    }

    /// The smallest range containing every cell element of the sheet.
    pub fn used_range(&self) -> Option<CellRange> {
        used_range(self.data)
    }

    /// The range recorded in the `<dimension>` element.
    pub fn dimension(&self) -> Option<CellRange> {
        self.data
            .dimension
            .as_ref()?
            .ref_
            .as_deref()
            .and_then(|r| CellRange::parse(r).ok())
    }

    /// Merged ranges.
    pub fn merged_ranges(&self) -> Vec<CellRange> {
        self.data
            .merge_cells
            .iter()
            .flat_map(|m| m.merge_cell.iter())
            .filter_map(|m| CellRange::parse(m.ref_.as_deref()?).ok())
            .collect()
    }

    /// Width of a column (in characters), if set.
    pub fn column_width(&self, col: u32) -> Option<f64> {
        self.data
            .cols
            .iter()
            .flat_map(|c| c.col.iter())
            .find(|c| c.min.unwrap_or(0) <= col && col <= c.max.unwrap_or(0))
            .and_then(|c| c.width)
    }

    /// Height of a row (in points), if set.
    pub fn row_height(&self, row: u32) -> Option<f64> {
        let rows = &self.data.sheet_data.as_ref()?.row;
        rows[find_row(rows, row).ok()?].ht
    }

    /// Top-left cell of the scrolling area if panes are frozen.
    pub fn frozen_at(&self) -> Option<CellRef> {
        let pane = self
            .data
            .sheet_views
            .as_ref()?
            .sheet_view
            .first()?
            .pane
            .as_ref()?;
        if !matches!(
            pane.state,
            Some(sml::ST_PaneState::Frozen | sml::ST_PaneState::FrozenSplit)
        ) {
            return None;
        }
        let cols = pane.x_split.unwrap_or(0.0) as u32;
        let rows = pane.y_split.unwrap_or(0.0) as u32;
        CellRef::new(rows + 1, cols + 1).ok()
    }
}

/// The smallest range containing every cell element.
pub(crate) fn used_range(ws: &sml::CT_Worksheet) -> Option<CellRange> {
    let rows = &ws.sheet_data.as_ref()?.row;
    let mut range: Option<CellRange> = None;
    for row in rows {
        let (Some(first), Some(last)) = (row.c.first(), row.c.last()) else {
            continue;
        };
        let r = row.r.unwrap_or(0);
        let (a, b) = (CellRef::new(r, cell_col(first)), CellRef::new(r, cell_col(last)));
        if let (Ok(a), Ok(b)) = (a, b) {
            let rr = CellRange::new(a, b);
            range = Some(range.map_or(rr, |x| x.union(&rr)));
        }
    }
    range
}

/// The range to record in `<dimension>`.
///
/// Producers differ in what they include (Excel counts formatted rows
/// without cells, others do not), so an existing dimension is kept as long
/// as it still contains every cell and is extended otherwise. The `A1`
/// placeholder of an empty sheet does not count as an existing dimension.
pub(crate) fn dimension_range(ws: &sml::CT_Worksheet) -> Option<CellRange> {
    let cells = used_range(ws);
    let a1 = CellRange::single(CellRef::new(1, 1).expect("A1"));
    let existing = ws
        .dimension
        .as_ref()
        .and_then(|d| d.ref_.as_deref())
        .and_then(|r| CellRange::parse(r).ok())
        .filter(|d| *d != a1 || cells.is_some_and(|c| c.contains(a1.start())));
    match (existing, cells) {
        (Some(d), Some(c)) if d.contains(c.start()) && d.contains(c.end()) => Some(d),
        (Some(d), Some(c)) => Some(d.union(&c)),
        (d, c) => d.or(c),
    }
}

fn parse_spans(spans: &XmlList<String>) -> Option<(u32, u32)> {
    let (mut lo, mut hi) = (u32::MAX, 0);
    for s in &spans.0 {
        let (a, b) = s.split_once(':')?;
        lo = lo.min(a.parse().ok()?);
        hi = hi.max(b.parse().ok()?);
    }
    (lo <= hi).then_some((lo, hi))
}

/// Refreshes `<dimension>` (when the sheet has one), row spans and
/// collection counts before saving.
pub(crate) fn finalize(ws: &mut sml::CT_Worksheet) {
    if ws.dimension.is_some() {
        let range = dimension_range(ws).map_or_else(|| "A1".to_owned(), |r| r.to_string());
        ws.dimension.get_or_insert_with(Box::default).ref_ = Some(range);
    }
    if let Some(data) = ws.sheet_data.as_mut() {
        for row in &mut data.row {
            // `spans` is an optimisation hint; keep it unless cells fall outside it.
            let (Some(spans), Some(first), Some(last)) = (row.spans.as_ref(), row.c.first(), row.c.last())
            else {
                continue;
            };
            let (a, b) = (cell_col(first), cell_col(last));
            match parse_spans(spans) {
                Some((lo, hi)) if lo <= a && b <= hi => {}
                Some((lo, hi)) => row.spans = Some(XmlList(vec![format!("{}:{}", lo.min(a), hi.max(b))])),
                None => row.spans = Some(XmlList(vec![format!("{a}:{b}")])),
            }
        }
    }
    if let Some(m) = ws.merge_cells.as_mut().filter(|m| m.count.is_some()) {
        m.count = Some(m.merge_cell.len() as u32);
    }
    if ws.merge_cells.as_ref().is_some_and(|m| m.merge_cell.is_empty()) {
        ws.merge_cells = None;
    }
}

/// An editable view of a worksheet.
///
/// ```
/// use openxml_xlsx::{Workbook, CellValue};
///
/// let mut wb = Workbook::new();
/// let mut sheet = wb.worksheet_mut("Sheet1")?;
/// sheet.set_value("A1", "Total")?;
/// sheet.set_value("B1", 42.5)?;
/// sheet.set_formula("C1", "B1*2")?;
/// assert_eq!(sheet.as_view().cell("B1")?, CellValue::Number(42.5));
/// # Ok::<(), openxml_core::Error>(())
/// ```
pub struct WorksheetMut<'a> {
    pub(crate) name: &'a str,
    pub(crate) part: &'a PartName,
    pub(crate) data: &'a mut sml::CT_Worksheet,
    pub(crate) sst: &'a mut SharedStrings,
    pub(crate) styles: &'a mut Styles,
    pub(crate) date_system: DateSystem,
    pub(crate) needs_recalc: &'a mut bool,
}

impl WorksheetMut<'_> {
    /// A read-only view (all reading methods live there).
    pub fn as_view(&self) -> Worksheet<'_> {
        Worksheet::new(
            self.name,
            self.part,
            self.data,
            ReadCtx {
                sst: self.sst,
                styles: self.styles,
                date_system: self.date_system,
            },
        )
    }

    /// Sheet name.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Value of a cell, see [`Worksheet::cell`].
    pub fn cell(&self, r: impl ToCellRef) -> Result<CellValue> {
        self.as_view().cell(r)
    }

    /// The typed worksheet, for changes the API does not cover.
    pub fn raw_mut(&mut self) -> &mut sml::CT_Worksheet {
        self.data
    }

    fn row_mut(&mut self, r: u32) -> &mut sml::CT_Row {
        let rows = &mut self.data.sheet_data.get_or_insert_with(Box::default).row;
        let i = match find_row(rows, r) {
            Ok(i) => i,
            Err(i) => {
                rows.insert(
                    i,
                    sml::CT_Row {
                        r: Some(r),
                        ..Default::default()
                    },
                );
                i
            }
        };
        &mut rows[i]
    }

    fn cell_mut(&mut self, r: CellRef) -> &mut sml::CT_Cell {
        let row = self.row_mut(r.row());
        let i = match find_cell(&row.c, r.col()) {
            Ok(i) => i,
            Err(i) => {
                row.c.insert(
                    i,
                    sml::CT_Cell {
                        r: Some(r.to_string()),
                        ..Default::default()
                    },
                );
                i
            }
        };
        &mut row.c[i]
    }

    /// Removes the cell's value (keeping its style). Cells and rows left
    /// without content are removed.
    pub fn clear(&mut self, r: impl ToCellRef) -> Result<()> {
        let r = r.to_cell_ref()?;
        let Some(data) = self.data.sheet_data.as_mut() else {
            return Ok(());
        };
        let Ok(ri) = find_row(&data.row, r.row()) else {
            return Ok(());
        };
        let row = &mut data.row[ri];
        if let Ok(ci) = find_cell(&row.c, r.col()) {
            clear_value(&mut row.c[ci]);
            let c = &row.c[ci];
            if c.s.is_none() && c.extra_attrs.is_empty() && c.extra_children.is_empty() && c.ext_lst.is_none()
            {
                row.c.remove(ci);
            }
        }
        let row_is_plain = row.c.is_empty()
            && row.s.is_none()
            && row.ht.is_none()
            && row.hidden.is_none()
            && row.outline_level.is_none()
            && row.extra_attrs.is_empty()
            && row.extra_children.is_empty();
        if row_is_plain {
            data.row.remove(ri);
        }
        Ok(())
    }

    /// Sets the value of a cell. Text goes to the shared string table; a
    /// [`CellValue::DateTime`] is stored as a serial number and gets a date
    /// format unless the cell already has one.
    pub fn set_value(&mut self, r: impl ToCellRef, value: impl Into<CellValue>) -> Result<()> {
        let r = r.to_cell_ref()?;
        let value = value.into();
        if value.is_empty() {
            return self.clear(r);
        }
        validate(&value)?;
        let date_system = self.date_system;
        // Resolve what needs the shared tables before borrowing the cell.
        let prepared = match &value {
            CellValue::Text(s) => Prepared::Shared(self.sst.intern(s)),
            CellValue::DateTime(d) => {
                let current = self.as_view().cell_style(r)?;
                let is_date = current.is_some_and(|s| self.styles.is_date_style(s.0));
                let style = if is_date {
                    current
                } else {
                    let fmt = if d.is_midnight() {
                        NumberFormat::DATE
                    } else {
                        NumberFormat::DATE_TIME
                    };
                    Some(self.styles.with_number_format(current, &fmt))
                };
                let serial = d.to_serial(date_system).ok_or_else(|| {
                    Error::InvalidArgument(format!(
                        "{d} cannot be represented in the {date_system:?} date system"
                    ))
                })?;
                Prepared::Date(serial, style)
            }
            CellValue::Formula { cached: None, .. } => {
                *self.needs_recalc = true;
                Prepared::None
            }
            _ => Prepared::None,
        };
        let cell = self.cell_mut(r);
        clear_value(cell);
        match (value, prepared) {
            (CellValue::Number(n), _) => cell.v = Some(format_number(n)),
            (CellValue::Text(_), Prepared::Shared(i)) => {
                cell.t = Some(sml::ST_CellType::S);
                cell.v = Some(i.to_string());
            }
            (CellValue::Bool(b), _) => {
                cell.t = Some(sml::ST_CellType::B);
                cell.v = Some(if b { "1" } else { "0" }.into());
            }
            (CellValue::Error(e), _) => {
                cell.t = Some(sml::ST_CellType::E);
                cell.v = Some(e);
            }
            (CellValue::DateTime(_), Prepared::Date(serial, style)) => {
                cell.v = Some(format_number(serial));
                cell.s = style.map(|s| s.0);
            }
            (CellValue::Formula { formula, cached }, _) => {
                cell.f = Some(Box::new(sml::CT_CellFormula {
                    value: encode_xstring(&formula).into_owned(),
                    ..Default::default()
                }));
                match cached.map(|c| *c) {
                    Some(CellValue::Number(n)) => cell.v = Some(format_number(n)),
                    Some(CellValue::Text(s)) => {
                        cell.t = Some(sml::ST_CellType::Str);
                        cell.v = Some(encode_xstring(&s).into_owned());
                    }
                    Some(CellValue::Bool(b)) => {
                        cell.t = Some(sml::ST_CellType::B);
                        cell.v = Some(if b { "1" } else { "0" }.into());
                    }
                    Some(CellValue::Error(e)) => {
                        cell.t = Some(sml::ST_CellType::E);
                        cell.v = Some(e);
                    }
                    Some(CellValue::DateTime(d)) => cell.v = d.to_serial(date_system).map(format_number),
                    _ => {}
                }
            }
            _ => unreachable!("prepared matches the value"),
        }
        Ok(())
    }

    /// Sets a formula without a cached result (a leading `=` is optional).
    /// Excel computes it when the workbook is opened.
    pub fn set_formula(&mut self, r: impl ToCellRef, formula: &str) -> Result<()> {
        self.set_value(r, CellValue::formula(formula))
    }

    /// Applies a cell format (from [`crate::Workbook::add_style`]) to a cell.
    pub fn set_cell_style(&mut self, r: impl ToCellRef, style: StyleId) -> Result<()> {
        let r = r.to_cell_ref()?;
        if style.0 as usize >= self.styles.len().max(1) {
            return Err(Error::InvalidArgument(format!("unknown style {}", style.0)));
        }
        self.cell_mut(r).s = (style.0 != 0).then_some(style.0);
        Ok(())
    }

    /// Applies a cell format to every cell of a range (creating empty cells).
    pub fn set_range_style(&mut self, range: CellRange, style: StyleId) -> Result<()> {
        for c in range.cells() {
            self.set_cell_style(c, style)?;
        }
        Ok(())
    }

    /// Merges a range. Fails if it overlaps an existing merge or is a single cell.
    pub fn merge_cells(&mut self, range: CellRange) -> Result<()> {
        if range.len() < 2 {
            return Err(Error::InvalidArgument(format!(
                "cannot merge a single cell ({range})"
            )));
        }
        if let Some(existing) = self
            .as_view()
            .merged_ranges()
            .into_iter()
            .find(|m| m.intersects(&range))
        {
            return Err(Error::InvalidArgument(format!(
                "{range} overlaps merged range {existing}"
            )));
        }
        let merges = self.data.merge_cells.get_or_insert_with(|| {
            Box::new(sml::CT_MergeCells {
                count: Some(0),
                ..Default::default()
            })
        });
        merges.merge_cell.push(sml::CT_MergeCell {
            ref_: Some(range.to_string()),
            ..Default::default()
        });
        Ok(())
    }

    /// Removes a merge. Returns whether it existed.
    pub fn unmerge_cells(&mut self, range: CellRange) -> bool {
        let Some(m) = self.data.merge_cells.as_mut() else {
            return false;
        };
        let before = m.merge_cell.len();
        m.merge_cell
            .retain(|c| c.ref_.as_deref().and_then(|r| CellRange::parse(r).ok()) != Some(range));
        before != m.merge_cell.len()
    }

    /// Sets the width of a column (in characters of the default font).
    pub fn set_column_width(&mut self, col: u32, width: f64) -> Result<()> {
        if !(1..=MAX_COL).contains(&col) {
            return Err(Error::InvalidArgument(format!("column out of range: {col}")));
        }
        if !(0.0..=255.0).contains(&width) {
            return Err(Error::InvalidArgument(format!(
                "column width out of range: {width}"
            )));
        }
        if self.data.cols.is_empty() {
            self.data.cols.push(sml::CT_Cols::default());
        }
        let cols = &mut self.data.cols[0].col;
        // Split any definition that covers the column.
        let mut out = Vec::with_capacity(cols.len() + 2);
        for c in cols.drain(..) {
            let (min, max) = (c.min.unwrap_or(0), c.max.unwrap_or(0));
            if min <= col && col <= max {
                if min < col {
                    out.push(sml::CT_Col {
                        max: Some(col - 1),
                        ..c.clone()
                    });
                }
                if col < max {
                    out.push(sml::CT_Col {
                        min: Some(col + 1),
                        ..c.clone()
                    });
                }
                out.push(sml::CT_Col {
                    min: Some(col),
                    max: Some(col),
                    width: Some(width),
                    custom_width: Some(true),
                    ..c
                });
            } else {
                out.push(c);
            }
        }
        if !out.iter().any(|c| c.min == Some(col)) {
            out.push(sml::CT_Col {
                min: Some(col),
                max: Some(col),
                width: Some(width),
                custom_width: Some(true),
                ..Default::default()
            });
        }
        out.sort_by_key(|c| c.min);
        *cols = out;
        Ok(())
    }

    /// Sets the height of a row in points.
    pub fn set_row_height(&mut self, row: u32, points: f64) -> Result<()> {
        if !(1..=MAX_ROW).contains(&row) {
            return Err(Error::InvalidArgument(format!("row out of range: {row}")));
        }
        if !(0.0..=409.0).contains(&points) {
            return Err(Error::InvalidArgument(format!(
                "row height out of range: {points}"
            )));
        }
        let r = self.row_mut(row);
        r.ht = Some(points);
        r.custom_height = Some(true);
        Ok(())
    }

    /// Freezes the rows above and the columns left of `top_left` (the first
    /// scrolling cell). Freezing at `A1` removes frozen panes.
    pub fn freeze_panes(&mut self, top_left: impl ToCellRef) -> Result<()> {
        let at = top_left.to_cell_ref()?;
        let views = self.data.sheet_views.get_or_insert_with(Box::default);
        if views.sheet_view.is_empty() {
            views.sheet_view.push(sml::CT_SheetView {
                workbook_view_id: Some(0),
                ..Default::default()
            });
        }
        let view = &mut views.sheet_view[0];
        let (rows, cols) = (at.row() - 1, at.col() - 1);
        if rows == 0 && cols == 0 {
            view.pane = None;
            view.selection.retain(|s| s.pane.is_none());
            return Ok(());
        }
        let active = match (rows > 0, cols > 0) {
            (true, true) => sml::ST_Pane::BottomRight,
            (true, false) => sml::ST_Pane::BottomLeft,
            _ => sml::ST_Pane::TopRight,
        };
        view.pane = Some(Box::new(sml::CT_Pane {
            x_split: (cols > 0).then_some(f64::from(cols)),
            y_split: (rows > 0).then_some(f64::from(rows)),
            top_left_cell: Some(at.to_string()),
            active_pane: Some(active),
            state: Some(sml::ST_PaneState::Frozen),
            ..Default::default()
        }));
        view.selection = vec![sml::CT_Selection {
            pane: Some(active),
            active_cell: Some(at.to_string()),
            sqref: Some(XmlList(vec![at.to_string()])),
            ..Default::default()
        }];
        Ok(())
    }
}

enum Prepared {
    None,
    Shared(u32),
    Date(f64, Option<StyleId>),
}

fn validate(value: &CellValue) -> Result<()> {
    match value {
        CellValue::Number(n) if !n.is_finite() => {
            Err(Error::InvalidArgument(format!("{n} is not a finite number")))
        }
        CellValue::Text(s) if s.chars().count() > MAX_TEXT_LEN => Err(Error::InvalidArgument(format!(
            "text longer than {MAX_TEXT_LEN} characters"
        ))),
        CellValue::Error(e) if !e.starts_with('#') => {
            Err(Error::InvalidArgument(format!("invalid error value {e:?}")))
        }
        CellValue::Formula { formula, cached } => {
            if formula.trim().is_empty() {
                return Err(Error::InvalidArgument("empty formula".into()));
            }
            if let Some(c) = cached {
                if matches!(**c, CellValue::Formula { .. }) {
                    return Err(Error::InvalidArgument(
                        "a formula result cannot be a formula".into(),
                    ));
                }
                validate(c)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn clear_value(c: &mut sml::CT_Cell) {
    c.t = None;
    c.v = None;
    c.f = None;
    c.is = None;
    c.cm = None;
    c.vm = None;
    c.extra_attrs.retain(|a| !a.name.is(Ns::NONE, "t"));
    c.extra_children.retain(|e| {
        !(e.element.name.is(Ns::X, "v") || e.element.name.is(Ns::X, "f") || e.element.name.is(Ns::X, "is"))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(xml_rows: &str) -> sml::CT_Worksheet {
        let xml = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{xml_rows}</sheetData></worksheet>"#
        );
        let mut w = sml::elements::WORKSHEET.parse(&xml).unwrap();
        normalize(&mut w);
        w
    }

    #[test]
    fn normalize_fills_missing_references_and_sorts() {
        let w = ws(
            r#"<row><c><v>1</v></c><c><v>2</v></c></row><row r="5"><c r="C5"/><c><v>3</v></c></row><row r="3"><c r="B3"/><c r="A3"/></row>"#,
        );
        let rows = &w.sheet_data.as_ref().unwrap().row;
        assert_eq!(rows.iter().map(|r| r.r.unwrap()).collect::<Vec<_>>(), [1, 3, 5]);
        assert_eq!(
            rows[0].c.iter().map(|c| c.r.clone().unwrap()).collect::<Vec<_>>(),
            ["A1", "B1"]
        );
        assert_eq!(
            rows[1].c.iter().map(|c| c.r.clone().unwrap()).collect::<Vec<_>>(),
            ["A3", "B3"]
        );
        assert_eq!(
            rows[2].c.iter().map(|c| c.r.clone().unwrap()).collect::<Vec<_>>(),
            ["C5", "D5"]
        );
    }

    #[test]
    fn used_range_and_finalize() {
        let mut w = ws(
            r#"<row r="2" spans="1:9"><c r="B2"/><c r="D2"/></row><row r="7" spans="3:4"><c r="A7"/></row><row r="9" spans="1:4" ht="20"/>"#,
        );
        assert_eq!(used_range(&w).unwrap().to_string(), "A2:D7");
        assert_eq!(
            dimension_range(&w).unwrap().to_string(),
            "A2:D7",
            "without a dimension, the cells"
        );
        w.dimension = Some(Box::new(sml::CT_SheetDimension {
            ref_: Some("A1:D9".into()),
            ..Default::default()
        }));
        assert_eq!(
            dimension_range(&w).unwrap().to_string(),
            "A1:D9",
            "a covering dimension is kept"
        );
        w.dimension = Some(Box::new(sml::CT_SheetDimension {
            ref_: Some("B2:C3".into()),
            ..Default::default()
        }));
        assert_eq!(
            dimension_range(&w).unwrap().to_string(),
            "A2:D7",
            "a stale dimension grows"
        );
        w.dimension = Some(Box::new(sml::CT_SheetDimension {
            ref_: Some("A1".into()),
            ..Default::default()
        }));
        assert_eq!(
            dimension_range(&w).unwrap().to_string(),
            "A2:D7",
            "the A1 placeholder is ignored"
        );
        finalize(&mut w);
        assert_eq!(w.dimension.as_ref().unwrap().ref_.as_deref(), Some("A2:D7"));
        let rows = &w.sheet_data.as_ref().unwrap().row;
        assert_eq!(
            rows[0].spans,
            Some(XmlList(vec!["1:9".to_owned()])),
            "valid spans are kept"
        );
        assert_eq!(
            rows[1].spans,
            Some(XmlList(vec!["1:4".to_owned()])),
            "spans grow to cover cells"
        );
        assert_eq!(
            rows[2].spans,
            Some(XmlList(vec!["1:4".to_owned()])),
            "empty rows keep their spans"
        );
        assert_eq!(
            parse_spans(&XmlList(vec!["1:2".into(), "5:7".into()])),
            Some((1, 7))
        );
        assert_eq!(parse_spans(&XmlList(vec!["x".into()])), None);
        let mut empty = ws("");
        assert_eq!(used_range(&empty), None);
        finalize(&mut empty);
        assert!(empty.dimension.is_none(), "an absent dimension stays absent");
        empty.dimension = Some(Box::default());
        finalize(&mut empty);
        assert_eq!(empty.dimension.as_ref().unwrap().ref_.as_deref(), Some("A1"));
    }

    #[test]
    fn shared_formula_masters() {
        let w = ws(
            r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A3" si="0">B1*2</f><v>2</v></c></row><row r="2"><c r="A2"><f t="shared" si="0"/><v>4</v></c></row>"#,
        );
        let map = shared_formulas(&w);
        assert_eq!(map[&0], (CellRef::parse("A1").unwrap(), "B1*2".to_owned()));
    }

    #[test]
    fn value_validation() {
        assert!(validate(&CellValue::Number(f64::INFINITY)).is_err());
        assert!(validate(&CellValue::Text("x".repeat(MAX_TEXT_LEN + 1))).is_err());
        assert!(validate(&CellValue::Text("x".repeat(MAX_TEXT_LEN))).is_ok());
        assert!(validate(&CellValue::Error("oops".into())).is_err());
        assert!(validate(&CellValue::formula("")).is_err());
        let nested = CellValue::Formula {
            formula: "A1".into(),
            cached: Some(Box::new(CellValue::formula("B1"))),
        };
        assert!(validate(&nested).is_err());
        assert!(validate(&CellValue::formula_with_result("A1", f64::NAN)).is_err());
        assert!(validate(&CellValue::Bool(true)).is_ok());
    }
}
