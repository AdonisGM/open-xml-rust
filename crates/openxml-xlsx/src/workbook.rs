//! The workbook: package handling, sheets, shared tables and saving.

use std::cell::OnceCell;
use std::io::{Cursor, Read, Seek, Write};
use std::ops::ControlFlow;
use std::path::Path;

use openxml_core::part::{read_part, read_related, write_part};
use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{CoreProperties, Package, PartName, w3cdtf_now};
use openxml_schema::{shared_extended_properties as ep, sml};
use openxml_xml::{XmlRead, XmlReader, decode_xml_bytes};

use crate::cell_ref::CellRef;
use crate::date::DateSystem;
use crate::shared_strings::SharedStrings;
use crate::stream::StreamingWorksheet;
use crate::styles::{CellStyle, StyleId, Styles, default_stylesheet};
use crate::value::CellValue;
use crate::worksheet::{
    ReadCtx, SharedFormulas, SheetEntry, SheetKind, Worksheet, WorksheetMut, collect_shared, finalize,
    normalize_row, row_values,
};

/// Name of the application recorded in new documents.
pub const APPLICATION_NAME: &str = "openxml-rust";

/// Relationship type of Excel 4.0 macro sheets.
const XL_MACROSHEET: &str = "http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet";

/// A defined name (named range or formula).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedName {
    /// The name.
    pub name: String,
    /// The formula it stands for, e.g. `Sheet1!$A$1:$B$5`.
    pub formula: String,
    /// Index of the sheet the name is local to (`None` for workbook scope).
    pub local_sheet: Option<u32>,
}

/// An Excel workbook (`.xlsx`).
///
/// The workbook keeps the whole package. The parts it edits — the workbook
/// part, the worksheets that were modified, the shared string table and the
/// stylesheet — are re-serialized on save; every other part (charts,
/// drawings, pivot tables, VBA projects, …) is written back byte for byte.
///
/// ```
/// use openxml_xlsx::{Workbook, CellValue};
///
/// let mut wb = Workbook::new();
/// {
///     let mut sheet = wb.worksheet_mut("Sheet1")?;
///     sheet.set_value("A1", "Hello")?;
///     sheet.set_value("B1", 3.5)?;
/// }
/// let bytes = wb.to_bytes()?;
///
/// let reopened = Workbook::from_bytes(&bytes)?;
/// let sheet = reopened.worksheet("Sheet1")?;
/// assert_eq!(sheet.cell("A1")?, CellValue::Text("Hello".into()));
/// assert_eq!(sheet.cell("B1")?.as_f64(), Some(3.5));
/// # Ok::<(), openxml_core::Error>(())
/// ```
#[derive(Debug)]
pub struct Workbook {
    package: Package,
    workbook_part: PartName,
    workbook: sml::CT_Workbook,
    workbook_dirty: bool,
    pub(crate) sheets: Vec<SheetEntry>,
    pub(crate) sst: SharedStrings,
    sst_part: Option<PartName>,
    pub(crate) styles: Styles,
    styles_part: Option<PartName>,
    pub(crate) date_system: DateSystem,
    pub(crate) needs_recalc: bool,
}

fn part(name: &str) -> PartName {
    PartName::new(name).expect("static part name")
}

/// Checks the rules Excel applies to sheet names.
pub fn validate_sheet_name(name: &str) -> Result<()> {
    let bad = |m: &str| {
        Err(Error::InvalidArgument(format!(
            "invalid sheet name {name:?}: {m}"
        )))
    };
    if name.is_empty() {
        return bad("empty");
    }
    if name.chars().count() > 31 {
        return bad("longer than 31 characters");
    }
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
    {
        return bad(&format!("contains {c:?}"));
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return bad("starts or ends with an apostrophe");
    }
    if name.eq_ignore_ascii_case("History") {
        return bad("reserved by Excel");
    }
    if name.chars().any(char::is_control) {
        return bad("contains control characters");
    }
    Ok(())
}

pub(crate) fn new_worksheet(selected: bool) -> sml::CT_Worksheet {
    sml::CT_Worksheet {
        dimension: Some(Box::new(sml::CT_SheetDimension {
            ref_: Some("A1".into()),
            ..Default::default()
        })),
        sheet_views: Some(Box::new(sml::CT_SheetViews {
            sheet_view: vec![sml::CT_SheetView {
                tab_selected: selected.then_some(true),
                workbook_view_id: Some(0),
                ..Default::default()
            }],
            ..Default::default()
        })),
        sheet_format_pr: Some(Box::new(sml::CT_SheetFormatPr {
            default_row_height: Some(15.0),
            ..Default::default()
        })),
        sheet_data: Some(Box::default()),
        page_margins: Some(Box::new(sml::CT_PageMargins {
            left: Some(0.7),
            right: Some(0.7),
            top: Some(0.75),
            bottom: Some(0.75),
            header: Some(0.3),
            footer: Some(0.3),
            ..Default::default()
        })),
        ..Default::default()
    }
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

impl Workbook {
    /// A new workbook with one empty worksheet named `Sheet1`.
    pub fn new() -> Self {
        let mut package = Package::new();
        let workbook_part = part("/xl/workbook.xml");
        let workbook = sml::CT_Workbook {
            workbook_pr: Some(Box::new(sml::CT_WorkbookPr {
                default_theme_version: None,
                ..Default::default()
            })),
            book_views: Some(Box::new(sml::CT_BookViews {
                workbook_view: vec![sml::CT_BookView {
                    x_window: Some(0),
                    y_window: Some(0),
                    window_width: Some(28_800),
                    window_height: Some(12_300),
                    ..Default::default()
                }],
                ..Default::default()
            })),
            sheets: Some(Box::default()),
            calc_pr: Some(Box::new(sml::CT_CalcPr {
                calc_id: Some(191_029),
                ..Default::default()
            })),
            ..Default::default()
        };
        write_part(
            &mut package,
            &workbook_part,
            ct::SML_WORKBOOK,
            &sml::elements::WORKBOOK,
            &workbook,
        )
        .expect("new workbook part");
        package
            .add_relationship(None, rel_types::OFFICE_DOCUMENT, &workbook_part)
            .expect("package relationship");

        let styles_part = part("/xl/styles.xml");
        let styles = Styles::new_default();
        write_part(
            &mut package,
            &styles_part,
            ct::SML_STYLES,
            &sml::elements::STYLE_SHEET,
            &styles.to_stylesheet(),
        )
        .expect("styles part");
        package
            .add_relationship(Some(&workbook_part), rel_types::STYLES, &styles_part)
            .expect("styles rel");

        let now = w3cdtf_now();
        let core = CoreProperties {
            creator: Some(APPLICATION_NAME.into()),
            created: Some(now.clone()),
            modified: Some(now),
            ..Default::default()
        };
        package.set_core_properties(&core).expect("core properties");
        let app = ep::CT_Properties {
            application: Some(APPLICATION_NAME.into()),
            doc_security: Some(0),
            scale_crop: Some(false),
            links_up_to_date: Some(false),
            shared_doc: Some(false),
            hyperlinks_changed: Some(false),
            ..Default::default()
        };
        let app_part = part("/docProps/app.xml");
        write_part(
            &mut package,
            &app_part,
            ct::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
            &app,
        )
        .expect("app properties");
        package
            .add_relationship(None, rel_types::EXTENDED_PROPERTIES, &app_part)
            .expect("app rel");

        let mut wb = Workbook {
            package,
            workbook_part,
            workbook,
            workbook_dirty: true,
            sheets: Vec::new(),
            sst: SharedStrings::new(),
            sst_part: None,
            styles,
            styles_part: Some(styles_part),
            date_system: DateSystem::V1900,
            needs_recalc: false,
        };
        wb.add_worksheet("Sheet1").expect("first sheet");
        wb
    }

    /// Opens a workbook file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open_path(path)?)
    }

    /// Reads a workbook from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    /// Reads a workbook from a seekable reader.
    pub fn from_reader<R: Read + Seek>(reader: R) -> Result<Self> {
        Self::from_package(Package::open(reader)?)
    }

    /// Interprets a package as a workbook.
    pub fn from_package(package: Package) -> Result<Self> {
        let workbook_part = package
            .main_part()
            .ok_or_else(|| Error::InvalidDocument("the package has no main document relationship".into()))?;
        let content_type = package
            .part(&workbook_part)
            .ok_or_else(|| Error::MissingPart(workbook_part.to_string()))?
            .content_type();
        let is_workbook = (content_type.contains("spreadsheetml") || content_type.contains("ms-excel"))
            && content_type.ends_with("main+xml");
        if !is_workbook {
            return Err(Error::InvalidDocument(format!(
                "the main part is not a workbook ({content_type})"
            )));
        }
        let workbook = read_part(&package, &workbook_part, &sml::elements::WORKBOOK)?;
        let rels = package
            .relationships(Some(&workbook_part))
            .cloned()
            .unwrap_or_default();
        let mut sheets = Vec::new();
        for s in workbook.sheets.iter().flat_map(|s| s.sheet.iter()) {
            let rel_id = s.r_id.clone().unwrap_or_default();
            let rel = rels.get(&rel_id);
            let part = package
                .relationship_target(Some(&workbook_part), &rel_id)
                .filter(|p| package.contains(p));
            let kind = match rel.map(|r| r.rel_type.as_str()) {
                _ if part.is_none() => SheetKind::Other,
                Some(rel_types::WORKSHEET) => SheetKind::Worksheet,
                Some(rel_types::CHARTSHEET) => SheetKind::Chartsheet,
                Some(rel_types::DIALOGSHEET) => SheetKind::Dialogsheet,
                Some(XL_MACROSHEET) | Some(_) | None => SheetKind::Other,
            };
            sheets.push(SheetEntry {
                name: s.name.clone().unwrap_or_default(),
                rel_id,
                part,
                kind,
                data: OnceCell::new(),
                dirty: false,
            });
        }
        let (sst_part, sst) = match read_related(
            &package,
            Some(&workbook_part),
            rel_types::SHARED_STRINGS,
            &sml::elements::SST,
        )? {
            Some((p, sst)) => (Some(p), SharedStrings::from_sst(sst)),
            None => (None, SharedStrings::new()),
        };
        let (styles_part, styles) = match read_related(
            &package,
            Some(&workbook_part),
            rel_types::STYLES,
            &sml::elements::STYLE_SHEET,
        )? {
            Some((p, s)) => (Some(p), Styles::from_stylesheet(s)),
            None => (None, Styles::from_stylesheet(default_stylesheet())),
        };
        let date_system = if workbook.workbook_pr.as_ref().and_then(|p| p.date1904) == Some(true) {
            DateSystem::V1904
        } else {
            DateSystem::V1900
        };
        Ok(Workbook {
            package,
            workbook_part,
            workbook,
            workbook_dirty: false,
            sheets,
            sst,
            sst_part,
            styles,
            styles_part,
            date_system,
            needs_recalc: false,
        })
    }

    // ----- saving ------------------------------------------------------------

    /// Writes the typed parts that changed into the package.
    pub fn flush(&mut self) -> Result<()> {
        for sheet in &mut self.sheets {
            if !sheet.dirty {
                continue;
            }
            let (Some(part), Some(data)) = (sheet.part.as_ref(), sheet.data.get_mut()) else {
                continue;
            };
            finalize(data);
            let content_type = self
                .package
                .part(part)
                .map_or(ct::SML_WORKSHEET, |p| p.content_type())
                .to_owned();
            write_part(
                &mut self.package,
                part,
                &content_type,
                &sml::elements::WORKSHEET,
                data,
            )?;
            sheet.dirty = false;
        }
        if self.sst.is_dirty() && !self.sst.is_empty() {
            let part = match &self.sst_part {
                Some(p) => p.clone(),
                None => {
                    let p = part("/xl/sharedStrings.xml");
                    let p = if self.package.contains(&p) {
                        self.package.next_part_name("/xl/sharedStrings{}.xml")?
                    } else {
                        p
                    };
                    self.package.add_relationship(
                        Some(&self.workbook_part),
                        rel_types::SHARED_STRINGS,
                        &p,
                    )?;
                    self.sst_part = Some(p.clone());
                    p
                }
            };
            write_part(
                &mut self.package,
                &part,
                ct::SML_SHARED_STRINGS,
                &sml::elements::SST,
                &self.sst.to_sst(),
            )?;
            self.sst = SharedStrings::from_sst(self.sst.to_sst());
        }
        if self.styles.is_dirty() {
            let part = match &self.styles_part {
                Some(p) => p.clone(),
                None => {
                    let p = part("/xl/styles.xml");
                    let p = if self.package.contains(&p) {
                        self.package.next_part_name("/xl/styles{}.xml")?
                    } else {
                        p
                    };
                    self.package
                        .add_relationship(Some(&self.workbook_part), rel_types::STYLES, &p)?;
                    self.styles_part = Some(p.clone());
                    p
                }
            };
            let sheet = self.styles.to_stylesheet();
            write_part(
                &mut self.package,
                &part,
                ct::SML_STYLES,
                &sml::elements::STYLE_SHEET,
                &sheet,
            )?;
            self.styles = Styles::from_stylesheet(sheet);
        }
        if self.needs_recalc {
            let calc = self.workbook.calc_pr.get_or_insert_with(Box::default);
            if calc.full_calc_on_load != Some(true) {
                calc.full_calc_on_load = Some(true);
                self.workbook_dirty = true;
            }
            self.needs_recalc = false;
        }
        if self.workbook_dirty {
            let content_type = self
                .package
                .part(&self.workbook_part)
                .map_or(ct::SML_WORKBOOK, |p| p.content_type())
                .to_owned();
            write_part(
                &mut self.package,
                &self.workbook_part,
                &content_type,
                &sml::elements::WORKBOOK,
                &self.workbook,
            )?;
            self.workbook_dirty = false;
        }
        Ok(())
    }

    /// Saves the workbook to a file.
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.flush()?;
        self.package.save_path(path)?;
        Ok(())
    }

    /// Serializes the workbook.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        self.flush()?;
        Ok(self.package.to_bytes()?)
    }

    /// Writes the workbook to a seekable writer and returns it.
    pub fn write_to<W: Write + Seek>(&mut self, writer: W) -> Result<W> {
        self.flush()?;
        Ok(self.package.save(writer)?)
    }

    // ----- package access ----------------------------------------------------

    /// The underlying package.
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The underlying package. Parts the workbook edits (see [`Workbook`]) are
    /// overwritten from the typed model on save if they changed.
    pub fn package_mut(&mut self) -> &mut Package {
        &mut self.package
    }

    /// Consumes the workbook and returns the package with all changes applied.
    pub fn into_package(mut self) -> Result<Package> {
        self.flush()?;
        Ok(self.package)
    }

    /// Name of the workbook part.
    pub fn workbook_part(&self) -> &PartName {
        &self.workbook_part
    }

    /// The typed workbook part.
    pub fn raw_workbook(&self) -> &sml::CT_Workbook {
        &self.workbook
    }

    /// The typed workbook part, for changes the API does not cover. The part
    /// is rewritten on save.
    pub fn raw_workbook_mut(&mut self) -> &mut sml::CT_Workbook {
        self.workbook_dirty = true;
        &mut self.workbook
    }

    /// The typed stylesheet.
    pub fn stylesheet(&self) -> &sml::CT_Stylesheet {
        self.styles.stylesheet()
    }

    /// The typed stylesheet, for changes the API does not cover.
    pub fn stylesheet_mut(&mut self) -> &mut sml::CT_Stylesheet {
        self.styles.stylesheet_mut()
    }

    /// Number of distinct strings in the shared string table.
    pub fn shared_string_count(&self) -> usize {
        self.sst.len()
    }

    /// Core properties (title, author, dates…).
    pub fn core_properties(&self) -> Result<CoreProperties> {
        Ok(self.package.core_properties()?)
    }

    /// Replaces the core properties.
    pub fn set_core_properties(&mut self, props: &CoreProperties) -> Result<()> {
        Ok(self.package.set_core_properties(props)?)
    }

    /// The date system of the workbook.
    pub fn date_system(&self) -> DateSystem {
        self.date_system
    }

    /// Switches the date system. Existing serial numbers are not converted,
    /// so dates already stored shift by 1462 days.
    pub fn set_date_system(&mut self, system: DateSystem) {
        self.date_system = system;
        let pr = self.workbook.workbook_pr.get_or_insert_with(Box::default);
        pr.date1904 = (system == DateSystem::V1904).then_some(true);
        self.workbook_dirty = true;
    }

    // ----- styles ------------------------------------------------------------

    /// Registers a cell style and returns its id (identical records are reused).
    pub fn add_style(&mut self, style: &CellStyle) -> StyleId {
        self.styles.add(style)
    }

    /// The style of a cell format id (best effort reconstruction).
    pub fn cell_style(&self, id: StyleId) -> Option<CellStyle> {
        self.styles.cell_style(id)
    }

    /// Number format id and code of a cell format.
    pub fn number_format_of(&self, id: StyleId) -> (u32, Option<String>) {
        self.styles.number_format_of(id)
    }

    // ----- sheets ------------------------------------------------------------

    /// Names of all sheets in tab order (worksheets, chart sheets, …).
    pub fn sheet_names(&self) -> Vec<String> {
        self.sheets.iter().map(|s| s.name.clone()).collect()
    }

    /// Names of the worksheets in tab order.
    pub fn worksheet_names(&self) -> Vec<String> {
        self.sheets
            .iter()
            .filter(|s| s.kind == SheetKind::Worksheet)
            .map(|s| s.name.clone())
            .collect()
    }

    /// Number of sheets.
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// Kind of a sheet.
    pub fn sheet_kind(&self, name: &str) -> Option<SheetKind> {
        self.sheets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .map(|s| s.kind)
    }

    fn index_of(&self, name: &str) -> Result<usize> {
        self.sheets
            .iter()
            .position(|s| s.name == name)
            .or_else(|| self.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name)))
            .ok_or_else(|| Error::NotFound(format!("sheet {name:?}")))
    }

    fn worksheet_index(&self, name: &str) -> Result<usize> {
        let i = self.index_of(name)?;
        if self.sheets[i].kind != SheetKind::Worksheet {
            return Err(Error::InvalidArgument(format!(
                "sheet {name:?} is not a worksheet"
            )));
        }
        Ok(i)
    }

    fn view(&self, i: usize) -> Result<Worksheet<'_>> {
        let entry = &self.sheets[i];
        let data = entry.load(&self.package)?;
        let ctx = ReadCtx {
            sst: &self.sst,
            styles: &self.styles,
            date_system: self.date_system,
        };
        Ok(Worksheet::new(
            &entry.name,
            entry.part.as_ref().expect("loaded sheets have a part"),
            data,
            ctx,
        ))
    }

    fn view_mut(&mut self, i: usize) -> Result<WorksheetMut<'_>> {
        self.sheets[i].load(&self.package)?;
        let Workbook {
            sheets,
            sst,
            styles,
            date_system,
            needs_recalc,
            ..
        } = self;
        let entry = &mut sheets[i];
        entry.dirty = true;
        let SheetEntry { name, part, data, .. } = entry;
        Ok(WorksheetMut {
            name,
            part: part.as_ref().expect("loaded sheets have a part"),
            data: data.get_mut().expect("loaded above"),
            sst,
            styles,
            date_system: *date_system,
            needs_recalc,
        })
    }

    /// A worksheet by name (exact match first, then case-insensitive).
    pub fn worksheet(&self, name: &str) -> Result<Worksheet<'_>> {
        let i = self.worksheet_index(name)?;
        self.view(i)
    }

    /// An editable worksheet by name. The worksheet is rewritten on save.
    pub fn worksheet_mut(&mut self, name: &str) -> Result<WorksheetMut<'_>> {
        let i = self.worksheet_index(name)?;
        self.view_mut(i)
    }

    /// The worksheet at tab position `index` (0-based).
    pub fn worksheet_at(&self, index: usize) -> Result<Worksheet<'_>> {
        let entry = self
            .sheets
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("sheet #{index}")))?;
        if entry.kind != SheetKind::Worksheet {
            return Err(Error::InvalidArgument(format!(
                "sheet #{index} is not a worksheet"
            )));
        }
        self.view(index)
    }

    /// The editable worksheet at tab position `index` (0-based).
    pub fn worksheet_at_mut(&mut self, index: usize) -> Result<WorksheetMut<'_>> {
        let entry = self
            .sheets
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("sheet #{index}")))?;
        if entry.kind != SheetKind::Worksheet {
            return Err(Error::InvalidArgument(format!(
                "sheet #{index} is not a worksheet"
            )));
        }
        self.view_mut(index)
    }

    pub(crate) fn check_new_name(&self, name: &str, except: Option<usize>) -> Result<()> {
        validate_sheet_name(name)?;
        let taken = self
            .sheets
            .iter()
            .enumerate()
            .any(|(i, s)| Some(i) != except && s.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(Error::InvalidArgument(format!(
                "a sheet named {name:?} already exists"
            )));
        }
        Ok(())
    }

    /// Registers a worksheet part (already in the package) under `name`.
    pub(crate) fn register_sheet(
        &mut self,
        name: &str,
        part: PartName,
        data: Option<sml::CT_Worksheet>,
    ) -> Result<usize> {
        let rel_id = self
            .package
            .add_relationship(Some(&self.workbook_part), rel_types::WORKSHEET, &part)?;
        let sheets = self.workbook.sheets.get_or_insert_with(Box::default);
        let sheet_id = sheets.sheet.iter().filter_map(|s| s.sheet_id).max().unwrap_or(0) + 1;
        sheets.sheet.push(sml::CT_Sheet {
            name: Some(name.to_owned()),
            sheet_id: Some(sheet_id),
            r_id: Some(rel_id.clone()),
            ..Default::default()
        });
        self.workbook_dirty = true;
        let dirty = data.is_some();
        let cell = OnceCell::new();
        if let Some(d) = data {
            let _ = cell.set(d);
        }
        self.sheets.push(SheetEntry {
            name: name.to_owned(),
            rel_id,
            part: Some(part),
            kind: SheetKind::Worksheet,
            data: cell,
            dirty,
        });
        Ok(self.sheets.len() - 1)
    }

    /// Adds an empty worksheet at the end and returns it.
    pub fn add_worksheet(&mut self, name: &str) -> Result<WorksheetMut<'_>> {
        self.check_new_name(name, None)?;
        let part = self.package.next_part_name("/xl/worksheets/sheet{}.xml")?;
        let data = new_worksheet(self.sheets.is_empty());
        write_part(
            &mut self.package,
            &part,
            ct::SML_WORKSHEET,
            &sml::elements::WORKSHEET,
            &data,
        )?;
        let i = self.register_sheet(name, part, Some(data))?;
        self.view_mut(i)
    }

    /// Starts a worksheet written row by row without building it in memory,
    /// for large data sets. Call [`StreamingWorksheet::finish`] to add it.
    pub fn add_streaming_worksheet(&mut self, name: &str) -> Result<StreamingWorksheet<'_>> {
        self.check_new_name(name, None)?;
        Ok(StreamingWorksheet::new(self, name))
    }

    /// Renames a sheet.
    pub fn rename_worksheet(&mut self, old: &str, new: &str) -> Result<()> {
        let i = self.index_of(old)?;
        self.check_new_name(new, Some(i))?;
        self.sheets[i].name = new.to_owned();
        let rel_id = self.sheets[i].rel_id.clone();
        if let Some(s) = self
            .workbook
            .sheets
            .as_mut()
            .and_then(|s| s.sheet.iter_mut().find(|s| s.r_id.as_deref() == Some(&rel_id)))
        {
            s.name = Some(new.to_owned());
        }
        self.workbook_dirty = true;
        Ok(())
    }

    /// Removes a sheet and its part. The last visible sheet cannot be removed.
    pub fn remove_worksheet(&mut self, name: &str) -> Result<()> {
        let i = self.index_of(name)?;
        let visible = |s: &sml::CT_Sheet| s.state.is_none_or(|st| st == sml::ST_SheetState::Visible);
        let sheet_list = self
            .workbook
            .sheets
            .as_ref()
            .map(|s| s.sheet.as_slice())
            .unwrap_or(&[]);
        let rel_id = self.sheets[i].rel_id.clone();
        let removing_visible = sheet_list
            .iter()
            .find(|s| s.r_id.as_deref() == Some(&rel_id))
            .is_some_and(visible);
        let visible_count = sheet_list.iter().filter(|s| visible(s)).count();
        if self.sheets.len() == 1 || (removing_visible && visible_count <= 1) {
            return Err(Error::InvalidArgument(
                "a workbook must keep at least one visible sheet".into(),
            ));
        }
        let entry = self.sheets.remove(i);
        if let Some(sheets) = self.workbook.sheets.as_mut() {
            sheets.sheet.retain(|s| s.r_id.as_deref() != Some(&rel_id));
        }
        if let Some(part) = &entry.part {
            self.package.remove_part(part);
        }
        if let Some(rels) = self.package.relationships_mut(Some(&self.workbook_part)) {
            rels.remove(&rel_id);
        }
        let idx = i as u32;
        if let Some(names) = self.workbook.defined_names.as_mut() {
            names.defined_name.retain(|n| n.local_sheet_id != Some(idx));
            for n in &mut names.defined_name {
                if let Some(id) = n.local_sheet_id.filter(|&id| id > idx) {
                    n.local_sheet_id = Some(id - 1);
                }
            }
        }
        let last = (self.sheets.len() - 1) as u32;
        if let Some(views) = self.workbook.book_views.as_mut() {
            for v in &mut views.workbook_view {
                if let Some(t) = v.active_tab.filter(|&t| t > idx || t > last) {
                    v.active_tab = Some(t.saturating_sub(1).min(last));
                }
                if let Some(f) = v.first_sheet.filter(|&f| f > idx || f > last) {
                    v.first_sheet = Some(f.saturating_sub(1).min(last));
                }
            }
        }
        self.workbook_dirty = true;
        Ok(())
    }

    /// Index of the active (selected) sheet.
    pub fn active_sheet(&self) -> usize {
        self.workbook
            .book_views
            .as_ref()
            .and_then(|v| v.workbook_view.first())
            .and_then(|v| v.active_tab)
            .map_or(0, |t| t as usize)
            .min(self.sheets.len().saturating_sub(1))
    }

    /// Makes a sheet the active one.
    pub fn set_active_sheet(&mut self, index: usize) -> Result<()> {
        if index >= self.sheets.len() {
            return Err(Error::NotFound(format!("sheet #{index}")));
        }
        let views = self.workbook.book_views.get_or_insert_with(Box::default);
        if views.workbook_view.is_empty() {
            views.workbook_view.push(sml::CT_BookView::default());
        }
        views.workbook_view[0].active_tab = (index > 0).then_some(index as u32);
        self.workbook_dirty = true;
        for i in 0..self.sheets.len() {
            if self.sheets[i].kind != SheetKind::Worksheet {
                continue;
            }
            let selected = i == index;
            let currently = self
                .view(i)?
                .raw()
                .sheet_views
                .as_ref()
                .and_then(|v| v.sheet_view.first())
                .and_then(|v| v.tab_selected);
            if currently.unwrap_or(false) != selected {
                let mut ws = self.view_mut(i)?;
                let views = ws.raw_mut().sheet_views.get_or_insert_with(Box::default);
                if views.sheet_view.is_empty() {
                    views.sheet_view.push(sml::CT_SheetView {
                        workbook_view_id: Some(0),
                        ..Default::default()
                    });
                }
                views.sheet_view[0].tab_selected = selected.then_some(true);
            }
        }
        Ok(())
    }

    // ----- defined names -----------------------------------------------------

    /// Defined names of the workbook.
    pub fn defined_names(&self) -> Vec<DefinedName> {
        self.workbook
            .defined_names
            .iter()
            .flat_map(|d| d.defined_name.iter())
            .map(|n| DefinedName {
                name: n.name.clone().unwrap_or_default(),
                formula: n.value.clone(),
                local_sheet: n.local_sheet_id,
            })
            .collect()
    }

    /// Adds or replaces a defined name. `local_sheet` scopes it to a sheet index.
    pub fn set_defined_name(&mut self, name: &str, formula: &str, local_sheet: Option<u32>) -> Result<()> {
        let valid = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\')
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\'))
            && CellRef::parse(name).is_err();
        if !valid {
            return Err(Error::InvalidArgument(format!("invalid defined name {name:?}")));
        }
        if local_sheet.is_some_and(|i| i as usize >= self.sheets.len()) {
            return Err(Error::NotFound(format!("sheet #{}", local_sheet.unwrap_or(0))));
        }
        let formula = formula.strip_prefix('=').unwrap_or(formula).to_owned();
        let names = self.workbook.defined_names.get_or_insert_with(Box::default);
        match names.defined_name.iter_mut().find(|n| {
            n.local_sheet_id == local_sheet && n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name))
        }) {
            Some(n) => n.value = formula,
            None => names.defined_name.push(sml::CT_DefinedName {
                value: formula,
                name: Some(name.to_owned()),
                local_sheet_id: local_sheet,
                ..Default::default()
            }),
        }
        self.workbook_dirty = true;
        Ok(())
    }

    /// Removes a defined name. Returns whether it existed.
    pub fn remove_defined_name(&mut self, name: &str, local_sheet: Option<u32>) -> bool {
        let Some(names) = self.workbook.defined_names.as_mut() else {
            return false;
        };
        let before = names.defined_name.len();
        names.defined_name.retain(|n| {
            !(n.local_sheet_id == local_sheet
                && n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name)))
        });
        let removed = names.defined_name.len() != before;
        if names.defined_name.is_empty() {
            self.workbook.defined_names = None;
        }
        if removed {
            self.workbook_dirty = true;
        }
        removed
    }

    // ----- streaming reading -------------------------------------------------

    /// Visits the rows of a worksheet in order without building the sheet in
    /// memory (only one row is parsed at a time). The callback receives the
    /// row number and the row's non-empty cells, and can stop the scan by
    /// returning [`ControlFlow::Break`].
    ///
    /// Worksheets that were already loaded (or edited) are read from memory.
    pub fn for_each_row(
        &self,
        name: &str,
        mut f: impl FnMut(u32, &[(CellRef, CellValue)]) -> ControlFlow<()>,
    ) -> Result<()> {
        let i = self.worksheet_index(name)?;
        let entry = &self.sheets[i];
        if entry.data.get().is_some() {
            let view = self.view(i)?;
            for row in view.rows() {
                let cells: Vec<_> = row.cells().collect();
                if f(row.index(), &cells).is_break() {
                    break;
                }
            }
            return Ok(());
        }
        let part_name = entry
            .part
            .as_ref()
            .ok_or_else(|| Error::MissingPart(name.to_owned()))?;
        let data = self
            .package
            .part(part_name)
            .ok_or_else(|| Error::MissingPart(part_name.to_string()))?
            .data();
        let xml_err = |source| Error::Xml {
            part: part_name.to_string(),
            source,
        };
        let text = decode_xml_bytes(data).map_err(xml_err)?;
        let mut r = XmlReader::new(&text);
        let root = r.root().map_err(xml_err)?;
        if root.is_empty() {
            return Ok(());
        }
        let ctx = ReadCtx {
            sst: &self.sst,
            styles: &self.styles,
            date_system: self.date_system,
        };
        while let Some(child) = r.next_child().map_err(xml_err)? {
            if child.local() != "sheetData" || child.is_empty() {
                r.skip(&child).map_err(xml_err)?;
                continue;
            }
            let mut shared = SharedFormulas::new();
            let mut next_row = 1u32;
            while let Some(row_tag) = r.next_child().map_err(xml_err)? {
                if row_tag.local() != "row" {
                    r.skip(&row_tag).map_err(xml_err)?;
                    continue;
                }
                let mut row = sml::CT_Row::read_xml(&mut r, &row_tag).map_err(xml_err)?;
                next_row = normalize_row(&mut row, next_row);
                collect_shared(&row, &mut shared);
                let cells = row_values(&row, ctx, &shared);
                if f(row.r.unwrap_or(0), &cells).is_break() {
                    return Ok(());
                }
            }
            break;
        }
        Ok(())
    }
}

impl Workbook {
    /// Serializes to an in-memory buffer (convenience for tests and examples).
    pub fn to_cursor(&mut self) -> Result<Cursor<Vec<u8>>> {
        self.write_to(Cursor::new(Vec::new()))
    }
}
