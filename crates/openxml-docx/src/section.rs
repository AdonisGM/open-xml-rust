//! Page setup, headers and footers.

use openxml_core::{Length, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::wml::{self, EG_HdrFtrReferences, ST_HdrFtr, ST_PageOrientation};

use crate::document::{Document, HeaderFooterPart, Typed};
use crate::paragraph::{Paragraph, ParagraphMut};
use crate::text;
use crate::util::{signed_twips, signed_twips_value, twips, twips_value};

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

    fn default_reference(&self, kind: HeaderFooterKind) -> Option<String> {
        let sect = self.body().sect_pr.as_deref()?;
        sect.hdr_ftr_references.iter().find_map(|r| {
            let rf = match (kind, r) {
                (HeaderFooterKind::Header, EG_HdrFtrReferences::HeaderReference(h)) => h,
                (HeaderFooterKind::Footer, EG_HdrFtrReferences::FooterReference(f)) => f,
                _ => return None,
            };
            matches!(rf.type_, None | Some(ST_HdrFtr::Default))
                .then(|| rf.r_id.clone())
                .flatten()
        })
    }

    fn default_part_index(&self, kind: HeaderFooterKind) -> Option<usize> {
        let id = self.default_reference(kind)?;
        let name = self
            .shared
            .package
            .relationship_target(Some(&self.shared.main_part), &id)?;
        self.headers
            .iter()
            .position(|h| h.part.name == name && h.kind == kind)
    }

    /// The default header of the last section.
    pub fn header(&self) -> Option<HeaderFooter<'_>> {
        let i = self.default_part_index(HeaderFooterKind::Header)?;
        let h = &self.headers[i];
        Some(HeaderFooter {
            kind: h.kind,
            name: &h.part.name,
            content: &h.part.value,
        })
    }

    /// The default footer of the last section.
    pub fn footer(&self) -> Option<HeaderFooter<'_>> {
        let i = self.default_part_index(HeaderFooterKind::Footer)?;
        let h = &self.headers[i];
        Some(HeaderFooter {
            kind: h.kind,
            name: &h.part.name,
            content: &h.part.value,
        })
    }

    fn set_header_footer(&mut self, kind: HeaderFooterKind, text: &str) -> Result<ParagraphMut<'_>> {
        let style_id = self.shared.resolve_style(match kind {
            HeaderFooterKind::Header => "Header",
            HeaderFooterKind::Footer => "Footer",
        })?;
        let content = crate::template::header_footer(&style_id);
        let index = match self.default_part_index(kind) {
            Some(i) => {
                self.headers[i].part.value = content;
                self.headers[i].part.dirty = true;
                i
            }
            None => {
                let (pattern, ty, rel) = match kind {
                    HeaderFooterKind::Header => ("/word/header{}.xml", ct::WML_HEADER, rel_types::HEADER),
                    HeaderFooterKind::Footer => ("/word/footer{}.xml", ct::WML_FOOTER, rel_types::FOOTER),
                };
                let pkg = &mut self.shared.package;
                let name = pkg.next_part_name(pattern)?;
                pkg.add_part(name.clone(), ty, Vec::new())?;
                let id = pkg.add_relationship(Some(&self.shared.main_part), rel, &name)?;
                let reference = Box::new(wml::CT_HdrFtrRef {
                    r_id: Some(id),
                    type_: Some(ST_HdrFtr::Default),
                    ..Default::default()
                });
                let sect = self.body_mut().sect_pr.get_or_insert_with(Default::default);
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
                    !(same_kind && rf.is_some_and(|rf| matches!(rf.type_, None | Some(ST_HdrFtr::Default))))
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
        self.set_header_footer(HeaderFooterKind::Header, text)
    }

    /// Sets the default footer of the last section (see [`Document::set_header`]).
    pub fn set_footer(&mut self, text: &str) -> Result<ParagraphMut<'_>> {
        self.set_header_footer(HeaderFooterKind::Footer, text)
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
