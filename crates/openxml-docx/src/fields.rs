//! Fields (simple `w:fldSimple` and complex `w:fldChar` fields) and the
//! table of contents.

use openxml_core::Result;
use openxml_schema::shared_types::ST_OnOff;
use openxml_schema::wml::{
    self, CT_RunTrackChange_Choice, EG_BlockLevelElts, EG_ContentBlockContent, EG_PContent,
    EG_RunInnerContent, ST_FldCharType,
};

use crate::document::{Document, Shared};
use crate::paragraph::ParagraphMut;
use crate::run::RunMut;
use crate::section::HeaderFooter;
use crate::text;
use crate::util::{on, string_val, text_node};

/// A field code.
///
/// ```
/// use openxml_docx::{Document, Field};
///
/// let mut doc = Document::new();
/// let mut footer = doc.set_footer("Page ")?;
/// footer.add_field(&Field::Page, "1");
/// footer.add_text(" of ");
/// footer.add_field(&Field::NumPages, "1");
/// doc.add_paragraph("Printed on ").add_simple_field(&Field::Date(Some("yyyy-MM-dd".into())), "2024-01-31");
/// assert_eq!(doc.fields()[0].instruction, r#"DATE \@ "yyyy-MM-dd""#);
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    /// Current page number (`PAGE`).
    Page,
    /// Number of pages (`NUMPAGES`).
    NumPages,
    /// Current date (`DATE`) with an optional picture such as `dd/MM/yyyy`.
    Date(Option<String>),
    /// Current time (`TIME`) with an optional picture such as `HH:mm`.
    Time(Option<String>),
    /// Page number of a bookmark (`PAGEREF name \h`).
    PageRef(String),
    /// Any other field instruction, e.g. `AUTHOR \* Upper`.
    Code(String),
}

impl Field {
    /// The field instruction (without the surrounding spaces Word adds).
    pub fn instruction(&self) -> String {
        let picture = |name: &str, f: &Option<String>| match f {
            Some(f) => format!("{name} \\@ \"{f}\""),
            None => name.to_owned(),
        };
        match self {
            Field::Page => "PAGE".into(),
            Field::NumPages => "NUMPAGES".into(),
            Field::Date(f) => picture("DATE", f),
            Field::Time(f) => picture("TIME", f),
            Field::PageRef(b) => format!("PAGEREF {b} \\h"),
            Field::Code(c) => c.trim().to_owned(),
        }
    }
}

/// A field read from a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldInfo {
    /// Field instruction, trimmed (e.g. `PAGE` or `TOC \o "1-3" \h \z \u`).
    pub instruction: String,
    /// Cached result text (the text shown until fields are updated).
    pub result: String,
}

/// Options of a table of contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableOfContents {
    /// Title paragraph (in the `TOCHeading` style); none when `None`.
    pub title: Option<String>,
    /// First heading level included.
    pub first_level: u8,
    /// Last heading level included.
    pub last_level: u8,
    /// Entries are hyperlinks to the headings (`\h`).
    pub hyperlinks: bool,
}

impl Default for TableOfContents {
    /// `TOC \o "1-3" \h \z \u` with the title "Contents".
    fn default() -> Self {
        TableOfContents {
            title: Some("Contents".into()),
            first_level: 1,
            last_level: 3,
            hyperlinks: true,
        }
    }
}

impl TableOfContents {
    /// The field instruction.
    pub fn instruction(&self) -> String {
        format!(
            "TOC \\o \"{}-{}\" {}\\z \\u",
            self.first_level,
            self.last_level,
            if self.hyperlinks { "\\h " } else { "" }
        )
    }
}

/// A run holding a field character.
pub(crate) fn fld_char(kind: ST_FldCharType, form: Option<wml::CT_FFData>) -> wml::CT_R {
    wml::CT_R {
        run_inner_content: vec![EG_RunInnerContent::FldChar(Box::new(wml::CT_FldChar {
            fld_char_type: Some(kind),
            choice: form.map(|f| wml::CT_FldChar_Choice::FfData(Box::new(f))),
            ..Default::default()
        }))],
        ..Default::default()
    }
}

/// A run holding a field instruction (with the spaces Word puts around it).
pub(crate) fn instr_run(instruction: &str) -> wml::CT_R {
    wml::CT_R {
        run_inner_content: vec![EG_RunInnerContent::InstrText(text_node(&format!(
            " {instruction} "
        )))],
        ..Default::default()
    }
}

/// The runs of a complex field with a cached result.
pub(crate) fn complex_field(instruction: &str, result: &str) -> Vec<wml::CT_R> {
    let mut out = vec![
        fld_char(ST_FldCharType::Begin, None),
        instr_run(instruction),
        fld_char(ST_FldCharType::Separate, None),
    ];
    if !result.is_empty() {
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).add_text(result);
        out.push(r);
    }
    out.push(fld_char(ST_FldCharType::End, None));
    out
}

impl ParagraphMut<'_> {
    /// Appends a complex field (`w:fldChar` begin/separate/end around the
    /// instruction and the cached result).
    pub fn add_field(&mut self, field: &Field, cached_result: &str) -> &mut Self {
        for r in complex_field(&field.instruction(), cached_result) {
            self.p.p_content.push(EG_PContent::R(Box::new(r)));
        }
        self
    }

    /// Appends a simple field (`w:fldSimple`) with a cached result.
    pub fn add_simple_field(&mut self, field: &Field, cached_result: &str) -> &mut Self {
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).add_text(cached_result);
        self.p
            .p_content
            .push(EG_PContent::FldSimple(Box::new(wml::CT_SimpleField {
                instr: Some(format!(" {} ", field.instruction())),
                p_content: if cached_result.is_empty() {
                    Vec::new()
                } else {
                    vec![EG_PContent::R(Box::new(r))]
                },
                ..Default::default()
            })));
        self
    }
}

/// A field found while scanning, with its form-field data.
#[derive(Clone, Debug, Default)]
pub(crate) struct RawField {
    pub instruction: String,
    pub result: String,
    pub form: Option<wml::CT_FFData>,
}

#[derive(Default)]
struct Open {
    order: usize,
    field: RawField,
    in_result: bool,
}

/// Scans content for simple and complex fields (nested fields included).
#[derive(Default)]
pub(crate) struct FieldScanner {
    stack: Vec<Open>,
    done: Vec<(usize, RawField)>,
    next: usize,
}

impl FieldScanner {
    fn text(&mut self, s: &str) {
        for open in self.stack.iter_mut().rev() {
            if !open.in_result {
                // Text of a field nested in an instruction belongs to the instruction.
                break;
            }
            open.field.result.push_str(s);
        }
    }

    fn run(&mut self, r: &wml::CT_R) {
        for c in &r.run_inner_content {
            match c {
                EG_RunInnerContent::FldChar(f) => match f.fld_char_type {
                    Some(ST_FldCharType::Begin) => {
                        let form = match &f.choice {
                            Some(wml::CT_FldChar_Choice::FfData(d)) => Some((**d).clone()),
                            _ => None,
                        };
                        self.stack.push(Open {
                            order: self.next,
                            field: RawField {
                                form,
                                ..Default::default()
                            },
                            in_result: false,
                        });
                        self.next += 1;
                    }
                    Some(ST_FldCharType::Separate) => {
                        if let Some(top) = self.stack.last_mut() {
                            top.in_result = true;
                        }
                    }
                    Some(ST_FldCharType::End) => {
                        if let Some(open) = self.stack.pop() {
                            self.done.push((open.order, open.field));
                        }
                    }
                    None => {}
                },
                EG_RunInnerContent::InstrText(t) => {
                    if let Some(top) = self.stack.last_mut()
                        && !top.in_result
                    {
                        top.field.instruction.push_str(&t.value);
                    }
                }
                other => {
                    let mut s = String::new();
                    text::push_run_text(
                        &wml::CT_R {
                            run_inner_content: vec![other.clone()],
                            ..Default::default()
                        },
                        &mut s,
                    );
                    if !s.is_empty() {
                        self.text(&s);
                    }
                }
            }
        }
    }

    fn choices(&mut self, items: &[CT_RunTrackChange_Choice]) {
        for item in items {
            match item {
                CT_RunTrackChange_Choice::WR(r) => self.run(r),
                CT_RunTrackChange_Choice::Ins(x) | CT_RunTrackChange_Choice::MoveTo(x) => {
                    self.choices(&x.choice)
                }
                CT_RunTrackChange_Choice::Sdt(x) => {
                    if let Some(c) = &x.sdt_content {
                        self.content(&c.p_content);
                    }
                }
                CT_RunTrackChange_Choice::CustomXml(x) => self.content(&x.p_content),
                CT_RunTrackChange_Choice::SmartTag(x) => self.content(&x.p_content),
                _ => {}
            }
        }
    }

    pub(crate) fn content(&mut self, items: &[EG_PContent]) {
        for item in items {
            match item {
                EG_PContent::R(r) => self.run(r),
                EG_PContent::FldSimple(f) => {
                    let order = self.next;
                    self.next += 1;
                    let result = text::paragraph_text(&wml::CT_P {
                        p_content: f.p_content.clone(),
                        ..Default::default()
                    });
                    // The cached result also belongs to enclosing fields.
                    self.content(&f.p_content);
                    self.done.push((
                        order,
                        RawField {
                            instruction: f.instr.clone().unwrap_or_default(),
                            result,
                            form: None,
                        },
                    ));
                }
                EG_PContent::Hyperlink(h) => self.content(&h.p_content),
                EG_PContent::CustomXml(x) => self.content(&x.p_content),
                EG_PContent::SmartTag(x) => self.content(&x.p_content),
                EG_PContent::Dir(x) => self.content(&x.p_content),
                EG_PContent::Bdo(x) => self.content(&x.p_content),
                EG_PContent::Sdt(x) => {
                    if let Some(c) = &x.sdt_content {
                        self.content(&c.p_content);
                    }
                }
                EG_PContent::Ins(x) | EG_PContent::MoveTo(x) => self.choices(&x.choice),
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: &wml::CT_P) {
        self.content(&p.p_content);
        self.text("\n");
    }

    fn content_blocks(&mut self, items: &[EG_ContentBlockContent]) {
        for item in items {
            match item {
                EG_ContentBlockContent::P(p) => self.paragraph(p),
                EG_ContentBlockContent::Tbl(t) => self.table(t),
                EG_ContentBlockContent::Sdt(s) => {
                    if let Some(c) = &s.sdt_content {
                        self.content_blocks(&c.content_block_content);
                    }
                }
                EG_ContentBlockContent::CustomXml(x) => self.content_blocks(&x.content_block_content),
                _ => {}
            }
        }
    }

    fn table(&mut self, t: &wml::CT_Tbl) {
        for row in text::rows(t) {
            for cell in text::cells(row) {
                self.blocks(&cell.block_level_elts);
            }
        }
    }

    pub(crate) fn blocks(&mut self, items: &[EG_BlockLevelElts]) {
        for item in items {
            match item {
                EG_BlockLevelElts::P(p) => self.paragraph(p),
                EG_BlockLevelElts::Tbl(t) => self.table(t),
                EG_BlockLevelElts::Sdt(s) => {
                    if let Some(c) = &s.sdt_content {
                        self.content_blocks(&c.content_block_content);
                    }
                }
                EG_BlockLevelElts::CustomXml(x) => self.content_blocks(&x.content_block_content),
                _ => {}
            }
        }
    }

    /// The fields found, in the order they start.
    pub(crate) fn finish(mut self) -> Vec<RawField> {
        self.done.sort_by_key(|(order, _)| *order);
        self.done.into_iter().map(|(_, f)| f).collect()
    }
}

/// Fields of a block container.
pub(crate) fn scan_blocks(items: &[EG_BlockLevelElts]) -> Vec<RawField> {
    let mut scanner = FieldScanner::default();
    scanner.blocks(items);
    scanner.finish()
}

fn public(fields: Vec<RawField>) -> Vec<FieldInfo> {
    fields
        .into_iter()
        .map(|f| {
            let mut result = f.result;
            while result.ends_with('\n') {
                result.pop();
            }
            FieldInfo {
                instruction: f.instruction.trim().to_owned(),
                result,
            }
        })
        .collect()
}

impl HeaderFooter<'_> {
    /// Fields of the header or footer.
    pub fn fields(&self) -> Vec<FieldInfo> {
        public(scan_blocks(&self.raw().block_level_elts))
    }
}

impl Shared {
    /// Outline level (0-based) of a paragraph style, following `basedOn`.
    pub(crate) fn style_outline_level(&self, style_id: &str) -> Option<u8> {
        let styles = &self.styles.as_ref()?.value.style;
        let mut id = style_id.to_owned();
        for _ in 0..16 {
            let style = styles
                .iter()
                .find(|s| s.style_id.as_deref() == Some(id.as_str()))?;
            if let Some(level) = style
                .p_pr
                .as_deref()
                .and_then(|p| p.outline_lvl.as_deref())
                .and_then(|l| l.val)
            {
                return (0..9).contains(&level).then_some(level as u8);
            }
            id = style.based_on.as_deref()?.val.clone()?;
        }
        None
    }
}

impl Document {
    /// Fields of the body in the order they start, with their cached results.
    pub fn fields(&self) -> Vec<FieldInfo> {
        public(scan_blocks(&self.body().block_level_elts))
    }

    /// Asks the application to update all fields when the document is
    /// opened (`w:updateFields` in the settings).
    pub fn set_update_fields_on_open(&mut self, value: bool) -> Result<()> {
        self.shared.settings_mut()?.update_fields = value.then(on);
        Ok(())
    }

    /// Appends a table of contents; see [`Document::insert_table_of_contents`].
    pub fn add_table_of_contents(&mut self, toc: &TableOfContents) -> Result<()> {
        let at = self.body().block_level_elts.len();
        self.insert_table_of_contents(at, toc)
    }

    /// Inserts a table of contents before the `index`-th block of the body
    /// (paragraphs and tables, as in [`crate::Document::blocks`]; past the
    /// end appends).
    ///
    /// The cached result lists the headings present now (entries without
    /// page numbers, linked to `_Toc` bookmarks placed on the headings);
    /// `w:updateFields` is set so that the application rebuilds the table,
    /// with page numbers, when the document is opened.
    ///
    /// ```
    /// use openxml_docx::{Document, TableOfContents};
    ///
    /// let mut doc = Document::new();
    /// doc.add_heading("Introduction", 1)?;
    /// doc.add_heading("Scope", 2)?;
    /// doc.insert_table_of_contents(0, &TableOfContents::default())?;
    /// let toc = &doc.fields()[0];
    /// assert_eq!(toc.instruction, r#"TOC \o "1-3" \h \z \u"#);
    /// assert_eq!(toc.result, "Introduction\nScope");
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn insert_table_of_contents(&mut self, index: usize, toc: &TableOfContents) -> Result<()> {
        let range = toc.first_level.max(1)..=toc.last_level.clamp(toc.first_level.max(1), 9);
        // Collect the headings and bookmark them.
        let part = self.shared.main_part.clone();
        let mut entries: Vec<(u8, String, String)> = Vec::new();
        let body = self.main.body.get_or_insert_with(Default::default);
        let top = body.block_level_elts.len();
        for item in body.block_level_elts.iter_mut() {
            let EG_BlockLevelElts::P(p) = item else { continue };
            let direct = p
                .p_pr
                .as_deref()
                .and_then(|pr| pr.outline_lvl.as_deref())
                .and_then(|l| l.val)
                .filter(|l| (0..9).contains(l))
                .map(|l| l as u8);
            let style = p
                .p_pr
                .as_deref()
                .and_then(|pr| pr.p_style.as_deref())
                .and_then(|s| s.val.clone());
            let Some(level) = direct.or_else(|| style.and_then(|s| self.shared.style_outline_level(&s)))
            else {
                continue;
            };
            let level = level + 1;
            let heading = text::paragraph_text(p);
            if !range.contains(&level) || heading.trim().is_empty() {
                continue;
            }
            let existing = p.p_content.iter().find_map(|c| match c {
                EG_PContent::BookmarkStart(b) => b.name.clone().filter(|n| n.starts_with("_Toc")),
                _ => None,
            });
            let name = match existing {
                Some(name) => name,
                None => {
                    let name = format!("_Toc{}", self.shared.last_id + 1);
                    ParagraphMut::new(p, &mut self.shared, part.clone())
                        .add_bookmark(&name, crate::TextSpan::Paragraph)?;
                    name
                }
            };
            entries.push((level, heading, name));
        }
        let heading_style = match &toc.title {
            Some(_) => Some(self.shared.resolve_style("TOCHeading")?),
            None => None,
        };
        let mut paragraphs: Vec<wml::CT_P> = Vec::new();
        if let (Some(title), Some(style)) = (&toc.title, heading_style) {
            let mut p = wml::CT_P {
                p_pr: Some(Box::new(wml::CT_PPr {
                    p_style: Some(string_val(&style)),
                    ..Default::default()
                })),
                ..Default::default()
            };
            let mut r = wml::CT_R::default();
            RunMut::new(&mut r).add_text(title);
            p.p_content.push(EG_PContent::R(Box::new(r)));
            paragraphs.push(p);
        }
        let first_entry = paragraphs.len();
        if entries.is_empty() {
            let style = self.shared.resolve_style(&format!("TOC{}", range.start()))?;
            let mut r = wml::CT_R::default();
            RunMut::new(&mut r).add_text("No table of contents entries found.");
            paragraphs.push(wml::CT_P {
                p_pr: Some(Box::new(wml::CT_PPr {
                    p_style: Some(string_val(&style)),
                    ..Default::default()
                })),
                p_content: vec![EG_PContent::R(Box::new(r))],
                ..Default::default()
            });
        }
        for (level, heading, bookmark) in &entries {
            let style = self.shared.resolve_style(&format!("TOC{level}"))?;
            let mut r = wml::CT_R::default();
            RunMut::new(&mut r).add_text(heading);
            let content = if toc.hyperlinks {
                EG_PContent::Hyperlink(Box::new(wml::CT_Hyperlink {
                    anchor: Some(bookmark.clone()),
                    history: Some(ST_OnOff::Boolean(true)),
                    p_content: vec![EG_PContent::R(Box::new(r))],
                    ..Default::default()
                }))
            } else {
                EG_PContent::R(Box::new(r))
            };
            paragraphs.push(wml::CT_P {
                p_pr: Some(Box::new(wml::CT_PPr {
                    p_style: Some(string_val(&style)),
                    ..Default::default()
                })),
                p_content: vec![content],
                ..Default::default()
            });
        }
        // The field starts in the first entry paragraph and ends in the last.
        let mut begin = fld_char(ST_FldCharType::Begin, None);
        if let Some(EG_RunInnerContent::FldChar(f)) = begin.run_inner_content.first_mut() {
            f.dirty = Some(ST_OnOff::Boolean(true));
        }
        let head = [
            begin,
            instr_run(&toc.instruction()),
            fld_char(ST_FldCharType::Separate, None),
        ];
        let first = &mut paragraphs[first_entry];
        for (i, r) in head.into_iter().enumerate() {
            first.p_content.insert(i, EG_PContent::R(Box::new(r)));
        }
        let last = paragraphs.last_mut().expect("at least one entry paragraph");
        last.p_content
            .push(EG_PContent::R(Box::new(fld_char(ST_FldCharType::End, None))));
        let at = index.min(top);
        let blocks = &mut self.body_mut().block_level_elts;
        for (i, p) in paragraphs.into_iter().enumerate() {
            blocks.insert(at + i, EG_BlockLevelElts::P(Box::new(p)));
        }
        self.set_update_fields_on_open(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions() {
        assert_eq!(Field::Page.instruction(), "PAGE");
        assert_eq!(Field::NumPages.instruction(), "NUMPAGES");
        assert_eq!(
            Field::Date(Some("dd/MM/yyyy".into())).instruction(),
            r#"DATE \@ "dd/MM/yyyy""#
        );
        assert_eq!(Field::Time(None).instruction(), "TIME");
        assert_eq!(Field::PageRef("x".into()).instruction(), r#"PAGEREF x \h"#);
        assert_eq!(Field::Code("  AUTHOR ".into()).instruction(), "AUTHOR");
        assert_eq!(
            TableOfContents::default().instruction(),
            r#"TOC \o "1-3" \h \z \u"#
        );
        let plain = TableOfContents {
            hyperlinks: false,
            first_level: 2,
            last_level: 4,
            title: None,
        };
        assert_eq!(plain.instruction(), r#"TOC \o "2-4" \z \u"#);
    }

    #[test]
    fn scanner_handles_nesting_and_paragraphs() {
        let xml = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> IF </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>3</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>yes</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>9</w:t></w:r></w:fldSimple></w:p>"#;
        let doc = wml::elements::DOCUMENT
            .parse(&format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{xml}</w:body></w:document>"#
            ))
            .unwrap();
        let fields = public(scan_blocks(&doc.body.unwrap().block_level_elts));
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].instruction, "IF");
        assert_eq!(fields[0].result, "yes\ntwo");
        assert_eq!(
            (fields[1].instruction.as_str(), fields[1].result.as_str()),
            ("PAGE", "3")
        );
        assert_eq!(
            (fields[2].instruction.as_str(), fields[2].result.as_str()),
            ("NUMPAGES", "9")
        );
    }

    #[test]
    fn complex_field_runs() {
        let runs = complex_field("PAGE", "1");
        assert_eq!(runs.len(), 5);
        let empty = complex_field("PAGE", "");
        assert_eq!(empty.len(), 4);
        let EG_RunInnerContent::InstrText(t) = &runs[1].run_inner_content[0] else {
            panic!()
        };
        assert_eq!(t.value, " PAGE ");
        assert_eq!(t.xml_space.as_deref(), Some("preserve"));
    }
}
