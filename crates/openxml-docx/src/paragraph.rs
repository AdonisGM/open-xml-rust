//! Paragraphs.

use openxml_core::{Length, Result};
use openxml_opc::PartName;
use openxml_schema::wml::{self, EG_PContent, ST_Jc};

use crate::document::Shared;
use crate::run::{BreakKind, Run, RunMut};
use crate::text;
use crate::util::{self, is_on, off, on, string_val};

/// Horizontal alignment of a paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Aligned to the left (leading) margin.
    Left,
    /// Centered.
    Center,
    /// Aligned to the right (trailing) margin.
    Right,
    /// Justified between both margins.
    Justify,
    /// Characters distributed evenly.
    Distribute,
}

impl Alignment {
    pub(crate) fn to_jc(self) -> ST_Jc {
        match self {
            Alignment::Left => ST_Jc::Left,
            Alignment::Center => ST_Jc::Center,
            Alignment::Right => ST_Jc::Right,
            Alignment::Justify => ST_Jc::Both,
            Alignment::Distribute => ST_Jc::Distribute,
        }
    }

    pub(crate) fn from_jc(jc: ST_Jc) -> Option<Self> {
        Some(match jc {
            ST_Jc::Left | ST_Jc::Start => Alignment::Left,
            ST_Jc::Center => Alignment::Center,
            ST_Jc::Right | ST_Jc::End => Alignment::Right,
            ST_Jc::Both => Alignment::Justify,
            ST_Jc::Distribute => Alignment::Distribute,
            _ => return None,
        })
    }
}

/// A hyperlink found in a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyperlinkRef {
    /// Displayed text.
    pub text: String,
    /// Relationship id of an external target (resolve with [`crate::Document::hyperlink_target`]).
    pub relationship_id: Option<String>,
    /// Bookmark name for an internal target.
    pub anchor: Option<String>,
    /// Tooltip shown when hovering the link.
    pub tooltip: Option<String>,
}

/// Target of a hyperlink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// An external URL.
    Url(String),
    /// An e-mail address (`mailto:`), with an optional subject.
    Email {
        /// Address.
        address: String,
        /// Subject line.
        subject: Option<String>,
    },
    /// A bookmark inside the document.
    Bookmark(String),
}

impl LinkTarget {
    /// The URL of an external target (`mailto:` for e-mail addresses).
    pub fn url(&self) -> Option<String> {
        match self {
            LinkTarget::Url(u) => Some(u.clone()),
            LinkTarget::Email { address, subject } => Some(match subject {
                Some(s) => format!("mailto:{address}?subject={}", percent_encode(s)),
                None => format!("mailto:{address}"),
            }),
            LinkTarget::Bookmark(_) => None,
        }
    }
}

/// Percent-encodes everything but unreserved URI characters.
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Read-only view of a paragraph.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    pub(crate) p: &'a wml::CT_P,
}

impl<'a> Paragraph<'a> {
    pub(crate) fn new(p: &'a wml::CT_P) -> Self {
        Paragraph { p }
    }

    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_P {
        self.p
    }

    /// Plain text of the paragraph (tabs as `\t`, breaks as `\n`; deleted text excluded).
    pub fn text(&self) -> String {
        text::paragraph_text(self.p)
    }

    /// Paragraph style id, if one is applied.
    pub fn style_id(&self) -> Option<&'a str> {
        self.p.p_pr.as_deref()?.p_style.as_deref()?.val.as_deref()
    }

    /// Visible runs in document order, including runs inside hyperlinks,
    /// fields, content controls and tracked insertions.
    pub fn runs(&self) -> Vec<Run<'a>> {
        text::runs(&self.p.p_content).into_iter().map(Run::new).collect()
    }

    /// Direct paragraph alignment.
    pub fn alignment(&self) -> Option<Alignment> {
        self.p
            .p_pr
            .as_deref()?
            .jc
            .as_deref()?
            .val
            .and_then(Alignment::from_jc)
    }

    /// Numbering applied directly: `(numbering id, level)`.
    pub fn numbering(&self) -> Option<(i64, i64)> {
        let num = self.p.p_pr.as_deref()?.num_pr.as_deref()?;
        Some((
            num.num_id.as_deref()?.val?,
            num.ilvl.as_deref().and_then(|l| l.val).unwrap_or(0),
        ))
    }

    /// Hyperlinks of the paragraph.
    pub fn hyperlinks(&self) -> Vec<HyperlinkRef> {
        fn collect(items: &[EG_PContent], out: &mut Vec<HyperlinkRef>) {
            for item in items {
                match item {
                    EG_PContent::Hyperlink(h) => out.push(HyperlinkRef {
                        text: text::runs(&h.p_content)
                            .iter()
                            .map(|r| text::run_text(r))
                            .collect(),
                        relationship_id: h.r_id.clone(),
                        anchor: h.anchor.clone(),
                        tooltip: h.tooltip.clone(),
                    }),
                    EG_PContent::Sdt(s) => {
                        if let Some(c) = &s.sdt_content {
                            collect(&c.p_content, out);
                        }
                    }
                    EG_PContent::CustomXml(x) => collect(&x.p_content, out),
                    EG_PContent::SmartTag(x) => collect(&x.p_content, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        collect(&self.p.p_content, &mut out);
        out
    }

    /// Whether the paragraph starts on a new page.
    pub fn page_break_before(&self) -> bool {
        self.p
            .p_pr
            .as_deref()
            .is_some_and(|p| is_on(&p.page_break_before))
    }

    /// Whether the paragraph has no text and no drawings.
    pub fn is_empty(&self) -> bool {
        self.text().is_empty() && !self.runs().iter().any(|r| r.has_drawing())
    }
}

/// Mutable access to a paragraph.
///
/// Obtained from [`crate::Document::add_paragraph`], [`crate::Document::paragraph_mut`],
/// table cells or headers. Methods return `&mut Self` so calls can be chained.
///
/// ```
/// use openxml_docx::{Alignment, Document};
///
/// let mut doc = Document::new();
/// let mut p = doc.add_paragraph("Visit ");
/// p.set_alignment(Alignment::Center);
/// p.add_hyperlink("our site", "https://example.com")?;
/// assert_eq!(doc.paragraphs()[0].text(), "Visit our site");
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Debug)]
pub struct ParagraphMut<'a> {
    pub(crate) p: &'a mut wml::CT_P,
    pub(crate) shared: &'a mut Shared,
    pub(crate) part: PartName,
}

impl<'a> ParagraphMut<'a> {
    pub(crate) fn new(p: &'a mut wml::CT_P, shared: &'a mut Shared, part: PartName) -> Self {
        ParagraphMut { p, shared, part }
    }

    /// The underlying schema object.
    pub fn raw(&mut self) -> &mut wml::CT_P {
        self.p
    }

    /// Read-only view.
    pub fn view(&self) -> Paragraph<'_> {
        Paragraph::new(self.p)
    }

    pub(crate) fn p_pr(&mut self) -> &mut wml::CT_PPr {
        self.p.p_pr.get_or_insert_with(Default::default)
    }

    pub(crate) fn push_run(&mut self, run: wml::CT_R) -> RunMut<'_> {
        self.p.p_content.push(EG_PContent::R(Box::new(run)));
        let Some(EG_PContent::R(r)) = self.p.p_content.last_mut() else {
            unreachable!()
        };
        RunMut::new(r)
    }

    /// Appends a run with `text` (which may be empty) and returns it for formatting.
    pub fn add_run(&mut self, text: &str) -> RunMut<'_> {
        let mut run = wml::CT_R::default();
        if !text.is_empty() {
            RunMut::new(&mut run).add_text(text);
        }
        self.push_run(run)
    }

    /// Appends unformatted text.
    pub fn add_text(&mut self, text: &str) -> &mut Self {
        self.add_run(text);
        self
    }

    /// Appends a tab character.
    pub fn add_tab(&mut self) -> &mut Self {
        self.add_run("").add_tab();
        self
    }

    /// Appends a line, page or column break.
    pub fn add_break(&mut self, kind: BreakKind) -> &mut Self {
        self.add_run("").add_break(kind);
        self
    }

    /// Applies a paragraph style. Built-in styles (`Heading1`, `Title`,
    /// `ListParagraph`, …) are added to the styles part when missing;
    /// otherwise the id is used as given.
    pub fn set_style(&mut self, style_id: &str) -> Result<&mut Self> {
        let id = self.shared.resolve_style(style_id)?;
        self.p_pr().p_style = Some(string_val(&id));
        Ok(self)
    }

    /// Removes the paragraph style.
    pub fn clear_style(&mut self) -> &mut Self {
        if let Some(p) = self.p.p_pr.as_mut() {
            p.p_style = None;
        }
        self
    }

    /// Sets the horizontal alignment.
    pub fn set_alignment(&mut self, alignment: Alignment) -> &mut Self {
        self.p_pr().jc = Some(Box::new(wml::CT_Jc {
            val: Some(alignment.to_jc()),
            ..Default::default()
        }));
        self
    }

    /// Sets space before and after the paragraph.
    pub fn set_spacing(&mut self, before: Option<Length>, after: Option<Length>) -> &mut Self {
        let spacing = self.p_pr().spacing.get_or_insert_with(Default::default);
        spacing.before = before.map(util::twips);
        spacing.after = after.map(util::twips);
        self
    }

    /// Sets the left indentation and the first-line indentation (negative for a hanging indent).
    pub fn set_indent(&mut self, left: Length, first_line: Length) -> &mut Self {
        let ind = self.p_pr().ind.get_or_insert_with(Default::default);
        ind.left = Some(util::signed_twips(left));
        if first_line.as_emu() >= 0 {
            ind.first_line = Some(util::twips(first_line));
            ind.hanging = None;
        } else {
            ind.hanging = Some(util::twips(-first_line));
            ind.first_line = None;
        }
        self
    }

    /// Keeps the paragraph on the same page as the next one.
    pub fn set_keep_with_next(&mut self, value: bool) -> &mut Self {
        self.p_pr().keep_next = Some(if value { on() } else { off() });
        self
    }

    /// Starts the paragraph on a new page.
    pub fn set_page_break_before(&mut self, value: bool) -> &mut Self {
        self.p_pr().page_break_before = Some(if value { on() } else { off() });
        self
    }

    /// Appends a hyperlink to an external URL, formatted with the `Hyperlink` character style.
    pub fn add_hyperlink(&mut self, text: &str, url: &str) -> Result<&mut Self> {
        self.add_link(text, &LinkTarget::Url(url.to_owned()), None)
    }

    /// Appends a hyperlink to a bookmark inside the document.
    pub fn add_internal_hyperlink(&mut self, text: &str, bookmark: &str) -> Result<&mut Self> {
        self.add_link(text, &LinkTarget::Bookmark(bookmark.to_owned()), None)
    }

    /// Appends a hyperlink to a URL, an e-mail address or a bookmark, with
    /// an optional tooltip, formatted with the `Hyperlink` character style.
    ///
    /// ```
    /// use openxml_docx::{Document, LinkTarget};
    ///
    /// let mut doc = Document::new();
    /// let mail = LinkTarget::Email { address: "team@example.com".into(), subject: Some("Hello there".into()) };
    /// doc.add_paragraph("Write to ").add_link("the team", &mail, Some("Send an e-mail"))?;
    /// let link = &doc.paragraphs()[0].hyperlinks()[0];
    /// assert_eq!(link.tooltip.as_deref(), Some("Send an e-mail"));
    /// let target = doc.hyperlink_target(link.relationship_id.as_deref().unwrap());
    /// assert_eq!(target, Some("mailto:team@example.com?subject=Hello%20there"));
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn add_link(&mut self, text: &str, target: &LinkTarget, tooltip: Option<&str>) -> Result<&mut Self> {
        let style = self.shared.resolve_style("Hyperlink")?;
        let mut run = wml::CT_R::default();
        RunMut::new(&mut run).style(&style).add_text(text);
        let mut link = wml::CT_Hyperlink {
            tooltip: tooltip.map(str::to_owned),
            p_content: vec![EG_PContent::R(Box::new(run))],
            ..Default::default()
        };
        match target {
            LinkTarget::Bookmark(name) => link.anchor = Some(name.clone()),
            _ => {
                let url = target.url().expect("external targets have a URL");
                link.r_id = Some(self.shared.add_hyperlink_relationship(&self.part, &url)?);
                link.history = Some(openxml_schema::shared_types::ST_OnOff::Boolean(true));
            }
        }
        self.p.p_content.push(EG_PContent::Hyperlink(Box::new(link)));
        Ok(self)
    }

    /// Appends an inline picture scaled to `width` (the height keeps the aspect ratio).
    ///
    /// Supported formats: PNG, JPEG, GIF, BMP, TIFF, EMF and WMF.
    pub fn add_picture(&mut self, image: &[u8], width: Length) -> Result<&mut Self> {
        let drawing = self.shared.add_picture(&self.part, image, width)?;
        let run = wml::CT_R {
            run_inner_content: vec![wml::EG_RunInnerContent::Drawing(Box::new(drawing))],
            ..Default::default()
        };
        self.p.p_content.push(EG_PContent::R(Box::new(run)));
        Ok(self)
    }

    /// Mutable access to the `index`-th visible run.
    pub fn run_mut(&mut self, index: usize) -> Option<RunMut<'_>> {
        text::runs_mut(&mut self.p.p_content)
            .into_iter()
            .nth(index)
            .map(RunMut::new)
    }

    /// Replaces every occurrence of `from` with `to` in the paragraph text,
    /// including occurrences split across runs. Returns the number of replacements.
    pub fn replace_text(&mut self, from: &str, to: &str) -> usize {
        replace_in_paragraph(self.p, from, to)
    }

    /// Removes all content but keeps the paragraph properties.
    pub fn clear(&mut self) -> &mut Self {
        self.p.p_content.clear();
        self
    }

    /// Applies list numbering: numbering definition `num_id`, level `level` (0–8).
    pub(crate) fn set_numbering(&mut self, num_id: i64, level: u8) -> &mut Self {
        self.p_pr().num_pr = Some(Box::new(wml::CT_NumPr {
            ilvl: Some(util::decimal(i64::from(level))),
            num_id: Some(util::decimal(num_id)),
            ..Default::default()
        }));
        self
    }
}

/// Replaces text in the runs of one paragraph. Each container (the paragraph
/// itself, a hyperlink, a field) is processed separately.
pub(crate) fn replace_in_paragraph(p: &mut wml::CT_P, from: &str, to: &str) -> usize {
    fn containers<'a>(items: &'a mut [EG_PContent], out: &mut Vec<Vec<&'a mut wml::CT_R>>) {
        let mut direct = Vec::new();
        for item in items {
            match item {
                EG_PContent::R(r) => direct.push(&mut **r),
                EG_PContent::Hyperlink(h) => containers(&mut h.p_content, out),
                EG_PContent::FldSimple(f) => containers(&mut f.p_content, out),
                EG_PContent::SmartTag(x) => containers(&mut x.p_content, out),
                EG_PContent::CustomXml(x) => containers(&mut x.p_content, out),
                EG_PContent::Sdt(x) => {
                    if let Some(c) = &mut x.sdt_content {
                        containers(&mut c.p_content, out);
                    }
                }
                _ => {}
            }
        }
        out.push(direct);
    }
    let mut groups = Vec::new();
    containers(&mut p.p_content, &mut groups);
    groups
        .into_iter()
        .map(|runs| {
            let mut nodes = text::text_nodes_mut(runs);
            text::replace_in_nodes(&mut nodes, from, to)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_mapping_round_trips() {
        for a in [
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
            Alignment::Distribute,
        ] {
            assert_eq!(Alignment::from_jc(a.to_jc()), Some(a));
        }
        assert_eq!(Alignment::from_jc(ST_Jc::Start), Some(Alignment::Left));
        assert_eq!(Alignment::from_jc(ST_Jc::End), Some(Alignment::Right));
        assert_eq!(Alignment::from_jc(ST_Jc::NumTab), None);
    }

    #[test]
    fn link_targets() {
        assert_eq!(percent_encode("a b&c/é"), "a%20b%26c%2F%C3%A9");
        let mail = LinkTarget::Email {
            address: "a@b.c".into(),
            subject: None,
        };
        assert_eq!(mail.url().as_deref(), Some("mailto:a@b.c"));
        assert_eq!(LinkTarget::Bookmark("x".into()).url(), None);
        assert_eq!(
            LinkTarget::Url("https://x".into()).url().as_deref(),
            Some("https://x")
        );
    }

    #[test]
    fn replace_keeps_containers_separate() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>foo b</w:t></w:r><w:r><w:t>ar</w:t></w:r><w:hyperlink><w:r><w:t>bar</w:t></w:r></w:hyperlink></w:p></w:body></w:document>"#;
        let mut doc = wml::elements::DOCUMENT.parse(xml).unwrap();
        let wml::EG_BlockLevelElts::P(p) = &mut doc.body.as_mut().unwrap().block_level_elts[0] else {
            panic!()
        };
        assert_eq!(replace_in_paragraph(p, "bar", "baz"), 2);
        assert_eq!(text::paragraph_text(p), "foo bazbaz");
    }

    #[test]
    fn paragraph_view_accessors() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:pageBreakBefore/><w:numPr><w:ilvl w:val="2"/><w:numId w:val="7"/></w:numPr><w:jc w:val="both"/></w:pPr><w:hyperlink r:id="rId9"><w:r><w:t>x</w:t></w:r></w:hyperlink><w:hyperlink w:anchor="bm"><w:r><w:t>y</w:t></w:r></w:hyperlink></w:p></w:body></w:document>"#;
        let doc = wml::elements::DOCUMENT.parse(xml).unwrap();
        let wml::EG_BlockLevelElts::P(p) = &doc.body.as_ref().unwrap().block_level_elts[0] else {
            panic!()
        };
        let v = Paragraph::new(p);
        assert_eq!(v.style_id(), Some("ListParagraph"));
        assert_eq!(v.numbering(), Some((7, 2)));
        assert_eq!(v.alignment(), Some(Alignment::Justify));
        assert!(v.page_break_before());
        let links = v.hyperlinks();
        assert_eq!(
            links[0],
            HyperlinkRef {
                text: "x".into(),
                relationship_id: Some("rId9".into()),
                anchor: None,
                tooltip: None
            }
        );
        assert_eq!(links[1].anchor.as_deref(), Some("bm"));
        assert_eq!(v.runs().len(), 2);
        assert!(!v.is_empty());
        assert!(Paragraph::new(&wml::CT_P::default()).is_empty());
    }
}
