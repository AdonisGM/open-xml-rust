//! Tables (Excel "ListObjects"): a range with a header row, optional
//! totals row, filter buttons and a table style, stored in a table part
//! (`xl/tables/tableN.xml`, ECMA-376 Part 1 §18.5).
//!
//! ```
//! use openxml_xlsx::{Workbook, Table, TotalsRow};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_value("A1", "Region")?;
//! sheet.set_value("B1", "Sales")?;
//! sheet.set_value("A2", "North")?;
//! sheet.set_value("B2", 120.0)?;
//! sheet.set_value("A3", "South")?;
//! sheet.set_value("B3", 80.0)?;
//! let table = sheet.add_table(
//!     "A1:B4",
//!     &Table::new().name("Sales").totals("Region", TotalsRow::Label("Total".into())).totals("Sales", TotalsRow::Sum),
//! )?;
//! assert_eq!(table.columns[1].name, "Sales");
//! assert_eq!(sheet.cell("B4")?.as_formula(), Some("SUBTOTAL(109,Sales[Sales])"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use std::borrow::Cow;

use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::sml;

use crate::cell_ref::{CellRange, CellRef, ToCellRange};
use crate::parts::{SideParts, SideValue};
use crate::util::remove_relationship_and_orphans;
use crate::value::{CellValue, format_number};
use crate::worksheet::{Worksheet, WorksheetMut};

/// What the totals row shows under a column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TotalsRow {
    /// Sum (`SUBTOTAL(109, …)`).
    Sum,
    /// Average (`SUBTOTAL(101, …)`).
    Average,
    /// Count of non-empty cells (`SUBTOTAL(103, …)`).
    Count,
    /// Count of numbers (`SUBTOTAL(102, …)`).
    CountNumbers,
    /// Maximum (`SUBTOTAL(104, …)`).
    Max,
    /// Minimum (`SUBTOTAL(105, …)`).
    Min,
    /// Sample standard deviation (`SUBTOTAL(107, …)`).
    StdDev,
    /// Sample variance (`SUBTOTAL(110, …)`).
    Var,
    /// A text label.
    Label(String),
    /// A custom formula.
    Formula(String),
}

impl TotalsRow {
    fn function(&self) -> Option<(sml::ST_TotalsRowFunction, u32)> {
        use sml::ST_TotalsRowFunction as F;
        Some(match self {
            TotalsRow::Sum => (F::Sum, 109),
            TotalsRow::Average => (F::Average, 101),
            TotalsRow::Count => (F::Count, 103),
            TotalsRow::CountNumbers => (F::CountNums, 102),
            TotalsRow::Max => (F::Max, 104),
            TotalsRow::Min => (F::Min, 105),
            TotalsRow::StdDev => (F::StdDev, 107),
            TotalsRow::Var => (F::Var, 110),
            TotalsRow::Label(_) | TotalsRow::Formula(_) => return None,
        })
    }
}

/// A column of a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableColumn {
    /// Column name (the header cell text).
    pub name: String,
    /// Totals row content.
    pub totals: Option<TotalsRow>,
}

/// Options for a new table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    name: Option<String>,
    columns: Vec<String>,
    totals: Vec<(String, TotalsRow)>,
    style: Option<String>,
    first_column: bool,
    last_column: bool,
    row_stripes: bool,
    column_stripes: bool,
    header_row: bool,
    totals_row: bool,
    auto_filter: bool,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// A table with a header row, filter buttons, banded rows and Excel's
    /// default style `TableStyleMedium2`.
    pub fn new() -> Self {
        Table {
            name: None,
            columns: Vec::new(),
            totals: Vec::new(),
            style: Some("TableStyleMedium2".into()),
            first_column: false,
            last_column: false,
            row_stripes: true,
            column_stripes: false,
            header_row: true,
            totals_row: false,
            auto_filter: true,
        }
    }

    /// Table name (unique in the workbook; default `TableN`).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Column names; they are written into the header row. By default the
    /// names are taken from the header cells.
    pub fn columns<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.columns = names.into_iter().map(Into::into).collect();
        self
    }

    /// Table style name (e.g. `TableStyleLight9`); `None` for no style.
    pub fn style(mut self, style: Option<&str>) -> Self {
        self.style = style.map(str::to_owned);
        self
    }

    /// Emphasizes the first column.
    pub fn first_column(mut self, on: bool) -> Self {
        self.first_column = on;
        self
    }

    /// Emphasizes the last column.
    pub fn last_column(mut self, on: bool) -> Self {
        self.last_column = on;
        self
    }

    /// Banded rows.
    pub fn row_stripes(mut self, on: bool) -> Self {
        self.row_stripes = on;
        self
    }

    /// Banded columns.
    pub fn column_stripes(mut self, on: bool) -> Self {
        self.column_stripes = on;
        self
    }

    /// A table without a header row (column names are then `Column1`…).
    pub fn no_header_row(mut self) -> Self {
        self.header_row = false;
        self
    }

    /// Without filter buttons.
    pub fn no_auto_filter(mut self) -> Self {
        self.auto_filter = false;
        self
    }

    /// Adds a totals row (the last row of the range) showing `totals` under
    /// the column named `column`.
    pub fn totals(mut self, column: impl Into<String>, totals: TotalsRow) -> Self {
        self.totals_row = true;
        self.totals.push((column.into(), totals));
        self
    }

    /// Adds an empty totals row.
    pub fn totals_row(mut self) -> Self {
        self.totals_row = true;
        self
    }
}

/// A table found on a worksheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    /// Table id (unique in the workbook).
    pub id: u32,
    /// Name used in structured references.
    pub name: String,
    /// The whole range, including header and totals rows.
    pub range: CellRange,
    /// Columns.
    pub columns: Vec<TableColumn>,
    /// Style name.
    pub style: Option<String>,
    /// Whether the first row is a header row.
    pub header_row: bool,
    /// Whether the last row is a totals row.
    pub totals_row: bool,
    /// Range of the filter buttons, if shown.
    pub auto_filter: Option<CellRange>,
    /// The table part.
    pub part: PartName,
}

fn info(t: &sml::CT_Table, part: PartName) -> Option<TableInfo> {
    let range = CellRange::parse(t.ref_.as_deref()?).ok()?;
    let columns = t
        .table_columns
        .as_ref()
        .map(|c| {
            c.table_column
                .iter()
                .map(|c| TableColumn {
                    name: c.name.clone().unwrap_or_default(),
                    totals: totals_of(c),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(TableInfo {
        id: t.id.unwrap_or(0),
        name: t
            .display_name
            .clone()
            .or_else(|| t.name.clone())
            .unwrap_or_default(),
        range,
        columns,
        style: t.table_style_info.as_ref().and_then(|s| s.name.clone()),
        header_row: t.header_row_count != Some(0),
        totals_row: t.totals_row_count.unwrap_or(0) > 0,
        auto_filter: t
            .auto_filter
            .as_ref()
            .and_then(|a| a.ref_.as_deref())
            .and_then(|r| CellRange::parse(r).ok()),
        part,
    })
}

fn totals_of(c: &sml::CT_TableColumn) -> Option<TotalsRow> {
    use sml::ST_TotalsRowFunction as F;
    if let Some(label) = &c.totals_row_label {
        return Some(TotalsRow::Label(label.clone()));
    }
    Some(match c.totals_row_function? {
        F::None => return None,
        F::Sum => TotalsRow::Sum,
        F::Min => TotalsRow::Min,
        F::Max => TotalsRow::Max,
        F::Average => TotalsRow::Average,
        F::Count => TotalsRow::Count,
        F::CountNums => TotalsRow::CountNumbers,
        F::StdDev => TotalsRow::StdDev,
        F::Var => TotalsRow::Var,
        F::Custom => TotalsRow::Formula(
            c.totals_row_formula
                .as_ref()
                .map(|f| f.value.clone())
                .unwrap_or_default(),
        ),
    })
}

/// Escapes a column name for use in a structured reference.
fn escape_column(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '[' | ']' | '#' | '\'') {
            out.push('\'');
        }
        out.push(c);
    }
    out
}

/// Whether `name` is a valid table or defined name.
pub(crate) fn valid_name(name: &str) -> bool {
    name.chars().count() <= 255
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\'))
        && CellRef::parse(name).is_err()
        && !name.eq_ignore_ascii_case("r")
        && !name.eq_ignore_ascii_case("c")
}

/// Tables of every sheet in the package: (part, id, name).
fn all_tables(pkg: &Package, side: &SideParts) -> Result<Vec<(PartName, u32, String)>> {
    let names: Vec<PartName> = pkg
        .parts()
        .filter(|(_, p)| p.content_type() == ct::SML_TABLE)
        .map(|(n, _)| n.clone())
        .collect();
    let mut out = Vec::new();
    for n in names {
        let t = side.peek_table(pkg, &n)?;
        out.push((
            n.clone(),
            t.id.unwrap_or(0),
            t.display_name
                .clone()
                .or_else(|| t.name.clone())
                .unwrap_or_default(),
        ));
    }
    Ok(out)
}

/// The table parts of a sheet with their relationship ids.
pub(crate) fn sheet_tables(
    ws: &sml::CT_Worksheet,
    pkg: &Package,
    sheet: &PartName,
) -> Vec<(String, PartName)> {
    ws.table_parts
        .iter()
        .flat_map(|t| t.table_part.iter())
        .filter_map(|t| {
            let rid = t.r_id.clone()?;
            let part = pkg
                .relationship_target(Some(sheet), &rid)
                .filter(|p| pkg.contains(p))?;
            Some((rid, part))
        })
        .collect()
}

fn display_text(v: &CellValue) -> String {
    match v {
        CellValue::Empty => String::new(),
        CellValue::Text(s) => s.clone(),
        CellValue::Number(n) => format_number(*n),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        CellValue::Error(e) => e.clone(),
        CellValue::DateTime(d) => d.to_iso(),
        CellValue::Formula { cached, .. } => cached.as_deref().map(display_text).unwrap_or_default(),
    }
}

impl Worksheet<'_> {
    /// The tables of the sheet.
    pub fn tables(&self) -> Result<Vec<TableInfo>> {
        let mut out = Vec::new();
        for (_, part) in sheet_tables(self.data, self.env.package, self.part) {
            let t = self.env.side.peek_table(self.env.package, &part)?;
            if let Some(i) = info(&t, part) {
                out.push(i);
            }
        }
        Ok(out)
    }

    /// A table of the sheet by name (case-insensitive).
    pub fn table(&self, name: &str) -> Result<Option<TableInfo>> {
        Ok(self
            .tables()?
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name)))
    }

    /// The typed table part of a table of this sheet.
    pub fn raw_table(&self, name: &str) -> Result<Option<Cow<'_, sml::CT_Table>>> {
        match self.table(name)? {
            Some(t) => Ok(Some(self.env.side.peek_table(self.env.package, &t.part)?)),
            None => Ok(None),
        }
    }
}

impl WorksheetMut<'_> {
    /// Turns a range into a table. The first row is the header row (its
    /// cells receive the column names, which must be unique); with a totals
    /// row, the last row holds the totals.
    pub fn add_table(&mut self, range: impl ToCellRange, options: &Table) -> Result<TableInfo> {
        let range = range.to_cell_range()?;
        let min_height = u32::from(options.header_row) + 1 + u32::from(options.totals_row);
        if range.height() < min_height {
            return Err(Error::InvalidArgument(format!(
                "{range} is too small for a table with these rows"
            )));
        }
        for existing in self.as_view().tables()? {
            if existing.range.intersects(&range) {
                return Err(Error::InvalidArgument(format!(
                    "{range} overlaps table {}",
                    existing.name
                )));
            }
        }
        if let Some(af) = self.data.auto_filter.as_ref().and_then(|a| a.ref_.as_deref())
            && CellRange::parse(af).is_ok_and(|af| af.intersects(&range))
        {
            return Err(Error::InvalidArgument(format!(
                "{range} overlaps the sheet's auto filter"
            )));
        }
        let header = CellRange::new(
            range.start(),
            CellRef::new(range.start().row(), range.end().col())?,
        );
        if options.header_row
            && let Some(m) = self
                .as_view()
                .merged_ranges()
                .into_iter()
                .find(|m| m.intersects(&header))
        {
            return Err(Error::InvalidArgument(format!(
                "the header row cannot contain merged cells ({m})"
            )));
        }
        let tables = all_tables(self.package, self.side)?;
        let id = tables.iter().map(|t| t.1).max().unwrap_or(0) + 1;
        let name = options.name.clone().unwrap_or_else(|| format!("Table{id}"));
        if !valid_name(&name) {
            return Err(Error::InvalidArgument(format!("invalid table name {name:?}")));
        }
        let taken = tables.iter().any(|t| t.2.eq_ignore_ascii_case(&name))
            || self
                .workbook
                .defined_names
                .iter()
                .flat_map(|d| d.defined_name.iter())
                .any(|d| d.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&name)));
        if taken {
            return Err(Error::InvalidArgument(format!(
                "the name {name:?} is already in use"
            )));
        }

        // Column names: given, or from the header cells.
        let width = range.width() as usize;
        let mut names: Vec<String> = if !options.columns.is_empty() {
            if options.columns.len() != width {
                return Err(Error::InvalidArgument(format!(
                    "{} column names for {width} columns",
                    options.columns.len()
                )));
            }
            options.columns.clone()
        } else if options.header_row {
            let view = self.as_view();
            header
                .cells()
                .map(|c| view.cell(c).map(|v| display_text(&v)))
                .collect::<Result<_>>()?
        } else {
            vec![String::new(); width]
        };
        for i in 0..names.len() {
            let base = names[i].trim().to_owned();
            let base = if base.is_empty() {
                format!("Column{}", i + 1)
            } else {
                base
            };
            let mut candidate = base.clone();
            let mut n = 2;
            while names[..i].iter().any(|x| x.eq_ignore_ascii_case(&candidate)) {
                candidate = format!("{base}{n}");
                n += 1;
            }
            names[i] = candidate;
        }
        for (column, _) in &options.totals {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(column)) {
                return Err(Error::NotFound(format!("table column {column:?}")));
            }
        }
        if options.header_row {
            for (cell, n) in header.cells().zip(&names) {
                if self.cell(cell)?.as_str() != Some(n.as_str()) {
                    self.set_value(cell, n.as_str())?;
                }
            }
        }

        // Totals row cells.
        let totals_row = range.end().row();
        let mut columns = Vec::with_capacity(width);
        for (i, n) in names.iter().enumerate() {
            let totals = options
                .totals
                .iter()
                .find(|(c, _)| c.eq_ignore_ascii_case(n))
                .map(|(_, t)| t.clone());
            let mut col = sml::CT_TableColumn {
                id: Some(i as u32 + 1),
                name: Some(n.clone()),
                ..Default::default()
            };
            let cell = CellRef::new(totals_row, range.start().col() + i as u32)?;
            match &totals {
                Some(TotalsRow::Label(l)) => {
                    col.totals_row_label = Some(l.clone());
                    self.set_value(cell, l.as_str())?;
                }
                Some(TotalsRow::Formula(f)) => {
                    let f = f.trim_start_matches('=').to_owned();
                    col.totals_row_function = Some(sml::ST_TotalsRowFunction::Custom);
                    col.totals_row_formula = Some(Box::new(sml::CT_TableFormula {
                        value: f.clone(),
                        ..Default::default()
                    }));
                    self.set_formula(cell, &f)?;
                }
                Some(t) => {
                    let (function, code) = t.function().expect("function totals");
                    col.totals_row_function = Some(function);
                    self.set_formula(cell, &format!("SUBTOTAL({code},{name}[{}])", escape_column(n)))?;
                }
                None => {}
            }
            columns.push(sml::CT_TableColumn { ..col });
        }

        let data_end = if options.totals_row {
            CellRef::new(range.end().row() - 1, range.end().col())?
        } else {
            range.end()
        };
        let filter_range = CellRange::new(range.start(), data_end);
        let table = sml::CT_Table {
            id: Some(id),
            name: Some(name.clone()),
            display_name: Some(name.clone()),
            ref_: Some(range.to_string()),
            header_row_count: (!options.header_row).then_some(0),
            totals_row_count: options.totals_row.then_some(1),
            totals_row_shown: (!options.totals_row).then_some(false),
            auto_filter: (options.header_row && options.auto_filter).then(|| {
                Box::new(sml::CT_AutoFilter {
                    ref_: Some(filter_range.to_string()),
                    ..Default::default()
                })
            }),
            table_columns: Some(Box::new(sml::CT_TableColumns {
                count: Some(columns.len() as u32),
                table_column: columns,
                ..Default::default()
            })),
            table_style_info: Some(Box::new(sml::CT_TableStyleInfo {
                name: options.style.clone(),
                show_first_column: Some(options.first_column),
                show_last_column: Some(options.last_column),
                show_row_stripes: Some(options.row_stripes),
                show_column_stripes: Some(options.column_stripes),
                ..Default::default()
            })),
            ..Default::default()
        };
        let part = self.package.next_part_name("/xl/tables/table{}.xml")?;
        let result = info(&table, part.clone()).expect("valid table");
        self.side
            .create(self.package, &part, SideValue::Table(Box::new(table)))?;
        let rid = self
            .package
            .add_relationship(Some(self.part), rel_types::TABLE, &part)?;
        let parts = self.data.table_parts.get_or_insert_with(Box::default);
        parts.table_part.push(sml::CT_TablePart {
            r_id: Some(rid),
            ..Default::default()
        });
        parts.count = Some(parts.table_part.len() as u32);
        Ok(result)
    }

    /// The typed table part of a table of this sheet, for changes the API
    /// does not cover. It is rewritten on save.
    pub fn table_mut(&mut self, name: &str) -> Result<&mut sml::CT_Table> {
        let t = self
            .as_view()
            .table(name)?
            .ok_or_else(|| Error::NotFound(format!("table {name:?}")))?;
        self.side.table(self.package, &t.part)
    }

    /// Removes a table (the cells keep their values). Returns whether it existed.
    pub fn remove_table(&mut self, name: &str) -> Result<bool> {
        let Some(t) = self.as_view().table(name)? else {
            return Ok(false);
        };
        let Some((rid, _)) = sheet_tables(self.data, self.package, self.part)
            .into_iter()
            .find(|(_, p)| *p == t.part)
        else {
            return Ok(false);
        };
        if let Some(parts) = self.data.table_parts.as_mut() {
            parts
                .table_part
                .retain(|p| p.r_id.as_deref() != Some(rid.as_str()));
            parts.count = Some(parts.table_part.len() as u32);
            if parts.table_part.is_empty() {
                self.data.table_parts = None;
            }
        }
        for gone in remove_relationship_and_orphans(self.package, self.part, &rid) {
            self.side.forget(&gone);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_escaping() {
        assert!(valid_name("Sales_2024"));
        assert!(valid_name("_t.1"));
        assert!(!valid_name("A1"));
        assert!(!valid_name("2024"));
        assert!(!valid_name("has space"));
        assert!(!valid_name("R"));
        assert!(!valid_name(""));
        assert_eq!(escape_column("Q[1]#'x"), "Q'[1']'#''x");
        assert_eq!(TotalsRow::Sum.function().unwrap().1, 109);
        assert_eq!(TotalsRow::Var.function().unwrap().1, 110);
        assert!(TotalsRow::Label("x".into()).function().is_none());
    }

    #[test]
    fn table_elements_read_back() {
        let xml = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="3" name="T" displayName="T" ref="B2:C5" totalsRowCount="1"><autoFilter ref="B2:C4"/><tableColumns count="2"><tableColumn id="1" name="a" totalsRowLabel="Total"/><tableColumn id="2" name="b" totalsRowFunction="custom"><totalsRowFormula>SUM(T[b])*2</totalsRowFormula></tableColumn></tableColumns><tableStyleInfo name="TableStyleLight1" showRowStripes="1"/></table>"#;
        let t = sml::elements::TABLE.parse(xml).unwrap();
        let i = info(&t, PartName::new("/xl/tables/table3.xml").unwrap()).unwrap();
        assert_eq!(i.id, 3);
        assert_eq!(i.range.to_string(), "B2:C5");
        assert!(i.header_row && i.totals_row);
        assert_eq!(i.columns[0].totals, Some(TotalsRow::Label("Total".into())));
        assert_eq!(
            i.columns[1].totals,
            Some(TotalsRow::Formula("SUM(T[b])*2".into()))
        );
        assert_eq!(i.auto_filter.unwrap().to_string(), "B2:C4");
        assert_eq!(i.style.as_deref(), Some("TableStyleLight1"));
    }
}
