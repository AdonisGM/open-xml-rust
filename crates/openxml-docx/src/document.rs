//! The [`Document`] type.

use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use openxml_core::part::{read_part, read_related, write_part};
use openxml_core::{Error, Length, Result, sniff_image};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{CoreProperties, Package, PartName, w3cdtf_now};
use openxml_schema::shared_extended_properties as ep;
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_ContentBlockContent};
use openxml_xml::ElementDef;

use crate::numbering::ListKind;
use crate::paragraph::{Paragraph, ParagraphMut};
use crate::run::BreakKind;
use crate::section::{HeaderFooterKind, PageSetup};
use crate::table::{Table, TableMut};
use crate::text::{self, BlockRef};
use crate::{picture, template};

/// Content types accepted for the main document part.
const MAIN_CONTENT_TYPES: &[&str] = &[
    ct::WML_DOCUMENT,
    ct::WML_TEMPLATE,
    ct::WML_DOCUMENT_MACRO,
    ct::WML_TEMPLATE_MACRO,
];

/// A typed XML part and whether it changed since it was read or written.
#[derive(Debug, Clone)]
pub(crate) struct Typed<T> {
    pub name: PartName,
    pub value: T,
    pub dirty: bool,
}

/// State shared by the document and the mutable views it hands out.
#[derive(Debug)]
pub(crate) struct Shared {
    pub package: Package,
    pub main_part: PartName,
    pub styles: Option<Typed<wml::CT_Styles>>,
    pub numbering: Option<Typed<wml::CT_Numbering>>,
    pub next_drawing_id: u32,
    pub bullet_list: Option<i64>,
    pub numbered_list: Option<i64>,
}

impl Shared {
    /// Returns the styles part, creating an empty one if the package has none.
    pub(crate) fn styles_mut(&mut self) -> Result<&mut wml::CT_Styles> {
        if self.styles.is_none() {
            let name = self.package.next_part_name("/word/styles{}.xml")?;
            let name = if self.package.contains(&PartName::new("/word/styles.xml")?) {
                name
            } else {
                PartName::new("/word/styles.xml")?
            };
            self.package.add_part(name.clone(), ct::WML_STYLES, Vec::new())?;
            self.package
                .add_relationship(Some(&self.main_part), rel_types::STYLES, &name)?;
            self.styles = Some(Typed {
                name,
                value: wml::CT_Styles::default(),
                dirty: true,
            });
        }
        let styles = self.styles.as_mut().expect("created above");
        styles.dirty = true;
        Ok(&mut styles.value)
    }

    /// Resolves a style id for use in the document.
    ///
    /// If the id exists it is returned unchanged. For a built-in style that
    /// is missing, a style with the same name (e.g. `heading 1` in a
    /// localized document) is used when present; otherwise the built-in
    /// definition — and the styles it is based on — is added.
    pub(crate) fn resolve_style(&mut self, style_id: &str) -> Result<String> {
        let exists = |s: &Option<Typed<wml::CT_Styles>>, id: &str| {
            s.as_ref()
                .is_some_and(|t| t.value.style.iter().any(|st| st.style_id.as_deref() == Some(id)))
        };
        if exists(&self.styles, style_id) {
            return Ok(style_id.to_owned());
        }
        let Some(mut builtin) = template::builtin_style(style_id) else {
            return Ok(style_id.to_owned());
        };
        let name = builtin
            .name
            .as_ref()
            .and_then(|n| n.val.clone())
            .unwrap_or_default();
        if let Some(styles) = &self.styles
            && let Some(same_name) = styles.value.style.iter().find(|s| {
                s.name
                    .as_ref()
                    .and_then(|n| n.val.as_deref())
                    .is_some_and(|n| n.eq_ignore_ascii_case(&name))
            })
        {
            return Ok(same_name.style_id.clone().unwrap_or_else(|| style_id.to_owned()));
        }
        // Dependencies first, so that references point at existing styles.
        if let Some(based_on) = builtin.based_on.as_mut()
            && let Some(id) = based_on.val.clone()
        {
            based_on.val = Some(self.resolve_default_or_builtin(&id, &builtin.type_)?);
        }
        if let Some(next) = builtin.next.as_mut()
            && let Some(id) = next.val.clone()
        {
            next.val = Some(self.resolve_default_or_builtin(&id, &builtin.type_)?);
        }
        // Only one default style per type: the built-in defaults become ordinary styles.
        if builtin.default.is_some() && self.default_style_id(builtin.type_).is_some() {
            builtin.default = None;
        }
        self.styles_mut()?.style.push(builtin);
        Ok(style_id.to_owned())
    }

    /// For `Normal`/`DefaultParagraphFont`/`TableNormal`, prefer the document's
    /// existing default style of that type.
    fn resolve_default_or_builtin(&mut self, id: &str, ty: &Option<wml::ST_StyleType>) -> Result<String> {
        let is_default_name = matches!(id, "Normal" | "DefaultParagraphFont" | "TableNormal" | "NoList");
        if is_default_name {
            let base_ty = template::builtin_style(id).and_then(|s| s.type_).or(*ty);
            if let Some(existing) = self.default_style_id(base_ty) {
                return Ok(existing);
            }
        }
        self.resolve_style(id)
    }

    fn default_style_id(&self, ty: Option<wml::ST_StyleType>) -> Option<String> {
        let styles = self.styles.as_ref()?;
        styles
            .value
            .style
            .iter()
            .find(|s| {
                s.type_ == ty
                    && s.default.as_ref().is_some_and(|d| {
                        matches!(
                            d,
                            openxml_schema::shared_types::ST_OnOff::Boolean(true)
                                | openxml_schema::shared_types::ST_OnOff::OnOff1(
                                    openxml_schema::shared_types::ST_OnOff1::On
                                )
                        )
                    })
            })
            .and_then(|s| s.style_id.clone())
    }

    /// Adds an external hyperlink relationship from `part`.
    pub(crate) fn add_hyperlink_relationship(&mut self, part: &PartName, url: &str) -> Result<String> {
        Ok(self
            .package
            .add_external_relationship(Some(part), rel_types::HYPERLINK, url)?)
    }

    /// Adds an image part related from `part` and builds its inline drawing.
    pub(crate) fn add_picture(
        &mut self,
        part: &PartName,
        image: &[u8],
        width: Length,
    ) -> Result<wml::CT_Drawing> {
        let info = sniff_image(image).ok_or(Error::UnsupportedImage)?;
        if width.as_emu() <= 0 {
            return Err(Error::InvalidArgument("picture width must be positive".into()));
        }
        let (width, height) = info.size_for_width(width);
        let pattern = format!("/word/media/image{{}}.{}", info.format.extension());
        let name = self.package.next_part_name(&pattern)?;
        self.package
            .add_part(name.clone(), info.format.content_type(), image.to_vec())?;
        let rel_id = self
            .package
            .add_relationship(Some(part), rel_types::IMAGE, &name)?;
        self.next_drawing_id += 1;
        let id = self.next_drawing_id;
        Ok(picture::inline_picture(
            &rel_id,
            id,
            name.file_name(),
            width,
            height,
        ))
    }
}

/// A header or footer part.
#[derive(Debug, Clone)]
pub(crate) struct HeaderFooterPart {
    pub kind: HeaderFooterKind,
    pub part: Typed<wml::CT_HdrFtr>,
}

/// A block-level item of the body: a paragraph or a table.
#[derive(Clone, Copy, Debug)]
pub enum Block<'a> {
    /// A paragraph.
    Paragraph(Paragraph<'a>),
    /// A table.
    Table(Table<'a>),
}

/// A WordprocessingML document (`.docx`).
///
/// The document keeps the whole package in memory. The parts it manages
/// (main document, styles, numbering, headers and footers) are held as typed
/// schema objects and written back on [`Document::save`] only if they
/// changed; every other part is saved unchanged.
///
/// ```
/// use openxml_docx::Document;
///
/// let mut doc = Document::new();
/// doc.add_heading("Report", 1)?;
/// doc.add_paragraph("Hello, world!");
/// let bytes = doc.to_bytes()?;
///
/// let reopened = Document::from_bytes(&bytes)?;
/// assert_eq!(reopened.text(), "Report\nHello, world!");
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Debug)]
pub struct Document {
    pub(crate) shared: Shared,
    pub(crate) main: wml::CT_Document,
    pub(crate) main_dirty: bool,
    pub(crate) headers: Vec<HeaderFooterPart>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

fn blank_body() -> wml::CT_Body {
    let mut body = wml::CT_Body::default();
    crate::section::apply_page_setup(
        body.sect_pr.get_or_insert_with(Default::default),
        &PageSetup::a4(),
    );
    body
}

impl Document {
    /// Creates an empty A4 document with the standard styles, settings,
    /// font table and document properties.
    pub fn new() -> Self {
        let build = || -> Result<Document> {
            let mut package = Package::new();
            let main_part = PartName::new("/word/document.xml")?;
            package.add_part(main_part.clone(), ct::WML_DOCUMENT, Vec::new())?;
            package.add_relationship(None, rel_types::OFFICE_DOCUMENT, &main_part)?;

            let styles_part = PartName::new("/word/styles.xml")?;
            package.add_part(styles_part.clone(), ct::WML_STYLES, Vec::new())?;
            package.add_relationship(Some(&main_part), rel_types::STYLES, &styles_part)?;

            let settings = PartName::new("/word/settings.xml")?;
            write_part(
                &mut package,
                &settings,
                ct::WML_SETTINGS,
                &wml::elements::SETTINGS,
                &template::default_settings(),
            )?;
            package.add_relationship(Some(&main_part), rel_types::SETTINGS, &settings)?;

            let fonts = PartName::new("/word/fontTable.xml")?;
            write_part(
                &mut package,
                &fonts,
                ct::WML_FONT_TABLE,
                &wml::elements::FONTS,
                &template::default_font_table(),
            )?;
            package.add_relationship(Some(&main_part), rel_types::FONT_TABLE, &fonts)?;

            let app = PartName::new("/docProps/app.xml")?;
            write_part(
                &mut package,
                &app,
                ct::EXTENDED_PROPERTIES,
                &ep::elements::PROPERTIES,
                &template::default_app_properties(),
            )?;
            package.add_relationship(None, rel_types::EXTENDED_PROPERTIES, &app)?;

            let now = w3cdtf_now();
            package.set_core_properties(&CoreProperties {
                created: Some(now.clone()),
                modified: Some(now),
                revision: Some("1".into()),
                ..Default::default()
            })?;

            let main = wml::CT_Document {
                body: Some(Box::new(blank_body())),
                ..Default::default()
            };
            Ok(Document {
                shared: Shared {
                    package,
                    main_part,
                    styles: Some(Typed {
                        name: styles_part,
                        value: template::default_styles(),
                        dirty: true,
                    }),
                    numbering: None,
                    next_drawing_id: 0,
                    bullet_list: None,
                    numbered_list: None,
                },
                main,
                main_dirty: true,
                headers: Vec::new(),
            })
        };
        build().expect("the built-in document template is valid")
    }

    /// Reads a document from a package.
    pub fn from_package(package: Package) -> Result<Self> {
        let main_part = package
            .main_part()
            .ok_or_else(|| Error::InvalidDocument("the package has no officeDocument relationship".into()))?;
        let content_type = package
            .part(&main_part)
            .ok_or_else(|| Error::MissingPart(main_part.to_string()))?
            .content_type();
        if !MAIN_CONTENT_TYPES.contains(&content_type) {
            return Err(Error::InvalidDocument(format!(
                "the main part has content type {content_type}, which is not a WordprocessingML document"
            )));
        }
        let main = read_part(&package, &main_part, &wml::elements::DOCUMENT)?;
        let styles = read_related(
            &package,
            Some(&main_part),
            rel_types::STYLES,
            &wml::elements::STYLES,
        )?
        .map(|(name, value)| Typed {
            name,
            value,
            dirty: false,
        });
        let numbering = read_related(
            &package,
            Some(&main_part),
            rel_types::NUMBERING,
            &wml::elements::NUMBERING,
        )?
        .map(|(name, value)| Typed {
            name,
            value,
            dirty: false,
        });
        let mut headers = Vec::new();
        let mut max_id = 0;
        for (kind, rel_type, def) in [
            (HeaderFooterKind::Header, rel_types::HEADER, &wml::elements::HDR),
            (HeaderFooterKind::Footer, rel_types::FOOTER, &wml::elements::FTR),
        ] {
            for name in package.related_parts(Some(&main_part), rel_type) {
                if headers.iter().any(|h: &HeaderFooterPart| h.part.name == name) || !package.contains(&name)
                {
                    continue;
                }
                let value = read_part(&package, &name, def)?;
                headers.push(HeaderFooterPart {
                    kind,
                    part: Typed {
                        name,
                        value,
                        dirty: false,
                    },
                });
            }
        }
        for (_, part) in package.parts() {
            if part
                .content_type()
                .starts_with("application/vnd.openxmlformats-officedocument.wordprocessingml.")
            {
                max_id = max_id.max(picture::max_drawing_id(part.data()));
            }
        }
        Ok(Document {
            shared: Shared {
                package,
                main_part,
                styles,
                numbering,
                next_drawing_id: max_id,
                bullet_list: None,
                numbered_list: None,
            },
            main,
            main_dirty: false,
            headers,
        })
    }

    /// Reads a document from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    /// Reads a document from a seekable reader.
    pub fn from_reader<R: Read + Seek>(reader: R) -> Result<Self> {
        Self::from_package(Package::open(reader)?)
    }

    /// Opens a `.docx` file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open_path(path)?)
    }

    /// Writes the typed parts that changed back into the package.
    ///
    /// Called automatically by the save methods; call it before inspecting
    /// [`Document::package`] if the document was modified.
    pub fn flush(&mut self) -> Result<()> {
        let pkg = &mut self.shared.package;
        if self.main_dirty {
            let ty = pkg
                .part(&self.shared.main_part)
                .map(|p| p.content_type().to_owned())
                .unwrap_or(ct::WML_DOCUMENT.into());
            write_part(
                pkg,
                &self.shared.main_part,
                &ty,
                &wml::elements::DOCUMENT,
                &self.main,
            )?;
            self.main_dirty = false;
        }
        flush_typed(
            pkg,
            &mut self.shared.styles,
            ct::WML_STYLES,
            &wml::elements::STYLES,
        )?;
        flush_typed(
            pkg,
            &mut self.shared.numbering,
            ct::WML_NUMBERING,
            &wml::elements::NUMBERING,
        )?;
        for h in &mut self.headers {
            if h.part.dirty {
                let (ty, def) = match h.kind {
                    HeaderFooterKind::Header => (ct::WML_HEADER, &wml::elements::HDR),
                    HeaderFooterKind::Footer => (ct::WML_FOOTER, &wml::elements::FTR),
                };
                write_part(pkg, &h.part.name, ty, def, &h.part.value)?;
                h.part.dirty = false;
            }
        }
        Ok(())
    }

    /// Marks every typed part (main document, styles, numbering, headers and
    /// footers) as modified, so that the next save re-serializes them in
    /// canonical schema order.
    pub fn normalize(&mut self) {
        self.main_dirty = true;
        if let Some(s) = self.shared.styles.as_mut() {
            s.dirty = true;
        }
        if let Some(n) = self.shared.numbering.as_mut() {
            n.dirty = true;
        }
        for h in &mut self.headers {
            h.part.dirty = true;
        }
    }

    /// Serializes the document into a writer.
    pub fn write_to<W: Write + Seek>(&mut self, writer: W) -> Result<W> {
        self.flush()?;
        Ok(self.shared.package.save(writer)?)
    }

    /// Serializes the document to bytes.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        Ok(self.write_to(Cursor::new(Vec::new()))?.into_inner())
    }

    /// Saves the document to a file.
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.flush()?;
        Ok(self.shared.package.save_path(path)?)
    }

    /// Consumes the document and returns its package (with all changes flushed).
    pub fn into_package(mut self) -> Result<Package> {
        self.flush()?;
        Ok(self.shared.package)
    }

    // ----- raw access ----------------------------------------------------------------

    /// The package. Typed parts reflect changes only after [`Document::flush`].
    pub fn package(&self) -> &Package {
        &self.shared.package
    }

    /// Mutable package. Parts the document manages are overwritten on save
    /// if they were modified through the document.
    pub fn package_mut(&mut self) -> &mut Package {
        &mut self.shared.package
    }

    /// Name of the main document part.
    pub fn main_part_name(&self) -> &PartName {
        &self.shared.main_part
    }

    /// The main document part (`w:document`).
    pub fn document(&self) -> &wml::CT_Document {
        &self.main
    }

    /// Mutable main document part.
    pub fn document_mut(&mut self) -> &mut wml::CT_Document {
        self.main_dirty = true;
        &mut self.main
    }

    /// The document body.
    pub fn body(&self) -> &wml::CT_Body {
        static EMPTY: std::sync::OnceLock<wml::CT_Body> = std::sync::OnceLock::new();
        self.main
            .body
            .as_deref()
            .unwrap_or_else(|| EMPTY.get_or_init(Default::default))
    }

    /// Mutable document body.
    pub fn body_mut(&mut self) -> &mut wml::CT_Body {
        self.main_dirty = true;
        self.main.body.get_or_insert_with(Default::default)
    }

    /// The styles part, if the document has one.
    pub fn styles(&self) -> Option<&wml::CT_Styles> {
        self.shared.styles.as_ref().map(|s| &s.value)
    }

    /// Mutable styles part (created if missing).
    pub fn styles_mut(&mut self) -> Result<&mut wml::CT_Styles> {
        self.shared.styles_mut()
    }

    /// The numbering part, if the document has one.
    pub fn numbering(&self) -> Option<&wml::CT_Numbering> {
        self.shared.numbering.as_ref().map(|s| &s.value)
    }

    /// Ids of the styles defined in the styles part.
    pub fn style_ids(&self) -> Vec<&str> {
        self.styles()
            .map(|s| s.style.iter().filter_map(|st| st.style_id.as_deref()).collect())
            .unwrap_or_default()
    }

    // ----- properties ---------------------------------------------------------------

    /// Core properties (title, author, dates, …).
    pub fn core_properties(&self) -> Result<CoreProperties> {
        Ok(self.shared.package.core_properties()?)
    }

    /// Replaces the core properties.
    pub fn set_core_properties(&mut self, props: &CoreProperties) -> Result<()> {
        Ok(self.shared.package.set_core_properties(props)?)
    }

    /// Target URL of an external hyperlink relationship of the main part.
    pub fn hyperlink_target(&self, relationship_id: &str) -> Option<&str> {
        let rel = self
            .shared
            .package
            .relationships(Some(&self.shared.main_part))?
            .get(relationship_id)?;
        rel.is_external().then_some(rel.target.as_str())
    }

    // ----- reading ------------------------------------------------------------------

    /// Paragraphs and tables of the body in document order (content controls
    /// and custom XML are looked through).
    pub fn blocks(&self) -> Vec<Block<'_>> {
        text::blocks(&self.body().block_level_elts)
            .into_iter()
            .map(|b| match b {
                BlockRef::P(p) => Block::Paragraph(Paragraph::new(p)),
                BlockRef::Tbl(t) => Block::Table(Table::new(t)),
            })
            .collect()
    }

    /// Paragraphs of the body (not those inside tables).
    pub fn paragraphs(&self) -> Vec<Paragraph<'_>> {
        self.blocks()
            .into_iter()
            .filter_map(|b| {
                if let Block::Paragraph(p) = b {
                    Some(p)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Tables of the body (not nested tables).
    pub fn tables(&self) -> Vec<Table<'_>> {
        self.blocks()
            .into_iter()
            .filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None })
            .collect()
    }

    /// Plain text of the body: paragraphs separated by newlines, table cells
    /// by tabs and table rows by newlines.
    pub fn text(&self) -> String {
        text::blocks_text(&text::blocks(&self.body().block_level_elts))
    }

    // ----- editing ------------------------------------------------------------------

    fn push_block(&mut self, block: EG_BlockLevelElts) -> &mut EG_BlockLevelElts {
        let body = self.body_mut();
        body.block_level_elts.push(block);
        body.block_level_elts.last_mut().expect("just pushed")
    }

    fn paragraph_handle<'a>(shared: &'a mut Shared, p: &'a mut wml::CT_P) -> ParagraphMut<'a> {
        let part = shared.main_part.clone();
        ParagraphMut::new(p, shared, part)
    }

    /// Appends a paragraph containing `text` (a single run; may be empty).
    pub fn add_paragraph(&mut self, text: &str) -> ParagraphMut<'_> {
        self.push_block(EG_BlockLevelElts::P(Box::default()));
        let Some(EG_BlockLevelElts::P(p)) = self
            .main
            .body
            .as_mut()
            .and_then(|b| b.block_level_elts.last_mut())
        else {
            unreachable!("a paragraph was just appended")
        };
        let mut handle = Self::paragraph_handle(&mut self.shared, p);
        if !text.is_empty() {
            handle.add_text(text);
        }
        handle
    }

    /// Appends a heading: level 0 is the document title (`Title` style),
    /// levels 1–6 use `Heading1`…`Heading6`.
    pub fn add_heading(&mut self, text: &str, level: u8) -> Result<ParagraphMut<'_>> {
        let style = match level {
            0 => "Title".to_owned(),
            1..=6 => format!("Heading{level}"),
            _ => {
                return Err(Error::InvalidArgument(format!(
                    "heading level {level} is not in 0..=6"
                )));
            }
        };
        let mut p = self.add_paragraph(text);
        p.set_style(&style)?;
        Ok(p)
    }

    /// Appends a paragraph containing a page break.
    pub fn add_page_break(&mut self) -> ParagraphMut<'_> {
        let mut p = self.add_paragraph("");
        p.add_break(BreakKind::Page);
        p
    }

    /// Appends a paragraph with an inline picture scaled to `width`.
    pub fn add_picture(&mut self, image: &[u8], width: Length) -> Result<ParagraphMut<'_>> {
        let part = self.shared.main_part.clone();
        let drawing = self.shared.add_picture(&part, image, width)?;
        let mut p = self.add_paragraph("");
        p.raw().p_content.push(wml::EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![wml::EG_RunInnerContent::Drawing(Box::new(drawing))],
            ..Default::default()
        })));
        Ok(p)
    }

    /// Appends a list item. Consecutive items of the same kind form one list;
    /// see [`Document::restart_list`] to start a new numbered list.
    pub fn add_list_item(&mut self, text: &str, kind: ListKind, level: u8) -> Result<ParagraphMut<'_>> {
        if level > 8 {
            return Err(Error::InvalidArgument(format!(
                "list level {level} is not in 0..=8"
            )));
        }
        let num_id = self.shared.list_num_id(kind)?;
        let mut p = self.add_paragraph(text);
        p.set_style("ListParagraph")?;
        p.set_numbering(num_id, level);
        Ok(p)
    }

    /// Makes the next list item of `kind` start a new list (numbering restarts at 1).
    pub fn restart_list(&mut self, kind: ListKind) -> Result<()> {
        self.shared.restart_list(kind)
    }

    /// Appends a table with `rows` × `cols` empty cells in the `TableGrid`
    /// style, with columns filling the text width.
    pub fn add_table(&mut self, rows: usize, cols: usize) -> Result<TableMut<'_>> {
        if rows == 0 || cols == 0 {
            return Err(Error::InvalidArgument(
                "a table needs at least one row and one column".into(),
            ));
        }
        let style = self.shared.resolve_style("TableGrid")?;
        let setup = self.page_setup();
        let text_width = setup.width - setup.margins.left - setup.margins.right;
        let table = crate::table::new_table(rows, cols, &style, text_width);
        self.push_block(EG_BlockLevelElts::Tbl(Box::new(table)));
        let Some(EG_BlockLevelElts::Tbl(t)) = self
            .main
            .body
            .as_mut()
            .and_then(|b| b.block_level_elts.last_mut())
        else {
            unreachable!("a table was just appended")
        };
        let part = self.shared.main_part.clone();
        Ok(TableMut::new(t, &mut self.shared, part))
    }

    /// Mutable access to the `index`-th paragraph (in [`Document::paragraphs`] order).
    pub fn paragraph_mut(&mut self, index: usize) -> Option<ParagraphMut<'_>> {
        self.main_dirty = true;
        let body = self.main.body.as_mut()?;
        let mut n = index;
        let p = nth_paragraph(&mut body.block_level_elts, &mut n)?;
        let part = self.shared.main_part.clone();
        Some(ParagraphMut::new(p, &mut self.shared, part))
    }

    /// Mutable access to the `index`-th table (in [`Document::tables`] order).
    pub fn table_mut(&mut self, index: usize) -> Option<TableMut<'_>> {
        self.main_dirty = true;
        let body = self.main.body.as_mut()?;
        let mut n = index;
        let t = nth_table(&mut body.block_level_elts, &mut n)?;
        let part = self.shared.main_part.clone();
        Some(TableMut::new(t, &mut self.shared, part))
    }

    /// Removes the `index`-th paragraph (in [`Document::paragraphs`] order).
    pub fn remove_paragraph(&mut self, index: usize) -> Result<()> {
        let count = self.paragraphs().len();
        let body = self.body_mut();
        let mut n = index;
        if remove_nth_paragraph(&mut body.block_level_elts, &mut n) {
            Ok(())
        } else {
            Err(Error::NotFound(format!(
                "paragraph {index} (the body has {count})"
            )))
        }
    }

    /// Replaces every occurrence of `from` with `to` in the body (including
    /// tables), headers and footers. Occurrences split across runs of a
    /// paragraph are found too; the replacement takes the formatting of the
    /// run where the occurrence starts. Returns the number of replacements.
    pub fn replace_text(&mut self, from: &str, to: &str) -> usize {
        let mut count = 0;
        if let Some(body) = self.main.body.as_mut() {
            count += replace_in_blocks(&mut body.block_level_elts, from, to);
        }
        if count > 0 {
            self.main_dirty = true;
        }
        for h in &mut self.headers {
            let n = replace_in_blocks(&mut h.part.value.block_level_elts, from, to);
            if n > 0 {
                h.part.dirty = true;
            }
            count += n;
        }
        count
    }
}

fn flush_typed<T: openxml_xml::XmlWrite>(
    pkg: &mut Package,
    part: &mut Option<Typed<T>>,
    content_type: &str,
    def: &ElementDef<T>,
) -> Result<()> {
    if let Some(t) = part
        && t.dirty
    {
        let ty = pkg
            .part(&t.name)
            .map(|p| p.content_type().to_owned())
            .unwrap_or(content_type.into());
        write_part(pkg, &t.name, &ty, def, &t.value)?;
        t.dirty = false;
    }
    Ok(())
}

fn nth_paragraph_in_content<'a>(
    items: &'a mut [EG_ContentBlockContent],
    n: &mut usize,
) -> Option<&'a mut wml::CT_P> {
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => {
                if *n == 0 {
                    return Some(p);
                }
                *n -= 1;
            }
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && let Some(p) = nth_paragraph_in_content(&mut c.content_block_content, n)
                {
                    return Some(p);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => {
                if let Some(p) = nth_paragraph_in_content(&mut x.content_block_content, n) {
                    return Some(p);
                }
            }
            _ => {}
        }
    }
    None
}

fn nth_paragraph<'a>(items: &'a mut [EG_BlockLevelElts], n: &mut usize) -> Option<&'a mut wml::CT_P> {
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => {
                if *n == 0 {
                    return Some(p);
                }
                *n -= 1;
            }
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && let Some(p) = nth_paragraph_in_content(&mut c.content_block_content, n)
                {
                    return Some(p);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => {
                if let Some(p) = nth_paragraph_in_content(&mut x.content_block_content, n) {
                    return Some(p);
                }
            }
            _ => {}
        }
    }
    None
}

fn nth_table_in_content<'a>(
    items: &'a mut [EG_ContentBlockContent],
    n: &mut usize,
) -> Option<&'a mut wml::CT_Tbl> {
    for item in items {
        match item {
            EG_ContentBlockContent::Tbl(t) => {
                if *n == 0 {
                    return Some(t);
                }
                *n -= 1;
            }
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && let Some(t) = nth_table_in_content(&mut c.content_block_content, n)
                {
                    return Some(t);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => {
                if let Some(t) = nth_table_in_content(&mut x.content_block_content, n) {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

fn nth_table<'a>(items: &'a mut [EG_BlockLevelElts], n: &mut usize) -> Option<&'a mut wml::CT_Tbl> {
    for item in items {
        match item {
            EG_BlockLevelElts::Tbl(t) => {
                if *n == 0 {
                    return Some(t);
                }
                *n -= 1;
            }
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && let Some(t) = nth_table_in_content(&mut c.content_block_content, n)
                {
                    return Some(t);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => {
                if let Some(t) = nth_table_in_content(&mut x.content_block_content, n) {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

fn remove_nth_in_content(items: &mut Vec<EG_ContentBlockContent>, n: &mut usize) -> bool {
    let mut i = 0;
    while i < items.len() {
        match &mut items[i] {
            EG_ContentBlockContent::P(_) => {
                if *n == 0 {
                    items.remove(i);
                    return true;
                }
                *n -= 1;
            }
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && remove_nth_in_content(&mut c.content_block_content, n)
                {
                    return true;
                }
            }
            EG_ContentBlockContent::CustomXml(x) => {
                if remove_nth_in_content(&mut x.content_block_content, n) {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn remove_nth_paragraph(items: &mut Vec<EG_BlockLevelElts>, n: &mut usize) -> bool {
    let mut i = 0;
    while i < items.len() {
        match &mut items[i] {
            EG_BlockLevelElts::P(_) => {
                if *n == 0 {
                    items.remove(i);
                    return true;
                }
                *n -= 1;
            }
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut()
                    && remove_nth_in_content(&mut c.content_block_content, n)
                {
                    return true;
                }
            }
            EG_BlockLevelElts::CustomXml(x) => {
                if remove_nth_in_content(&mut x.content_block_content, n) {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn replace_in_content(items: &mut [EG_ContentBlockContent], from: &str, to: &str) -> usize {
    let mut count = 0;
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => count += crate::paragraph::replace_in_paragraph(p, from, to),
            EG_ContentBlockContent::Tbl(t) => count += replace_in_table(t, from, to),
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut() {
                    count += replace_in_content(&mut c.content_block_content, from, to);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => {
                count += replace_in_content(&mut x.content_block_content, from, to)
            }
            _ => {}
        }
    }
    count
}

fn replace_in_table(t: &mut wml::CT_Tbl, from: &str, to: &str) -> usize {
    let mut count = 0;
    for row in text::rows_mut(t) {
        for cell in text::cells_mut(row) {
            count += replace_in_blocks(&mut cell.block_level_elts, from, to);
        }
    }
    count
}

/// Replaces text in every paragraph of a block container (recursing into tables).
pub(crate) fn replace_in_blocks(items: &mut [EG_BlockLevelElts], from: &str, to: &str) -> usize {
    let mut count = 0;
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => count += crate::paragraph::replace_in_paragraph(p, from, to),
            EG_BlockLevelElts::Tbl(t) => count += replace_in_table(t, from, to),
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut() {
                    count += replace_in_content(&mut c.content_block_content, from, to);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => {
                count += replace_in_content(&mut x.content_block_content, from, to)
            }
            _ => {}
        }
    }
    count
}
