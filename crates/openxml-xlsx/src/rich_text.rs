//! Rich text: cell text (and comments) made of runs with their own fonts
//! (`<si><r><rPr/><t/></r></si>`, ECMA-376 Part 1 §18.4.4).
//!
//! ```
//! use openxml_xlsx::{Workbook, RichText, Font, Color};
//!
//! let mut wb = Workbook::new();
//! let text = RichText::new()
//!     .push("Status: ")
//!     .bold("OK")
//!     .push_styled(" (checked)", Font { italic: true, color: Some(Color::Rgb(128, 128, 128)), ..Font::default() });
//! wb.worksheet_mut("Sheet1")?.set_rich_text("A1", &text)?;
//! let sheet = wb.worksheet("Sheet1")?;
//! assert_eq!(sheet.cell("A1")?.as_str(), Some("Status: OK (checked)"));
//! assert_eq!(sheet.rich_text("A1")?.unwrap(), text);
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::ToCellRef;
use crate::styles::{Color, Font, FontVerticalAlign, UnderlineStyle};
use crate::value::{MAX_TEXT_LEN, decode_xstring, encode_xstring};
use crate::worksheet::{Worksheet, WorksheetMut};

/// A run of text with an optional font.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// The text.
    pub text: String,
    /// Font of the run; `None` uses the cell's font.
    pub font: Option<Font>,
}

/// Text made of differently formatted runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    runs: Vec<TextRun>,
}

impl RichText {
    /// Empty text.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an unformatted run.
    pub fn push(mut self, text: impl Into<String>) -> Self {
        self.runs.push(TextRun {
            text: text.into(),
            font: None,
        });
        self
    }

    /// Appends a run with a font.
    pub fn push_styled(mut self, text: impl Into<String>, font: Font) -> Self {
        self.runs.push(TextRun {
            text: text.into(),
            font: Some(font),
        });
        self
    }

    /// Appends a bold run.
    pub fn bold(self, text: impl Into<String>) -> Self {
        self.push_styled(
            text,
            Font {
                bold: true,
                ..Font::default()
            },
        )
    }

    /// Appends an italic run.
    pub fn italic(self, text: impl Into<String>) -> Self {
        self.push_styled(
            text,
            Font {
                italic: true,
                ..Font::default()
            },
        )
    }

    /// The runs.
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// The plain text.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Whether there is no text.
    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }

    /// The typed string item.
    pub(crate) fn to_rst(&self) -> sml::CT_Rst {
        sml::CT_Rst {
            r: self
                .runs
                .iter()
                .map(|run| sml::CT_RElt {
                    r_pr: run.font.as_ref().map(|f| Box::new(run_properties(f))),
                    t: Some(encode_xstring(&run.text).into_owned()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Reads a string item (plain items become one unformatted run).
    pub(crate) fn from_rst(rst: &sml::CT_Rst) -> Self {
        let mut text = RichText::new();
        if let Some(t) = &rst.t {
            text = text.push(decode_xstring(t).into_owned());
        }
        for run in &rst.r {
            let t = decode_xstring(run.t.as_deref().unwrap_or("")).into_owned();
            text.runs.push(TextRun {
                text: t,
                font: run.r_pr.as_deref().map(font_of),
            });
        }
        text
    }
}

impl From<&str> for RichText {
    fn from(text: &str) -> Self {
        RichText::new().push(text)
    }
}

/// The run properties (`rPr`) for a font.
pub(crate) fn run_properties(f: &Font) -> sml::CT_RPrElt {
    use sml::CT_RPrElt_Choice as C;
    let flag = || Box::new(sml::CT_BooleanProperty::default());
    let mut choice = Vec::new();
    if f.bold {
        choice.push(C::B(flag()));
    }
    if f.italic {
        choice.push(C::I(flag()));
    }
    if f.strike {
        choice.push(C::Strike(flag()));
    }
    if f.underline || f.underline_style.is_some() {
        choice.push(C::U(Box::new(sml::CT_UnderlineProperty {
            val: f.underline_style.filter(|u| *u != UnderlineStyle::Single),
            ..Default::default()
        })));
    }
    if let Some(v) = f.vertical_align {
        choice.push(C::VertAlign(Box::new(sml::CT_VerticalAlignFontProperty {
            val: Some(v),
            ..Default::default()
        })));
    }
    if let Some(s) = f.size {
        choice.push(C::Sz(Box::new(sml::CT_FontSize {
            val: Some(s),
            ..Default::default()
        })));
    }
    if let Some(c) = f.color {
        choice.push(C::Color(Box::new(c.to_ct())));
    }
    if let Some(n) = &f.name {
        choice.push(C::RFont(Box::new(sml::CT_FontName {
            val: Some(n.clone()),
            ..Default::default()
        })));
    }
    sml::CT_RPrElt {
        choice,
        ..Default::default()
    }
}

/// The font described by run properties.
pub(crate) fn font_of(p: &sml::CT_RPrElt) -> Font {
    use sml::CT_RPrElt_Choice as C;
    let mut f = Font::default();
    for c in &p.choice {
        match c {
            C::B(b) => f.bold = b.val.unwrap_or(true),
            C::I(b) => f.italic = b.val.unwrap_or(true),
            C::Strike(b) => f.strike = b.val.unwrap_or(true),
            C::U(u) => {
                let val = u.val.unwrap_or(UnderlineStyle::Single);
                f.underline = val != UnderlineStyle::None;
                f.underline_style =
                    (val != UnderlineStyle::Single && val != UnderlineStyle::None).then_some(val);
            }
            C::VertAlign(v) => f.vertical_align = v.val.filter(|v| *v != FontVerticalAlign::Baseline),
            C::Sz(s) => f.size = s.val,
            C::Color(c) => f.color = Color::from_ct(c),
            C::RFont(n) => f.name = n.val.clone(),
            _ => {}
        }
    }
    f
}

impl Worksheet<'_> {
    /// The formatted text of a cell: the runs of a rich shared or inline
    /// string, a single run for plain text, `None` for other values.
    pub fn rich_text(&self, at: impl ToCellRef) -> Result<Option<RichText>> {
        let at = at.to_cell_ref()?;
        let Some(cell) = self.find_cell(at) else {
            return Ok(None);
        };
        let rst = match cell.t {
            Some(sml::ST_CellType::S) => cell
                .v
                .as_deref()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .and_then(|i| self.ctx.sst.item(i)),
            Some(sml::ST_CellType::InlineStr) => cell.is.as_deref(),
            _ => None,
        };
        Ok(rst.map(RichText::from_rst))
    }
}

impl WorksheetMut<'_> {
    /// Sets a cell to formatted text (stored in the shared string table).
    pub fn set_rich_text(&mut self, at: impl ToCellRef, text: &RichText) -> Result<()> {
        let at = at.to_cell_ref()?;
        if text.text().chars().count() > MAX_TEXT_LEN {
            return Err(Error::InvalidArgument(format!(
                "text longer than {MAX_TEXT_LEN} characters"
            )));
        }
        let index = self.sst.intern_rich(text.to_rst());
        let cell = self.cell_mut(at);
        crate::worksheet::clear_value(cell);
        cell.t = Some(sml::ST_CellType::S);
        cell.v = Some(index.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_round_trip_through_string_items() {
        let text = RichText::new().push("plain ").bold("bold").push_styled(
            " red",
            Font {
                color: Some(Color::Rgb(255, 0, 0)),
                size: Some(14.0),
                name: Some("Arial".into()),
                underline: true,
                underline_style: Some(UnderlineStyle::Double),
                vertical_align: Some(FontVerticalAlign::Superscript),
                strike: true,
                italic: true,
                ..Font::default()
            },
        );
        let rst = text.to_rst();
        let xml = sml::elements::SST.to_xml(&sml::CT_Sst {
            si: vec![rst],
            ..Default::default()
        });
        assert!(xml.contains(r#"<t xml:space="preserve">plain </t>"#), "{xml}");
        let sst = sml::elements::SST.parse(&xml).unwrap();
        assert_eq!(RichText::from_rst(&sst.si[0]), text);
        assert_eq!(text.text(), "plain bold red");
        assert!(!text.is_empty());
        assert!(RichText::new().is_empty());
        assert_eq!(RichText::from("x").runs().len(), 1);
    }
}
