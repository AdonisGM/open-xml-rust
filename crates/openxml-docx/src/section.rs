//! Sections: page setup, breaks, columns, numbering, borders, headers and footers.

use openxml_core::{Error, Length, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::wml::{
    self, EG_HdrFtrReferences, ST_HdrFtr, ST_LineNumberRestart, ST_NumberFormat as NumberFormat,
    ST_PageOrientation, ST_SectionMark,
};

use crate::document::{Document, HeaderFooterPart, Typed};
use crate::format::{Border, convert};
use crate::paragraph::{Paragraph, ParagraphMut};
use crate::text;
use crate::util::{is_on, off, on, signed_twips, signed_twips_value, twips, twips_value};

/// Page orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Portrait (taller than wide).
    Portrait,
    /// Landscape (wider than tall).
    Landscape,
}

/// Page margins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margins {
    /// Top margin.
    pub top: Length,
    /// Right margin.
    pub right: Length,
    /// Bottom margin.
    pub bottom: Length,
    /// Left margin.
    pub left: Length,
    /// Distance from the top edge to the header.
    pub header: Length,
    /// Distance from the bottom edge to the footer.
    pub footer: Length,
    /// Binding gutter.
    pub gutter: Length,
}

impl Default for Margins {
    /// One-inch margins, half-inch header and footer distances.
    fn default() -> Self {
        Margins {
            top: Length::inches(1.0),
            right: Length::inches(1.0),
            bottom: Length::inches(1.0),
            left: Length::inches(1.0),
            header: Length::inches(0.5),
            footer: Length::inches(0.5),
            gutter: Length::ZERO,
        }
    }
}

/// Size, orientation and margins of the pages of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSetup {
    /// Page width.
    pub width: Length,
    /// Page height.
    pub height: Length,
    /// Orientation.
    pub orientation: Orientation,
    /// Margins.
    pub margins: Margins,
}

impl PageSetup {
    /// A4 portrait (210 × 297 mm) with default margins.
    pub fn a4() -> Self {
        PageSetup {
            width: Length::twips(11906),
            height: Length::twips(16838),
            orientation: Orientation::Portrait,
            margins: Margins::default(),
        }
    }

    /// US Letter portrait (8.5 × 11 in) with default margins.
    pub fn letter() -> Self {
        PageSetup {
            width: Length::twips(12240),
            height: Length::twips(15840),
            orientation: Orientation::Portrait,
            margins: Margins::default(),
        }
    }

    /// The same paper turned to landscape (width and height swapped).
    pub fn landscape(self) -> Self {
        let (w, h) = if self.width < self.height {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        PageSetup {
            width: w,
            height: h,
            orientation: Orientation::Landscape,
            ..self
        }
    }
}

/// Writes page size and margins into section properties.
pub(crate) fn apply_page_setup(sect: &mut wml::CT_SectPr, setup: &PageSetup) {
    sect.pg_sz = Some(Box::new(wml::CT_PageSz {
        w: Some(twips(setup.width)),
        h: Some(twips(setup.height)),
        orient: (setup.orientation == Orientation::Landscape).then_some(ST_PageOrientation::Landscape),
        ..Default::default()
    }));
    let m = &setup.margins;
    sect.pg_mar = Some(Box::new(wml::CT_PageMar {
        top: Some(signed_twips(m.top)),
        right: Some(twips(m.right)),
        bottom: Some(signed_twips(m.bottom)),
        left: Some(twips(m.left)),
        header: Some(twips(m.header)),
        footer: Some(twips(m.footer)),
        gutter: Some(twips(m.gutter)),
        ..Default::default()
    }));
    if sect.cols.is_none() {
        sect.cols = Some(Box::new(wml::CT_Columns {
            space: Some(twips(Length::twips(720))),
            ..Default::default()
        }));
    }
    if sect.doc_grid.is_none() {
        sect.doc_grid = Some(Box::new(wml::CT_DocGrid {
            line_pitch: Some(360),
            ..Default::default()
        }));
    }
}

/// Reads page size and margins (missing values fall back to Letter/default margins,
/// as Word does).
pub(crate) fn read_page_setup(sect: Option<&wml::CT_SectPr>) -> PageSetup {
    let mut setup = PageSetup::letter();
    let Some(sect) = sect else { return setup };
    if let Some(sz) = sect.pg_sz.as_deref() {
        if let Some(w) = sz.w.as_ref().and_then(twips_value) {
            setup.width = w;
        }
        if let Some(h) = sz.h.as_ref().and_then(twips_value) {
            setup.height = h;
        }
        if sz.orient == Some(ST_PageOrientation::Landscape) {
            setup.orientation = Orientation::Landscape;
        }
    }
    if let Some(mar) = sect.pg_mar.as_deref() {
        let m = &mut setup.margins;
        let pos = |v: &Option<openxml_schema::shared_types::ST_TwipsMeasure>, d: Length| {
            v.as_ref().and_then(twips_value).unwrap_or(d)
        };
        let signed = |v: &Option<wml::ST_SignedTwipsMeasure>, d: Length| {
            v.as_ref().and_then(signed_twips_value).unwrap_or(d)
        };
        m.top = signed(&mar.top, m.top);
        m.bottom = signed(&mar.bottom, m.bottom);
        m.left = pos(&mar.left, m.left);
        m.right = pos(&mar.right, m.right);
        m.header = pos(&mar.header, m.header);
        m.footer = pos(&mar.footer, m.footer);
        m.gutter = pos(&mar.gutter, m.gutter);
    }
    setup
}

/// Header or footer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderFooterKind {
    /// A header.
    Header,
    /// A footer.
    Footer,
}

/// Read-only view of a header or footer part.
#[derive(Clone, Copy, Debug)]
pub struct HeaderFooter<'a> {
    kind: HeaderFooterKind,
    name: &'a openxml_opc::PartName,
    content: &'a wml::CT_HdrFtr,
}

impl<'a> HeaderFooter<'a> {
    /// Header or footer.
    pub fn kind(&self) -> HeaderFooterKind {
        self.kind
    }

    /// Name of the part.
    pub fn part_name(&self) -> &'a openxml_opc::PartName {
        self.name
    }

    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_HdrFtr {
        self.content
    }

    /// Paragraphs (outside tables).
    pub fn paragraphs(&self) -> Vec<Paragraph<'a>> {
        text::blocks(&self.content.block_level_elts)
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

    /// Plain text.
    pub fn text(&self) -> String {
        text::blocks_text(&text::blocks(&self.content.block_level_elts))
    }
}

/// How a section starts relative to the previous one (`w:type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionBreak {
    /// On the next page.
    NextPage,
    /// On the same page.
    Continuous,
    /// On the next even-numbered page.
    EvenPage,
    /// On the next odd-numbered page.
    OddPage,
    /// In the next column.
    NextColumn,
}

impl SectionBreak {
    fn to_mark(self) -> ST_SectionMark {
        match self {
            SectionBreak::NextPage => ST_SectionMark::NextPage,
            SectionBreak::Continuous => ST_SectionMark::Continuous,
            SectionBreak::EvenPage => ST_SectionMark::EvenPage,
            SectionBreak::OddPage => ST_SectionMark::OddPage,
            SectionBreak::NextColumn => ST_SectionMark::NextColumn,
        }
    }

    fn from_mark(m: ST_SectionMark) -> Self {
        match m {
            ST_SectionMark::NextPage => SectionBreak::NextPage,
            ST_SectionMark::Continuous => SectionBreak::Continuous,
            ST_SectionMark::EvenPage => SectionBreak::EvenPage,
            ST_SectionMark::OddPage => SectionBreak::OddPage,
            ST_SectionMark::NextColumn => SectionBreak::NextColumn,
        }
    }
}

/// Pages a header or footer is used on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderFooterType {
    /// Every page not covered by the other types.
    Default,
    /// The first page of the section (enables the section's title page).
    First,
    /// Even pages (enables different odd and even headers in the settings).
    Even,
}

impl HeaderFooterType {
    fn to_st(self) -> ST_HdrFtr {
        match self {
            HeaderFooterType::Default => ST_HdrFtr::Default,
            HeaderFooterType::First => ST_HdrFtr::First,
            HeaderFooterType::Even => ST_HdrFtr::Even,
        }
    }
}

/// Newspaper columns of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    /// Number of equal-width columns.
    pub count: u32,
    /// Space between columns.
    pub spacing: Length,
    /// Draw a line between columns.
    pub separator: bool,
}

/// Page numbering of a section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageNumbering {
    /// Number format (decimal when `None`).
    pub format: Option<NumberFormat>,
    /// Restart numbering at this value; continue from the previous section when `None`.
    pub start: Option<u32>,
}

/// When line numbers restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineNumberRestart {
    /// On each page.
    EachPage,
    /// At the start of the section.
    EachSection,
    /// Continue from the previous section.
    Continuous,
}

/// Line numbering of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineNumbering {
    /// Number every n-th line.
    pub count_by: u32,
    /// First line number.
    pub start: u32,
    /// Distance between the numbers and the text (automatic when `None`).
    pub distance: Option<Length>,
    /// When numbering restarts.
    pub restart: LineNumberRestart,
}

impl Default for LineNumbering {
    fn default() -> Self {
        LineNumbering {
            count_by: 1,
            start: 1,
            distance: None,
            restart: LineNumberRestart::EachPage,
        }
    }
}

/// Borders around the pages of a section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageBorders {
    /// Top border.
    pub top: Option<Border>,
    /// Left border.
    pub left: Option<Border>,
    /// Bottom border.
    pub bottom: Option<Border>,
    /// Right border.
    pub right: Option<Border>,
    /// Measure the border spacing from the text instead of the page edge.
    pub from_text: bool,
}

impl PageBorders {
    /// The same border on all four sides, measured from the page edge.
    pub fn around(border: Border) -> Self {
        PageBorders {
            top: Some(border.clone()),
            left: Some(border.clone()),
            bottom: Some(border.clone()),
            right: Some(border),
            from_text: false,
        }
    }

    fn to_ct(&self) -> Result<wml::CT_PageBorders> {
        fn side<T: openxml_xml::XmlRead>(b: &Option<Border>, local: &str) -> Result<Option<Box<T>>> {
            b.as_ref()
                .map(|b| convert::<wml::CT_Border, T>(&b.to_ct()?, local).map(Box::new))
                .transpose()
        }
        Ok(wml::CT_PageBorders {
            offset_from: Some(if self.from_text {
                wml::ST_PageBorderOffset::Text
            } else {
                wml::ST_PageBorderOffset::Page
            }),
            top: side(&self.top, "top")?,
            left: side(&self.left, "left")?,
            bottom: side(&self.bottom, "bottom")?,
            right: side(&self.right, "right")?,
            ..Default::default()
        })
    }

    fn from_ct(b: &wml::CT_PageBorders) -> Self {
        fn side<T: openxml_xml::XmlWrite>(b: &Option<Box<T>>, local: &str) -> Option<Border> {
            let ct: wml::CT_Border = convert(b.as_deref()?, local).ok()?;
            Border::from_ct(&ct)
        }
        PageBorders {
            top: side(&b.top, "top"),
            left: side(&b.left, "left"),
            bottom: side(&b.bottom, "bottom"),
            right: side(&b.right, "right"),
            from_text: b.offset_from == Some(wml::ST_PageBorderOffset::Text),
        }
    }
}

/// Read-only view of the properties of a section.
#[derive(Clone, Copy, Debug)]
pub struct Section<'a> {
    sect: &'a wml::CT_SectPr,
}

impl<'a> Section<'a> {
    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_SectPr {
        self.sect
    }

    /// How the section starts (next page when not specified).
    pub fn break_type(&self) -> SectionBreak {
        self.sect
            .type_
            .as_deref()
            .and_then(|t| t.val)
            .map_or(SectionBreak::NextPage, SectionBreak::from_mark)
    }

    /// Page size, orientation and margins.
    pub fn page_setup(&self) -> PageSetup {
        read_page_setup(Some(self.sect))
    }

    /// Columns (one column when not specified).
    pub fn columns(&self) -> Columns {
        let cols = self.sect.cols.as_deref();
        Columns {
            count: cols.and_then(|c| c.num).unwrap_or(1).max(1) as u32,
            spacing: cols
                .and_then(|c| c.space.as_ref())
                .and_then(twips_value)
                .unwrap_or(Length::twips(720)),
            separator: cols.and_then(|c| c.sep.as_ref()).is_some_and(on_off_attr),
        }
    }

    /// Page numbering, when specified.
    pub fn page_numbering(&self) -> Option<PageNumbering> {
        let n = self.sect.pg_num_type.as_deref()?;
        Some(PageNumbering {
            format: n.fmt,
            start: n.start.map(|s| s.max(0) as u32),
        })
    }

    /// Whether the first page has its own header and footer.
    pub fn title_page(&self) -> bool {
        is_on(&self.sect.title_pg)
    }

    /// Line numbering, when enabled.
    pub fn line_numbering(&self) -> Option<LineNumbering> {
        let l = self.sect.ln_num_type.as_deref()?;
        Some(LineNumbering {
            count_by: l.count_by.unwrap_or(1).max(1) as u32,
            // ECMA-376 §17.6.8: the actual first number, 1 when absent.
            start: l.start.unwrap_or(1).max(0) as u32,
            distance: l.distance.as_ref().and_then(twips_value),
            restart: match l.restart {
                None | Some(ST_LineNumberRestart::NewPage) => LineNumberRestart::EachPage,
                Some(ST_LineNumberRestart::NewSection) => LineNumberRestart::EachSection,
                Some(ST_LineNumberRestart::Continuous) => LineNumberRestart::Continuous,
            },
        })
    }

    /// Page borders, when specified.
    pub fn page_borders(&self) -> Option<PageBorders> {
        Some(PageBorders::from_ct(self.sect.pg_borders.as_deref()?))
    }

    /// Relationship id of the header or footer of the given type referenced
    /// by this section (sections without one inherit the previous section's).
    pub fn reference(&self, kind: HeaderFooterKind, ty: HeaderFooterType) -> Option<&'a str> {
        find_reference(self.sect, kind, ty)
    }
}

fn on_off_attr(v: &openxml_schema::shared_types::ST_OnOff) -> bool {
    use openxml_schema::shared_types::{ST_OnOff, ST_OnOff1};
    matches!(v, ST_OnOff::Boolean(true) | ST_OnOff::OnOff1(ST_OnOff1::On))
}

fn find_reference(sect: &wml::CT_SectPr, kind: HeaderFooterKind, ty: HeaderFooterType) -> Option<&str> {
    sect.hdr_ftr_references.iter().find_map(|r| {
        let rf = match (kind, r) {
            (HeaderFooterKind::Header, EG_HdrFtrReferences::HeaderReference(h)) => h,
            (HeaderFooterKind::Footer, EG_HdrFtrReferences::FooterReference(f)) => f,
            _ => return None,
        };
        (rf.type_.unwrap_or(ST_HdrFtr::Default) == ty.to_st())
            .then_some(rf.r_id.as_deref())
            .flatten()
    })
}

/// Relationship ids of all header and footer references of a section.
fn references(sect: &wml::CT_SectPr) -> Vec<&str> {
    sect.hdr_ftr_references
        .iter()
        .filter_map(|r| match r {
            EG_HdrFtrReferences::HeaderReference(h) | EG_HdrFtrReferences::FooterReference(h) => {
                h.r_id.as_deref()
            }
            EG_HdrFtrReferences::Other(_) => None,
        })
        .collect()
}

/// Section properties of a body in document order: those stored in the
/// paragraphs that end a section, then the body's own (last section).
fn body_sections(body: &wml::CT_Body) -> Vec<&wml::CT_SectPr> {
    static EMPTY: std::sync::OnceLock<wml::CT_SectPr> = std::sync::OnceLock::new();
    let mut out: Vec<&wml::CT_SectPr> = body
        .block_level_elts
        .iter()
        .filter_map(|b| match b {
            wml::EG_BlockLevelElts::P(p) => p.p_pr.as_deref()?.sect_pr.as_deref(),
            _ => None,
        })
        .collect();
    out.push(
        body.sect_pr
            .as_deref()
            .unwrap_or_else(|| EMPTY.get_or_init(Default::default)),
    );
    out
}

/// Mutable section properties in document order (the last one is created if missing).
pub(crate) fn body_sections_mut(body: &mut wml::CT_Body) -> Vec<&mut wml::CT_SectPr> {
    let wml::CT_Body {
        block_level_elts,
        sect_pr,
        ..
    } = body;
    let mut out: Vec<&mut wml::CT_SectPr> = block_level_elts
        .iter_mut()
        .filter_map(|b| match b {
            wml::EG_BlockLevelElts::P(p) => p.p_pr.as_deref_mut()?.sect_pr.as_deref_mut(),
            _ => None,
        })
        .collect();
    out.push(sect_pr.get_or_insert_with(Default::default));
    out
}

impl Document {
    /// Page setup of the last (or only) section.
    pub fn page_setup(&self) -> PageSetup {
        read_page_setup(self.body().sect_pr.as_deref())
    }

    /// Sets the page size, orientation and margins of the last section.
    pub fn set_page_setup(&mut self, setup: &PageSetup) {
        let sect = self.body_mut().sect_pr.get_or_insert_with(Default::default);
        apply_page_setup(sect, setup);
    }

    /// Sections of the document in order.
    pub fn sections(&self) -> Vec<Section<'_>> {
        body_sections(self.body())
            .into_iter()
            .map(|sect| Section { sect })
            .collect()
    }

    /// Ends the current (last) section with a paragraph holding its
    /// properties and starts a new section of kind `start`. The new section
    /// copies the page setup, columns and header/footer references of the
    /// previous one. Returns the new section for further setup.
    pub fn add_section(&mut self, start: SectionBreak) -> SectionMut<'_> {
        let body = self.body_mut();
        let mut ending = body.sect_pr.take().map(|s| *s).unwrap_or_else(|| {
            let mut s = wml::CT_SectPr::default();
            apply_page_setup(&mut s, &PageSetup::letter());
            s
        });
        let mut next = ending.clone();
        // A page numbering restart and the title page belong to the section that set them.
        next.pg_num_type = None;
        next.title_pg = None;
        next.type_ = Some(Box::new(wml::CT_SectType {
            val: Some(start.to_mark()),
            ..Default::default()
        }));
        ending.sect_pr_change = None;
        let p = wml::CT_P {
            p_pr: Some(Box::new(wml::CT_PPr {
                sect_pr: Some(Box::new(ending)),
                ..Default::default()
            })),
            ..Default::default()
        };
        body.block_level_elts.push(wml::EG_BlockLevelElts::P(Box::new(p)));
        body.sect_pr = Some(Box::new(next));
        let index = body_sections(self.body()).len() - 1;
        SectionMut { doc: self, index }
    }

    /// Mutable access to the `index`-th section.
    pub fn section_mut(&mut self, index: usize) -> Option<SectionMut<'_>> {
        (index < self.sections().len()).then_some(SectionMut { doc: self, index })
    }

    /// Mutable access to the last section.
    pub fn last_section_mut(&mut self) -> SectionMut<'_> {
        let index = self.sections().len() - 1;
        SectionMut { doc: self, index }
    }

    /// Uses different headers and footers on even and odd pages in the
    /// whole document (`w:evenAndOddHeaders` in the settings).
    pub fn set_even_and_odd_headers(&mut self, value: bool) -> Result<()> {
        self.shared.settings_mut()?.even_and_odd_headers = value.then(on);
        Ok(())
    }

    /// Whether even and odd pages have different headers and footers.
    pub fn even_and_odd_headers(&self) -> bool {
        self.settings().is_some_and(|s| is_on(&s.even_and_odd_headers))
    }

    /// All header and footer parts of the document.
    pub fn headers_and_footers(&self) -> Vec<HeaderFooter<'_>> {
        self.headers
            .iter()
            .map(|h| HeaderFooter {
                kind: h.kind,
                name: &h.part.name,
                content: &h.part.value,
            })
            .collect()
    }

    fn part_index(&self, section: usize, kind: HeaderFooterKind, ty: HeaderFooterType) -> Option<usize> {
        let sections = body_sections(self.body());
        let id = find_reference(sections.get(section)?, kind, ty)?;
        let name = self
            .shared
            .package
            .relationship_target(Some(&self.shared.main_part), id)?;
        self.headers
            .iter()
            .position(|h| h.part.name == name && h.kind == kind)
    }

    /// The header or footer of the given type referenced by a section.
    pub fn header_footer(
        &self,
        section: usize,
        kind: HeaderFooterKind,
        ty: HeaderFooterType,
    ) -> Option<HeaderFooter<'_>> {
        let h = &self.headers[self.part_index(section, kind, ty)?];
        Some(HeaderFooter {
            kind: h.kind,
            name: &h.part.name,
            content: &h.part.value,
        })
    }

    /// The default header of the last section.
    pub fn header(&self) -> Option<HeaderFooter<'_>> {
        self.header_footer(
            self.sections().len() - 1,
            HeaderFooterKind::Header,
            HeaderFooterType::Default,
        )
    }

    /// The default footer of the last section.
    pub fn footer(&self) -> Option<HeaderFooter<'_>> {
        self.header_footer(
            self.sections().len() - 1,
            HeaderFooterKind::Footer,
            HeaderFooterType::Default,
        )
    }

    /// Index of the header/footer part of `section`, creating an empty part
    /// (one paragraph in the `Header`/`Footer` style) when the section does
    /// not reference one of that type. With `reset` an existing part is
    /// emptied too.
    pub(crate) fn ensure_header_footer(
        &mut self,
        section: usize,
        kind: HeaderFooterKind,
        ty: HeaderFooterType,
        reset: bool,
    ) -> Result<usize> {
        let style_id = self.shared.resolve_style(match kind {
            HeaderFooterKind::Header => "Header",
            HeaderFooterKind::Footer => "Footer",
        })?;
        let content = crate::template::header_footer(&style_id);
        // A part referenced by other sections too (sections copy the
        // references of the previous one) is not overwritten: this section
        // gets its own part.
        let shared = {
            let sections = body_sections(self.body());
            let id = sections.get(section).and_then(|s| find_reference(s, kind, ty));
            id.is_some_and(|id| {
                sections
                    .iter()
                    .enumerate()
                    .any(|(i, s)| i != section && references(s).contains(&id))
            })
        };
        let index = match self.part_index(section, kind, ty) {
            Some(i) if !(reset && shared) => {
                if reset {
                    self.headers[i].part.value = content;
                    self.headers[i].part.dirty = true;
                }
                i
            }
            _ => {
                let (pattern, content_type, rel) = match kind {
                    HeaderFooterKind::Header => ("/word/header{}.xml", ct::WML_HEADER, rel_types::HEADER),
                    HeaderFooterKind::Footer => ("/word/footer{}.xml", ct::WML_FOOTER, rel_types::FOOTER),
                };
                let pkg = &mut self.shared.package;
                let name = pkg.next_part_name(pattern)?;
                pkg.add_part(name.clone(), content_type, Vec::new())?;
                let id = pkg.add_relationship(Some(&self.shared.main_part), rel, &name)?;
                let reference = Box::new(wml::CT_HdrFtrRef {
                    r_id: Some(id),
                    type_: Some(ty.to_st()),
                    ..Default::default()
                });
                let body = self.body_mut();
                let mut sections = body_sections_mut(body);
                let sect = sections
                    .get_mut(section)
                    .ok_or_else(|| Error::NotFound(format!("section {section}")))?;
                sect.hdr_ftr_references.retain(|r| {
                    let (same_kind, rf) = match r {
                        EG_HdrFtrReferences::HeaderReference(h) => {
                            (kind == HeaderFooterKind::Header, Some(h))
                        }
                        EG_HdrFtrReferences::FooterReference(f) => {
                            (kind == HeaderFooterKind::Footer, Some(f))
                        }
                        EG_HdrFtrReferences::Other(_) => (false, None),
                    };
                    !(same_kind && rf.is_some_and(|rf| rf.type_.unwrap_or(ST_HdrFtr::Default) == ty.to_st()))
                });
                sect.hdr_ftr_references.push(match kind {
                    HeaderFooterKind::Header => EG_HdrFtrReferences::HeaderReference(reference),
                    HeaderFooterKind::Footer => EG_HdrFtrReferences::FooterReference(reference),
                });
                self.headers.push(HeaderFooterPart {
                    kind,
                    part: Typed {
                        name,
                        value: content,
                        dirty: true,
                    },
                });
                self.headers.len() - 1
            }
        };
        match ty {
            HeaderFooterType::First => {
                let body = self.body_mut();
                if let Some(sect) = body_sections_mut(body).into_iter().nth(section) {
                    sect.title_pg = Some(on());
                }
            }
            HeaderFooterType::Even => self.set_even_and_odd_headers(true)?,
            HeaderFooterType::Default => {}
        }
        Ok(index)
    }

    /// Replaces a header/footer with one paragraph containing `text`.
    pub(crate) fn set_header_footer(
        &mut self,
        section: usize,
        kind: HeaderFooterKind,
        ty: HeaderFooterType,
        text: &str,
    ) -> Result<ParagraphMut<'_>> {
        let index = self.ensure_header_footer(section, kind, ty, true)?;
        let part = self.headers[index].part.name.clone();
        let content = &mut self.headers[index].part.value;
        let Some(wml::EG_BlockLevelElts::P(p)) = content.block_level_elts.first_mut() else {
            unreachable!("the template has one paragraph")
        };
        let mut handle = ParagraphMut::new(p, &mut self.shared, part);
        if !text.is_empty() {
            handle.add_text(text);
        }
        Ok(handle)
    }

    /// Sets the default header of the last section to a single paragraph
    /// with `text`, creating the header part if needed. Returns the
    /// paragraph for further formatting.
    pub fn set_header(&mut self, text: &str) -> Result<ParagraphMut<'_>> {
        let last = self.sections().len() - 1;
        self.set_header_footer(last, HeaderFooterKind::Header, HeaderFooterType::Default, text)
    }

    /// Sets the default footer of the last section (see [`Document::set_header`]).
    pub fn set_footer(&mut self, text: &str) -> Result<ParagraphMut<'_>> {
        let last = self.sections().len() - 1;
        self.set_header_footer(last, HeaderFooterKind::Footer, HeaderFooterType::Default, text)
    }
}

/// Mutable access to the properties of one section.
///
/// ```
/// use openxml_docx::{Columns, Document, HeaderFooterType, Length, PageNumbering, SectionBreak};
///
/// let mut doc = Document::new();
/// doc.add_paragraph("Title page");
/// let mut section = doc.add_section(SectionBreak::OddPage);
/// section.set_columns(&Columns { count: 2, spacing: Length::cm(1.0), separator: true });
/// section.set_page_numbering(&PageNumbering { format: None, start: Some(1) });
/// section.set_header(HeaderFooterType::First, "Chapter start")?;
/// doc.add_paragraph("Two columns");
/// assert_eq!(doc.sections().len(), 2);
/// assert_eq!(doc.sections()[1].columns().count, 2);
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Debug)]
pub struct SectionMut<'a> {
    doc: &'a mut Document,
    index: usize,
}

impl SectionMut<'_> {
    /// Index of the section.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The underlying schema object.
    pub fn raw(&mut self) -> &mut wml::CT_SectPr {
        let index = self.index;
        body_sections_mut(self.doc.body_mut())
            .into_iter()
            .nth(index)
            .expect("the section index was checked")
    }

    /// Read-only view.
    pub fn view(&self) -> Section<'_> {
        self.doc.sections()[self.index]
    }

    /// Sets how the section starts.
    pub fn set_break(&mut self, start: SectionBreak) -> &mut Self {
        self.raw().type_ = Some(Box::new(wml::CT_SectType {
            val: Some(start.to_mark()),
            ..Default::default()
        }));
        self
    }

    /// Sets the page size, orientation and margins.
    pub fn set_page_setup(&mut self, setup: &PageSetup) -> &mut Self {
        apply_page_setup(self.raw(), setup);
        self
    }

    /// Sets equal-width columns.
    pub fn set_columns(&mut self, columns: &Columns) -> &mut Self {
        self.raw().cols = Some(Box::new(wml::CT_Columns {
            equal_width: Some(openxml_schema::shared_types::ST_OnOff::Boolean(true)),
            space: Some(twips(columns.spacing)),
            num: Some(i64::from(columns.count.max(1))),
            sep: columns
                .separator
                .then_some(openxml_schema::shared_types::ST_OnOff::Boolean(true)),
            ..Default::default()
        }));
        self
    }

    /// Sets the page number format and restart value.
    pub fn set_page_numbering(&mut self, numbering: &PageNumbering) -> &mut Self {
        self.raw().pg_num_type = Some(Box::new(wml::CT_PageNumber {
            fmt: numbering.format,
            start: numbering.start.map(i64::from),
            ..Default::default()
        }));
        self
    }

    /// Gives the first page its own header and footer.
    pub fn set_title_page(&mut self, value: bool) -> &mut Self {
        self.raw().title_pg = Some(if value { on() } else { off() });
        self
    }

    /// Enables (`Some`) or disables (`None`) line numbering.
    pub fn set_line_numbering(&mut self, numbering: Option<&LineNumbering>) -> &mut Self {
        self.raw().ln_num_type = numbering.map(|n| {
            Box::new(wml::CT_LineNumber {
                count_by: Some(i64::from(n.count_by.max(1))),
                // Omitted for the default (1): Word reads a present value as zero-based.
                start: (n.start != 1).then_some(i64::from(n.start)),
                distance: n.distance.map(twips),
                restart: Some(match n.restart {
                    LineNumberRestart::EachPage => ST_LineNumberRestart::NewPage,
                    LineNumberRestart::EachSection => ST_LineNumberRestart::NewSection,
                    LineNumberRestart::Continuous => ST_LineNumberRestart::Continuous,
                }),
                ..Default::default()
            })
        });
        self
    }

    /// Sets (`Some`) or removes (`None`) the page borders.
    pub fn set_page_borders(&mut self, borders: Option<&PageBorders>) -> Result<&mut Self> {
        let value = borders.map(PageBorders::to_ct).transpose()?.map(Box::new);
        self.raw().pg_borders = value;
        Ok(self)
    }

    /// Sets a header of this section to one paragraph with `text`,
    /// creating the part when the section has no header of that type.
    pub fn set_header(&mut self, ty: HeaderFooterType, text: &str) -> Result<ParagraphMut<'_>> {
        self.doc
            .set_header_footer(self.index, HeaderFooterKind::Header, ty, text)
    }

    /// Sets a footer of this section (see [`SectionMut::set_header`]).
    pub fn set_footer(&mut self, ty: HeaderFooterType, text: &str) -> Result<ParagraphMut<'_>> {
        self.doc
            .set_header_footer(self.index, HeaderFooterKind::Footer, ty, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_setup_round_trips_through_section_properties() {
        let setup = PageSetup {
            margins: Margins {
                top: Length::cm(2.0),
                ..Margins::default()
            },
            ..PageSetup::a4()
        }
        .landscape();
        assert_eq!(setup.orientation, Orientation::Landscape);
        assert!(setup.width > setup.height);
        let mut sect = wml::CT_SectPr::default();
        apply_page_setup(&mut sect, &setup);
        let back = read_page_setup(Some(&sect));
        assert_eq!(back.width, setup.width);
        assert_eq!(back.height, setup.height);
        assert_eq!(back.orientation, Orientation::Landscape);
        assert_eq!(back.margins.top.as_twips(), Length::cm(2.0).as_twips());
        assert_eq!(back.margins.left, Length::inches(1.0));
        assert!(sect.cols.is_some() && sect.doc_grid.is_some());
    }

    #[test]
    fn defaults_when_section_properties_are_missing() {
        assert_eq!(read_page_setup(None), PageSetup::letter());
        let partial = wml::CT_SectPr {
            pg_sz: Some(Box::new(wml::CT_PageSz {
                w: Some(
                    openxml_schema::shared_types::ST_TwipsMeasure::PositiveUniversalMeasure("21cm".into()),
                ),
                ..Default::default()
            })),
            ..Default::default()
        };
        let s = read_page_setup(Some(&partial));
        assert_eq!(s.width, Length::cm(21.0));
        assert_eq!(s.height, PageSetup::letter().height);
        assert_eq!(
            PageSetup::letter().landscape().landscape().width,
            PageSetup::letter().height
        );
    }
}
