//! Writing large worksheets row by row.

use std::fmt::Write as _;

use openxml_core::{Error, Result};
use openxml_opc::known::content_types as ct;
use openxml_schema::sml;
use openxml_xml::{escape_attr, escape_text};

use crate::cell_ref::{CellRange, CellRef, MAX_COL, MAX_ROW, ToCellRef};
use crate::styles::{NumberFormat, StyleId};
use crate::value::{CellValue, encode_xstring, format_number};
use crate::workbook::{Workbook, new_worksheet};
use crate::worksheet::WorksheetMut;

/// A worksheet written row by row.
///
/// Rows are serialized as soon as they are written, so memory use is
/// proportional to the XML text of the sheet rather than to a full object
/// model. Text still goes through the workbook's shared string table.
/// The sheet is added to the workbook by [`StreamingWorksheet::finish`];
/// dropping the writer without finishing discards it.
///
/// ```
/// use openxml_xlsx::{Workbook, CellValue};
///
/// let mut wb = Workbook::new();
/// let mut sheet = wb.add_streaming_worksheet("Data")?;
/// sheet.write_row(["id", "value"])?;
/// for i in 1..=1000 {
///     sheet.write_row([CellValue::from(i), CellValue::from(i as f64 * 1.5)])?;
/// }
/// sheet.finish()?;
/// assert_eq!(wb.worksheet("Data")?.cell("B1001")?.as_f64(), Some(1500.0));
/// # Ok::<(), openxml_core::Error>(())
/// ```
pub struct StreamingWorksheet<'a> {
    wb: &'a mut Workbook,
    name: String,
    rows: String,
    next_row: u32,
    used: Option<CellRange>,
    columns: Vec<(u32, f64)>,
    freeze: Option<CellRef>,
    date_style: Option<StyleId>,
    date_time_style: Option<StyleId>,
}

impl<'a> StreamingWorksheet<'a> {
    pub(crate) fn new(wb: &'a mut Workbook, name: &str) -> Self {
        StreamingWorksheet {
            wb,
            name: name.to_owned(),
            rows: String::new(),
            next_row: 1,
            used: None,
            columns: Vec::new(),
            freeze: None,
            date_style: None,
            date_time_style: None,
        }
    }

    /// Number of the row the next `write_row` call writes.
    pub fn next_row(&self) -> u32 {
        self.next_row
    }

    /// Leaves `count` rows empty.
    pub fn skip_rows(&mut self, count: u32) -> Result<()> {
        let next = self.next_row.saturating_add(count);
        if next > MAX_ROW + 1 {
            return Err(Error::InvalidArgument(format!(
                "row {next} is beyond the last row"
            )));
        }
        self.next_row = next;
        Ok(())
    }

    /// Writes the next row; values start in column A. Returns the row number.
    pub fn write_row<I>(&mut self, values: I) -> Result<u32>
    where
        I: IntoIterator,
        I::Item: Into<CellValue>,
    {
        self.write_styled_row(values.into_iter().map(|v| (v, None)))
    }

    /// Writes the next row with a style per cell. Returns the row number.
    pub fn write_styled_row<I, V>(&mut self, cells: I) -> Result<u32>
    where
        I: IntoIterator<Item = (V, Option<StyleId>)>,
        V: Into<CellValue>,
    {
        let row = self.next_row;
        if row > MAX_ROW {
            return Err(Error::InvalidArgument("the worksheet is full".into()));
        }
        let mut xml = String::new();
        let mut last_col = 0;
        for (i, (value, style)) in cells.into_iter().enumerate() {
            let col = u32::try_from(i + 1).unwrap_or(u32::MAX);
            if col > MAX_COL {
                return Err(Error::InvalidArgument(format!(
                    "row {row} has more than {MAX_COL} cells"
                )));
            }
            let value = value.into();
            if value.is_empty() && style.is_none() {
                continue;
            }
            if let Some(s) = style
                && s.0 as usize >= self.wb.styles.len().max(1)
            {
                return Err(Error::InvalidArgument(format!("unknown style {}", s.0)));
            }
            let at = CellRef::new(row, col)?;
            self.write_cell(&mut xml, at, value, style)?;
            last_col = col;
            let cell_range = CellRange::single(at);
            self.used = Some(self.used.map_or(cell_range, |u| u.union(&cell_range)));
        }
        if last_col > 0 {
            let _ = write!(self.rows, "<row r=\"{row}\">{xml}</row>");
        }
        self.next_row += 1;
        Ok(row)
    }

    fn default_date_style(&mut self, with_time: bool) -> StyleId {
        let slot = if with_time {
            &mut self.date_time_style
        } else {
            &mut self.date_style
        };
        *slot.get_or_insert_with(|| {
            let fmt = if with_time {
                NumberFormat::DATE_TIME
            } else {
                NumberFormat::DATE
            };
            self.wb.styles.with_number_format(None, &fmt)
        })
    }

    fn write_cell(
        &mut self,
        out: &mut String,
        at: CellRef,
        value: CellValue,
        style: Option<StyleId>,
    ) -> Result<()> {
        let date_system = self.wb.date_system;
        let mut style = style;
        let (t, body) = match value {
            CellValue::Empty => (None, String::new()),
            CellValue::Number(n) if n.is_finite() => (None, format!("<v>{}</v>", format_number(n))),
            CellValue::Number(n) => {
                return Err(Error::InvalidArgument(format!("{n} is not a finite number")));
            }
            CellValue::Text(s) => {
                if s.chars().count() > crate::value::MAX_TEXT_LEN {
                    return Err(Error::InvalidArgument("text longer than 32767 characters".into()));
                }
                (Some("s"), format!("<v>{}</v>", self.wb.sst.intern(&s)))
            }
            CellValue::Bool(b) => (Some("b"), format!("<v>{}</v>", u8::from(b))),
            CellValue::Error(e) => {
                let mut v = String::new();
                escape_text(&mut v, &e);
                (Some("e"), format!("<v>{v}</v>"))
            }
            CellValue::DateTime(d) => {
                let serial = d
                    .to_serial(date_system)
                    .ok_or_else(|| Error::InvalidArgument(format!("{d} is before the epoch")))?;
                if style.is_none() {
                    style = Some(self.default_date_style(!d.is_midnight()));
                }
                (None, format!("<v>{}</v>", format_number(serial)))
            }
            CellValue::Formula { formula, cached } => {
                let mut f = String::new();
                escape_text(
                    &mut f,
                    &encode_xstring(formula.strip_prefix('=').unwrap_or(&formula)),
                );
                let (t, v) = match cached.map(|c| *c) {
                    None => {
                        self.wb.needs_recalc = true;
                        (None, String::new())
                    }
                    Some(CellValue::Number(n)) => (None, format_number(n)),
                    Some(CellValue::Text(s)) => {
                        let mut v = String::new();
                        escape_text(&mut v, &encode_xstring(&s));
                        (Some("str"), v)
                    }
                    Some(CellValue::Bool(b)) => (Some("b"), u8::from(b).to_string()),
                    Some(CellValue::Error(e)) => {
                        let mut v = String::new();
                        escape_text(&mut v, &e);
                        (Some("e"), v)
                    }
                    Some(CellValue::DateTime(d)) => (
                        None,
                        d.to_serial(date_system).map(format_number).unwrap_or_default(),
                    ),
                    Some(_) => (None, String::new()),
                };
                let v = if v.is_empty() {
                    String::new()
                } else {
                    format!("<v>{v}</v>")
                };
                (t, format!("<f>{f}</f>{v}"))
            }
        };
        let _ = write!(out, "<c r=\"{at}\"");
        if let Some(s) = style.filter(|s| s.0 != 0) {
            let _ = write!(out, " s=\"{}\"", s.0);
        }
        if let Some(t) = t {
            out.push_str(" t=\"");
            escape_attr(out, t);
            out.push('"');
        }
        if body.is_empty() {
            out.push_str("/>");
        } else {
            let _ = write!(out, ">{body}</c>");
        }
        Ok(())
    }

    /// Sets the width of a column (applied when the sheet is finished).
    pub fn set_column_width(&mut self, col: u32, width: f64) -> Result<()> {
        if !(1..=MAX_COL).contains(&col) || !(0.0..=255.0).contains(&width) {
            return Err(Error::InvalidArgument(format!(
                "invalid width {width} for column {col}"
            )));
        }
        self.columns.retain(|(c, _)| *c != col);
        self.columns.push((col, width));
        Ok(())
    }

    /// Freezes the panes above and left of `top_left`.
    pub fn freeze_panes(&mut self, top_left: impl ToCellRef) -> Result<()> {
        self.freeze = Some(top_left.to_cell_ref()?);
        Ok(())
    }

    /// Adds the sheet to the workbook.
    pub fn finish(self) -> Result<()> {
        let StreamingWorksheet {
            wb,
            name,
            rows,
            used,
            columns,
            freeze,
            ..
        } = self;
        let part = wb.package_mut().next_part_name("/xl/worksheets/sheet{}.xml")?;
        let mut ws = new_worksheet(false);
        ws.dimension.get_or_insert_with(Box::default).ref_ =
            Some(used.map_or_else(|| "A1".into(), |u| u.to_string()));
        {
            let mut recalc = false;
            let mut view = WorksheetMut {
                name: &name,
                part: &part,
                data: &mut ws,
                sst: &mut wb.sst,
                styles: &mut wb.styles,
                date_system: wb.date_system,
                needs_recalc: &mut recalc,
            };
            for (col, width) in columns {
                view.set_column_width(col, width)?;
            }
            if let Some(f) = freeze {
                view.freeze_panes(f)?;
            }
        }
        let xml = sml::elements::WORKSHEET.to_xml(&ws);
        let xml = xml.replacen("<sheetData/>", &format!("<sheetData>{rows}</sheetData>"), 1);
        wb.package_mut()
            .add_part(part.clone(), ct::SML_WORKSHEET, xml.into_bytes())?;
        wb.register_sheet(&name, part, None)?;
        Ok(())
    }
}
