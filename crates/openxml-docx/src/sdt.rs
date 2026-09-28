//! Content controls (structured document tags, `w:sdt`) and legacy form
//! check boxes (`FORMCHECKBOX` fields, the ECMA-376 way to put a check box
//! in a document; Word's `w14:checkbox` control is an extension).

use openxml_core::{Error, Result};
use openxml_schema::shared_types::ST_OnOff;
use openxml_schema::wml::{
    self, CT_FFData_Choice, CT_SdtPr_Choice, EG_BlockLevelElts, EG_ContentBlockContent, EG_PContent,
    EG_RunInnerContent, ST_FldCharType,
};

use crate::document::Document;
use crate::fields::{fld_char, instr_run, scan_blocks};
use crate::paragraph::ParagraphMut;
use crate::run::RunMut;
use crate::util::{is_on, on, on_off_value, string_val};
use crate::{text, walk};

/// An entry of a drop-down list or combo box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// Text shown to the user.
    pub display: String,
    /// Value stored when the entry is chosen.
    pub value: String,
}

impl ListItem {
    /// An entry.
    pub fn new(display: &str, value: &str) -> Self {
        ListItem {
            display: display.to_owned(),
            value: value.to_owned(),
        }
    }
}

/// Type of a content control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentControlKind {
    /// Plain text (no formatting changes inside).
    PlainText {
        /// Allow line breaks.
        multi_line: bool,
    },
    /// Rich text.
    RichText,
    /// Choice from a fixed list.
    DropDown(Vec<ListItem>),
    /// Choice from a list or free text.
    ComboBox(Vec<ListItem>),
    /// Date picker with a display format such as `dd/MM/yyyy`.
    Date {
        /// Display format (Word date picture).
        format: String,
    },
}

/// A content control to insert.
///
/// ```
/// use openxml_docx::{ContentControl, ContentControlKind, Document, ListItem};
///
/// let mut doc = Document::new();
/// let status = ContentControl::new(
///     ContentControlKind::DropDown(vec![ListItem::new("Open", "open"), ListItem::new("Closed", "closed")]),
///     "status",
/// );
/// doc.add_paragraph("Status: ").add_content_control(&status, "")?;
/// doc.set_content_control_value("status", "closed")?;
/// let controls = doc.content_controls();
/// assert_eq!(controls[0].text, "Closed");
/// assert_eq!(controls[0].value.as_deref(), Some("closed"));
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentControl {
    /// Type and options.
    pub kind: ContentControlKind,
    /// Tag used by programs to find the control.
    pub tag: Option<String>,
    /// Title shown to the user (`w:alias`).
    pub title: Option<String>,
    /// Text shown while the control is empty.
    pub placeholder: String,
}

impl ContentControl {
    /// A control of `kind` with `tag` (also used as title).
    pub fn new(kind: ContentControlKind, tag: &str) -> Self {
        ContentControl {
            kind,
            tag: Some(tag.to_owned()),
            title: Some(tag.to_owned()),
            placeholder: "Click here to enter text.".into(),
        }
    }
}

/// Type of a content control found in a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentControlType {
    /// Plain text.
    PlainText,
    /// Rich text (also controls without a type).
    RichText,
    /// Drop-down list.
    DropDown,
    /// Combo box.
    ComboBox,
    /// Date picker.
    Date,
    /// Picture.
    Picture,
    /// Another type (equation, citation, building block, group, extension).
    Other,
}

/// A content control read from a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentControlInfo {
    /// Identifier.
    pub id: Option<i64>,
    /// Tag.
    pub tag: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Type.
    pub kind: ContentControlType,
    /// Displayed text.
    pub text: String,
    /// Stored value: the chosen list value or the full date.
    pub value: Option<String>,
    /// Entries of a list.
    pub items: Vec<ListItem>,
    /// Whether the placeholder is shown.
    pub showing_placeholder: bool,
    /// Whether the control contains paragraphs (block-level) rather than runs.
    pub block: bool,
}

/// A legacy form check box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkbox {
    /// Form field name.
    pub name: String,
    /// Whether it is checked.
    pub checked: bool,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Parses `YYYY-MM-DD` (optionally followed by a time).
fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let date = s.get(..10)?;
    let mut parts = date.split('-');
    let y = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d) && date.as_bytes()[4] == b'-').then_some((y, m, d))
}

/// Formats a date with a Word date picture (`yyyy`, `yy`, `MMMM`, `MMM`,
/// `MM`, `M`, `dd`, `d`; other characters are copied).
fn format_date(format: &str, (y, m, d): (i32, u32, u32)) -> String {
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let run = chars[i..].iter().take_while(|&&x| x == c).count();
        match (c, run) {
            ('y', n) if n >= 4 => out.push_str(&format!("{y:04}")),
            ('y', _) => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            ('M', n) if n >= 4 => out.push_str(MONTHS[m as usize - 1]),
            ('M', 3) => out.push_str(&MONTHS[m as usize - 1][..3]),
            ('M', 2) => out.push_str(&format!("{m:02}")),
            ('M', _) => out.push_str(&m.to_string()),
            ('d', n) if n >= 2 => out.push_str(&format!("{d:02}")),
            ('d', _) => out.push_str(&d.to_string()),
            _ => {
                for _ in 0..run {
                    out.push(c);
                }
            }
        }
        i += run;
    }
    out
}

fn list_items(items: &[ListItem]) -> Vec<wml::CT_SdtListItem> {
    items
        .iter()
        .map(|i| wml::CT_SdtListItem {
            display_text: Some(i.display.clone()),
            value_attr: Some(i.value.clone()),
            ..Default::default()
        })
        .collect()
}

fn read_items(items: &[wml::CT_SdtListItem]) -> Vec<ListItem> {
    items
        .iter()
        .map(|i| ListItem {
            display: i.display_text.clone().unwrap_or_default(),
            value: i.value_attr.clone().unwrap_or_default(),
        })
        .collect()
}

/// Content control properties for a new control; the displayed text is
/// resolved from `value` (a list value or display text, or an ISO date).
fn properties(control: &ContentControl, id: i64, value: &str) -> Result<(wml::CT_SdtPr, String)> {
    let (choice, text) = match &control.kind {
        ContentControlKind::PlainText { multi_line } => (
            CT_SdtPr_Choice::Text(Box::new(wml::CT_SdtText {
                multi_line: multi_line.then_some(ST_OnOff::Boolean(true)),
                ..Default::default()
            })),
            value.to_owned(),
        ),
        ContentControlKind::RichText => (CT_SdtPr_Choice::RichText(Box::default()), value.to_owned()),
        ContentControlKind::DropDown(items) | ContentControlKind::ComboBox(items) => {
            let chosen = items.iter().find(|i| i.value == value || i.display == value);
            let combo = matches!(control.kind, ContentControlKind::ComboBox(_));
            if !value.is_empty() && chosen.is_none() && !combo {
                return Err(Error::InvalidArgument(format!(
                    "{value:?} is not an entry of the list"
                )));
            }
            let list = || {
                (
                    chosen
                        .map(|c| c.value.clone())
                        .or_else(|| (!value.is_empty()).then(|| value.to_owned())),
                    list_items(items),
                )
            };
            let (last_value, list_item) = list();
            let text = chosen.map_or_else(|| value.to_owned(), |c| c.display.clone());
            let choice = if combo {
                CT_SdtPr_Choice::ComboBox(Box::new(wml::CT_SdtComboBox {
                    last_value,
                    list_item,
                    ..Default::default()
                }))
            } else {
                CT_SdtPr_Choice::DropDownList(Box::new(wml::CT_SdtDropDownList {
                    last_value,
                    list_item,
                    ..Default::default()
                }))
            };
            (choice, text)
        }
        ContentControlKind::Date { format } => {
            let date =
                if value.is_empty() {
                    None
                } else {
                    Some(parse_date(value).ok_or_else(|| {
                        Error::InvalidArgument(format!("{value:?} is not a date (YYYY-MM-DD)"))
                    })?)
                };
            (
                CT_SdtPr_Choice::Date(Box::new(wml::CT_SdtDate {
                    full_date: date.map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}T00:00:00Z")),
                    date_format: Some(string_val(format)),
                    lid: Some(Box::new(wml::CT_Lang {
                        val: Some("en-US".into()),
                        ..Default::default()
                    })),
                    store_mapped_data_as: Some(Box::new(wml::CT_SdtDateMappingType {
                        val: Some(wml::ST_SdtDateMappingType::DateTime),
                        ..Default::default()
                    })),
                    calendar: Some(Box::new(wml::CT_CalendarType {
                        val: Some(openxml_schema::shared_types::ST_CalendarType::Gregorian),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                date.map(|d| format_date(format, d)).unwrap_or_default(),
            )
        }
    };
    let pr = wml::CT_SdtPr {
        alias: control.title.as_deref().map(string_val),
        tag: control.tag.as_deref().map(string_val),
        id: Some(crate::util::decimal(id)),
        showing_plc_hdr: text.is_empty().then(on),
        choice: Some(choice),
        ..Default::default()
    };
    let shown = if text.is_empty() {
        control.placeholder.clone()
    } else {
        text
    };
    Ok((pr, shown))
}

impl ParagraphMut<'_> {
    /// Appends an inline content control showing `value` (a list entry's
    /// value or display text, an ISO date `YYYY-MM-DD`, or text); the
    /// placeholder is shown when `value` is empty. Returns the control id.
    pub fn add_content_control(&mut self, control: &ContentControl, value: &str) -> Result<i64> {
        let id = self.shared.new_id();
        let (pr, text) = properties(control, id, value)?;
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).add_text(&text);
        self.p.p_content.push(EG_PContent::Sdt(Box::new(wml::CT_SdtRun {
            sdt_pr: Some(Box::new(pr)),
            sdt_content: Some(Box::new(wml::CT_SdtContentRun {
                p_content: vec![EG_PContent::R(Box::new(r))],
                ..Default::default()
            })),
            ..Default::default()
        })));
        Ok(id)
    }

    /// Appends a legacy form check box (a `FORMCHECKBOX` field with form
    /// field data, bookmarked with its name as Word does).
    pub fn add_checkbox(&mut self, name: &str, checked: bool) -> Result<&mut Self> {
        if name.is_empty() || name.chars().count() > 20 || name.chars().any(char::is_whitespace) {
            return Err(Error::InvalidArgument(format!(
                "invalid check box name {name:?}: 1 to 20 characters without spaces"
            )));
        }
        let form = wml::CT_FFData {
            choice: vec![
                CT_FFData_Choice::Name(Box::new(wml::CT_FFName {
                    val: Some(name.to_owned()),
                    ..Default::default()
                })),
                CT_FFData_Choice::Enabled(on()),
                CT_FFData_Choice::CalcOnExit(crate::util::off()),
                CT_FFData_Choice::CheckBox(Box::new(wml::CT_FFCheckBox {
                    choice: Some(wml::CT_FFCheckBox_Choice::SizeAuto(on())),
                    default: Some(if checked { on() } else { crate::util::off() }),
                    ..Default::default()
                })),
            ],
            ..Default::default()
        };
        let id = self.shared.new_id();
        let content = &mut self.p.p_content;
        content.push(EG_PContent::BookmarkStart(Box::new(wml::CT_Bookmark {
            id: Some(id),
            name: Some(name.to_owned()),
            ..Default::default()
        })));
        for r in [
            fld_char(ST_FldCharType::Begin, Some(form)),
            instr_run("FORMCHECKBOX"),
            fld_char(ST_FldCharType::End, None),
        ] {
            content.push(EG_PContent::R(Box::new(r)));
        }
        content.push(EG_PContent::BookmarkEnd(Box::new(wml::CT_MarkupRange {
            id: Some(id),
            ..Default::default()
        })));
        Ok(self)
    }
}

/// A content control being visited.
enum SdtMut<'a> {
    Block(&'a mut wml::CT_SdtBlock),
    Run(&'a mut wml::CT_SdtRun),
}

fn visit_content(items: &mut [EG_PContent], f: &mut dyn FnMut(SdtMut<'_>)) {
    for item in items {
        match item {
            EG_PContent::Sdt(s) => {
                f(SdtMut::Run(s));
                if let Some(c) = s.sdt_content.as_mut() {
                    visit_content(&mut c.p_content, f);
                }
            }
            EG_PContent::Hyperlink(h) => visit_content(&mut h.p_content, f),
            EG_PContent::CustomXml(x) => visit_content(&mut x.p_content, f),
            EG_PContent::SmartTag(x) => visit_content(&mut x.p_content, f),
            EG_PContent::FldSimple(x) => visit_content(&mut x.p_content, f),
            _ => {}
        }
    }
}

fn visit_block_content(items: &mut [EG_ContentBlockContent], f: &mut dyn FnMut(SdtMut<'_>)) {
    for item in items {
        match item {
            EG_ContentBlockContent::Sdt(s) => {
                f(SdtMut::Block(s));
                if let Some(c) = s.sdt_content.as_mut() {
                    visit_block_content(&mut c.content_block_content, f);
                }
            }
            EG_ContentBlockContent::P(p) => visit_content(&mut p.p_content, f),
            EG_ContentBlockContent::Tbl(t) => visit_table(t, f),
            EG_ContentBlockContent::CustomXml(x) => visit_block_content(&mut x.content_block_content, f),
            _ => {}
        }
    }
}

fn visit_table(t: &mut wml::CT_Tbl, f: &mut dyn FnMut(SdtMut<'_>)) {
    for row in text::rows_mut(t) {
        for cell in text::cells_mut(row) {
            visit_blocks(&mut cell.block_level_elts, f);
        }
    }
}

fn visit_blocks(items: &mut [EG_BlockLevelElts], f: &mut dyn FnMut(SdtMut<'_>)) {
    for item in items {
        match item {
            EG_BlockLevelElts::Sdt(s) => {
                f(SdtMut::Block(s));
                if let Some(c) = s.sdt_content.as_mut() {
                    visit_block_content(&mut c.content_block_content, f);
                }
            }
            EG_BlockLevelElts::P(p) => visit_content(&mut p.p_content, f),
            EG_BlockLevelElts::Tbl(t) => visit_table(t, f),
            EG_BlockLevelElts::CustomXml(x) => visit_block_content(&mut x.content_block_content, f),
            _ => {}
        }
    }
}

fn info(pr: Option<&wml::CT_SdtPr>, text: String, block: bool) -> ContentControlInfo {
    let (kind, value, items) = match pr.and_then(|p| p.choice.as_ref()) {
        Some(CT_SdtPr_Choice::Text(_)) => (ContentControlType::PlainText, None, Vec::new()),
        None | Some(CT_SdtPr_Choice::RichText(_)) => (ContentControlType::RichText, None, Vec::new()),
        Some(CT_SdtPr_Choice::DropDownList(l)) => (
            ContentControlType::DropDown,
            l.last_value.clone(),
            read_items(&l.list_item),
        ),
        Some(CT_SdtPr_Choice::ComboBox(l)) => (
            ContentControlType::ComboBox,
            l.last_value.clone(),
            read_items(&l.list_item),
        ),
        Some(CT_SdtPr_Choice::Date(d)) => (ContentControlType::Date, d.full_date.clone(), Vec::new()),
        Some(CT_SdtPr_Choice::Picture(_)) => (ContentControlType::Picture, None, Vec::new()),
        Some(_) => (ContentControlType::Other, None, Vec::new()),
    };
    ContentControlInfo {
        id: pr.and_then(|p| p.id.as_deref()).and_then(|i| i.val),
        tag: pr.and_then(|p| p.tag.as_deref()).and_then(|t| t.val.clone()),
        title: pr.and_then(|p| p.alias.as_deref()).and_then(|t| t.val.clone()),
        kind,
        text,
        value,
        items,
        showing_placeholder: pr.is_some_and(|p| is_on(&p.showing_plc_hdr)),
        block,
    }
}

/// Replaces the runs of inline content with one run showing `text`,
/// keeping the formatting of the first run.
fn set_run_content(content: &mut Vec<EG_PContent>, value: &str) {
    let r_pr = text::runs(content).first().and_then(|r| r.r_pr.clone());
    let mut r = wml::CT_R {
        r_pr,
        ..Default::default()
    };
    RunMut::new(&mut r).add_text(value);
    *content = vec![EG_PContent::R(Box::new(r))];
}

/// Displayed text for a new value, updating the control properties.
fn apply_value(pr: &mut wml::CT_SdtPr, value: &str) -> Result<String> {
    let text = match pr.choice.as_mut() {
        Some(CT_SdtPr_Choice::DropDownList(l)) => {
            let item = l
                .list_item
                .iter()
                .find(|i| i.value_attr.as_deref() == Some(value) || i.display_text.as_deref() == Some(value))
                .ok_or_else(|| Error::InvalidArgument(format!("{value:?} is not an entry of the list")))?;
            let shown = item
                .display_text
                .clone()
                .or_else(|| item.value_attr.clone())
                .unwrap_or_default();
            l.last_value = item.value_attr.clone();
            shown
        }
        Some(CT_SdtPr_Choice::ComboBox(l)) => {
            match l
                .list_item
                .iter()
                .find(|i| i.value_attr.as_deref() == Some(value) || i.display_text.as_deref() == Some(value))
            {
                Some(item) => {
                    l.last_value = item.value_attr.clone();
                    item.display_text.clone().unwrap_or_else(|| value.to_owned())
                }
                None => {
                    l.last_value = Some(value.to_owned());
                    value.to_owned()
                }
            }
        }
        Some(CT_SdtPr_Choice::Date(d)) => {
            let date = parse_date(value)
                .ok_or_else(|| Error::InvalidArgument(format!("{value:?} is not a date (YYYY-MM-DD)")))?;
            d.full_date = Some(format!("{:04}-{:02}-{:02}T00:00:00Z", date.0, date.1, date.2));
            let format = d
                .date_format
                .as_deref()
                .and_then(|f| f.val.clone())
                .unwrap_or_else(|| "yyyy-MM-dd".into());
            format_date(&format, date)
        }
        Some(CT_SdtPr_Choice::Text(_) | CT_SdtPr_Choice::RichText(_)) | None => value.to_owned(),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "this kind of content control has no text value".into(),
            ));
        }
    };
    pr.showing_plc_hdr = None;
    Ok(text)
}

impl Document {
    /// Appends a block-level content control holding one paragraph with
    /// `value` (see [`ParagraphMut::add_content_control`]). Returns its id.
    pub fn add_block_content_control(&mut self, control: &ContentControl, value: &str) -> Result<i64> {
        let id = self.shared.new_id();
        let (pr, text) = properties(control, id, value)?;
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).add_text(&text);
        let p = wml::CT_P {
            p_content: vec![EG_PContent::R(Box::new(r))],
            ..Default::default()
        };
        self.body_mut()
            .block_level_elts
            .push(EG_BlockLevelElts::Sdt(Box::new(wml::CT_SdtBlock {
                sdt_pr: Some(Box::new(pr)),
                sdt_content: Some(Box::new(wml::CT_SdtContentBlock {
                    content_block_content: vec![EG_ContentBlockContent::P(Box::new(p))],
                    ..Default::default()
                })),
                ..Default::default()
            })));
        Ok(id)
    }

    /// Content controls of the body (block-level and inline) in document order.
    pub fn content_controls(&self) -> Vec<ContentControlInfo> {
        let mut body = self.body().clone();
        let mut out = Vec::new();
        visit_blocks(&mut body.block_level_elts, &mut |sdt| match sdt {
            SdtMut::Block(b) => {
                let text = b
                    .sdt_content
                    .as_deref()
                    .map(|c| {
                        let mut blocks = Vec::new();
                        for item in &c.content_block_content {
                            match item {
                                EG_ContentBlockContent::P(p) => blocks.push(text::BlockRef::P(p)),
                                EG_ContentBlockContent::Tbl(t) => blocks.push(text::BlockRef::Tbl(t)),
                                _ => {}
                            }
                        }
                        text::blocks_text(&blocks)
                    })
                    .unwrap_or_default();
                out.push(info(b.sdt_pr.as_deref(), text, true));
            }
            SdtMut::Run(r) => {
                let text = r
                    .sdt_content
                    .as_deref()
                    .map(|c| {
                        text::paragraph_text(&wml::CT_P {
                            p_content: c.p_content.clone(),
                            ..Default::default()
                        })
                    })
                    .unwrap_or_default();
                out.push(info(r.sdt_pr.as_deref(), text, false));
            }
        });
        out
    }

    /// Sets the value of every content control tagged `tag`: text for text
    /// controls, an entry (value or display text) for lists, an ISO date
    /// (`YYYY-MM-DD`) for date pickers. Returns the number of controls set.
    pub fn set_content_control_value(&mut self, tag: &str, value: &str) -> Result<usize> {
        let mut count = 0;
        let mut error = None;
        let Some(body) = self.main.body.as_mut() else {
            return Err(Error::NotFound(format!("content control {tag:?}")));
        };
        visit_blocks(&mut body.block_level_elts, &mut |sdt| {
            if error.is_some() {
                return;
            }
            let pr = match &sdt {
                SdtMut::Block(b) => b.sdt_pr.as_deref(),
                SdtMut::Run(r) => r.sdt_pr.as_deref(),
            };
            let tagged = pr.and_then(|p| p.tag.as_deref()).and_then(|t| t.val.as_deref()) == Some(tag);
            if !tagged {
                return;
            }
            match sdt {
                SdtMut::Run(r) => {
                    let pr = r.sdt_pr.get_or_insert_with(Default::default);
                    match apply_value(pr, value) {
                        Ok(text) => {
                            let content = r.sdt_content.get_or_insert_with(Default::default);
                            set_run_content(&mut content.p_content, &text);
                            count += 1;
                        }
                        Err(e) => error = Some(e),
                    }
                }
                SdtMut::Block(b) => {
                    let pr = b.sdt_pr.get_or_insert_with(Default::default);
                    match apply_value(pr, value) {
                        Ok(text) => {
                            let content = b.sdt_content.get_or_insert_with(Default::default);
                            // Keep the first paragraph (and its properties), drop the rest.
                            let mut first = content
                                .content_block_content
                                .iter()
                                .find_map(|c| match c {
                                    EG_ContentBlockContent::P(p) => Some((**p).clone()),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            set_run_content(&mut first.p_content, &text);
                            content.content_block_content = vec![EG_ContentBlockContent::P(Box::new(first))];
                            count += 1;
                        }
                        Err(e) => error = Some(e),
                    }
                }
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
        if count == 0 {
            return Err(Error::NotFound(format!("content control {tag:?}")));
        }
        self.main_dirty = true;
        Ok(count)
    }

    /// Legacy form check boxes of the body in document order.
    pub fn checkboxes(&self) -> Vec<Checkbox> {
        scan_blocks(&self.body().block_level_elts)
            .into_iter()
            .filter_map(|f| {
                let form = f.form?;
                let mut name = String::new();
                let mut checked = None;
                for c in &form.choice {
                    match c {
                        CT_FFData_Choice::Name(n) => name = n.val.clone().unwrap_or_default(),
                        CT_FFData_Choice::CheckBox(b) => {
                            checked = Some(
                                b.checked
                                    .as_deref()
                                    .or(b.default.as_deref())
                                    .is_some_and(on_off_value),
                            );
                        }
                        _ => {}
                    }
                }
                Some(Checkbox {
                    name,
                    checked: checked?,
                })
            })
            .collect()
    }

    /// Checks or unchecks the form check boxes named `name` (`w:checked`).
    /// Returns the number of check boxes changed.
    pub fn set_checkbox(&mut self, name: &str, checked: bool) -> Result<usize> {
        let mut count = 0;
        let Some(body) = self.main.body.as_mut() else {
            return Err(Error::NotFound(format!("check box {name:?}")));
        };
        walk::walk_blocks(&mut body.block_level_elts, &mut |p| {
            for r in text::runs_mut(&mut p.p_content) {
                for c in &mut r.run_inner_content {
                    let EG_RunInnerContent::FldChar(f) = c else {
                        continue;
                    };
                    let Some(wml::CT_FldChar_Choice::FfData(data)) = f.choice.as_mut() else {
                        continue;
                    };
                    let named = data
                        .choice
                        .iter()
                        .any(|c| matches!(c, CT_FFData_Choice::Name(n) if n.val.as_deref() == Some(name)));
                    if !named {
                        continue;
                    }
                    for c in &mut data.choice {
                        if let CT_FFData_Choice::CheckBox(b) = c {
                            b.checked = Some(if checked { on() } else { crate::util::off() });
                            count += 1;
                        }
                    }
                }
            }
        });
        if count == 0 {
            return Err(Error::NotFound(format!("check box {name:?}")));
        }
        self.main_dirty = true;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_parse_and_format() {
        assert_eq!(parse_date("2024-03-09"), Some((2024, 3, 9)));
        assert_eq!(parse_date("2024-03-09T10:00:00Z"), Some((2024, 3, 9)));
        assert_eq!(parse_date("2024-13-09"), None);
        assert_eq!(parse_date("2024/03/09"), None);
        assert_eq!(parse_date("x"), None);
        let d = (2024, 3, 9);
        assert_eq!(format_date("dd/MM/yyyy", d), "09/03/2024");
        assert_eq!(format_date("d MMMM yy", d), "9 March 24");
        assert_eq!(format_date("MMM d, yyyy", d), "Mar 9, 2024");
        assert_eq!(format_date("M.d", d), "3.9");
    }

    #[test]
    fn properties_by_kind() {
        let items = vec![ListItem::new("One", "1"), ListItem::new("Two", "2")];
        let drop = ContentControl::new(ContentControlKind::DropDown(items.clone()), "n");
        let (pr, text) = properties(&drop, 5, "2").unwrap();
        assert_eq!(text, "Two");
        assert!(pr.showing_plc_hdr.is_none());
        assert!(properties(&drop, 5, "3").is_err());
        let (pr, text) = properties(&drop, 5, "").unwrap();
        assert_eq!(text, drop.placeholder);
        assert!(pr.showing_plc_hdr.is_some());
        let combo = ContentControl::new(ContentControlKind::ComboBox(items), "c");
        assert_eq!(properties(&combo, 1, "free").unwrap().1, "free");
        let date = ContentControl::new(
            ContentControlKind::Date {
                format: "dd.MM.yyyy".into(),
            },
            "d",
        );
        let (pr, text) = properties(&date, 1, "2025-12-31").unwrap();
        assert_eq!(text, "31.12.2025");
        let Some(CT_SdtPr_Choice::Date(d)) = &pr.choice else {
            panic!()
        };
        assert_eq!(d.full_date.as_deref(), Some("2025-12-31T00:00:00Z"));
        assert!(properties(&date, 1, "tomorrow").is_err());
        let text = ContentControl::new(ContentControlKind::PlainText { multi_line: true }, "t");
        let (pr, _) = properties(&text, 1, "x").unwrap();
        assert!(matches!(pr.choice, Some(CT_SdtPr_Choice::Text(ref t)) if t.multi_line.is_some()));
    }

    #[test]
    fn checkboxes_round_trip_in_memory() {
        let mut doc = Document::new();
        doc.add_paragraph("Agree ").add_checkbox("Agree", false).unwrap();
        doc.add_paragraph("Subscribe ").add_checkbox("Sub", true).unwrap();
        assert!(doc.add_paragraph("").add_checkbox("bad name", true).is_err());
        assert_eq!(
            doc.checkboxes(),
            [
                Checkbox {
                    name: "Agree".into(),
                    checked: false
                },
                Checkbox {
                    name: "Sub".into(),
                    checked: true
                }
            ]
        );
        assert_eq!(doc.set_checkbox("Agree", true).unwrap(), 1);
        assert!(doc.checkboxes()[0].checked);
        assert!(doc.set_checkbox("None", true).is_err());
        assert_eq!(doc.fields()[0].instruction, "FORMCHECKBOX");
        assert_eq!(doc.bookmarks()[0].name, "Agree");
    }
}
