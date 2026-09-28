//! Custom style definitions.

use openxml_core::{Error, Result};
use openxml_schema::shared_types::ST_OnOff;
use openxml_schema::wml::{self, ST_StyleType};

use crate::document::Document;
use crate::format::{
    ParagraphFormat, RunFormat, TableFormat, apply_paragraph_format, apply_run_properties,
    apply_table_format, convert,
};
use crate::util::{on, string_val};

/// Kind of style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleKind {
    /// Paragraph style (paragraph and character formatting).
    Paragraph,
    /// Character style (applied to runs).
    Character,
    /// Table style.
    Table,
}

/// A style to add to the styles part.
///
/// ```
/// use openxml_docx::{Alignment, Document, FontSize, ParagraphFormat, RunFormat, StyleDefinition};
///
/// let mut doc = Document::new();
/// let mut note = StyleDefinition::paragraph("Note", "Note");
/// note.paragraph = ParagraphFormat { alignment: Some(Alignment::Center), ..Default::default() };
/// note.run = RunFormat { italic: Some(true), size: Some(FontSize(9.0)), ..Default::default() };
/// doc.add_style(&note)?;
/// doc.add_paragraph("Remember to save.").set_style("Note")?;
/// assert_eq!(doc.paragraphs()[0].style_id(), Some("Note"));
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct StyleDefinition {
    /// Style id (referenced by `w:pStyle`, `w:rStyle`, `w:tblStyle`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Kind of style.
    pub kind: StyleKind,
    /// Parent style id.
    pub based_on: Option<String>,
    /// Style of the paragraph that follows (paragraph styles).
    pub next: Option<String>,
    /// Paragraph formatting (paragraph and table styles).
    pub paragraph: ParagraphFormat,
    /// Character formatting.
    pub run: RunFormat,
    /// Table formatting (table styles).
    pub table: TableFormat,
    /// Show the style in the style gallery.
    pub quick_format: bool,
}

impl StyleDefinition {
    fn new(id: &str, name: &str, kind: StyleKind, based_on: &str) -> Self {
        StyleDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
            based_on: Some(based_on.to_owned()),
            next: None,
            paragraph: ParagraphFormat::default(),
            run: RunFormat::default(),
            table: TableFormat::default(),
            quick_format: true,
        }
    }

    /// A paragraph style based on `Normal`.
    pub fn paragraph(id: &str, name: &str) -> Self {
        Self::new(id, name, StyleKind::Paragraph, "Normal")
    }

    /// A character style based on `DefaultParagraphFont`.
    pub fn character(id: &str, name: &str) -> Self {
        Self::new(id, name, StyleKind::Character, "DefaultParagraphFont")
    }

    /// A table style based on `TableNormal`.
    pub fn table(id: &str, name: &str) -> Self {
        Self::new(id, name, StyleKind::Table, "TableNormal")
    }
}

impl Document {
    /// Adds a style to the styles part, replacing a style with the same id.
    /// Built-in styles referenced by `based_on`/`next` are added when missing.
    pub fn add_style(&mut self, def: &StyleDefinition) -> Result<()> {
        if def.id.trim().is_empty() {
            return Err(Error::InvalidArgument("a style needs an id".into()));
        }
        let based_on = def
            .based_on
            .as_deref()
            .map(|b| self.shared.resolve_style(b))
            .transpose()?;
        let next = def
            .next
            .as_deref()
            .filter(|n| *n != def.id)
            .map(|n| self.shared.resolve_style(n))
            .transpose()?
            .or_else(|| def.next.clone());
        let mut style = wml::CT_Style {
            type_: Some(match def.kind {
                StyleKind::Paragraph => ST_StyleType::Paragraph,
                StyleKind::Character => ST_StyleType::Character,
                StyleKind::Table => ST_StyleType::Table,
            }),
            style_id: Some(def.id.clone()),
            custom_style: Some(ST_OnOff::Boolean(true)),
            name: Some(string_val(&def.name)),
            based_on: based_on.as_deref().map(string_val),
            next: next.as_deref().map(string_val),
            q_format: def.quick_format.then(on),
            ..Default::default()
        };
        if def.kind != StyleKind::Character && def.paragraph != ParagraphFormat::default() {
            let mut ppr = wml::CT_PPr::default();
            apply_paragraph_format(&mut ppr, &def.paragraph)?;
            style.p_pr = Some(Box::new(convert(&ppr, "pPr")?));
        }
        apply_run_properties(&mut style.r_pr, &def.run)?;
        if def.kind == StyleKind::Table && def.table != TableFormat::default() {
            let mut tpr = wml::CT_TblPr::default();
            apply_table_format(&mut tpr, &def.table)?;
            style.tbl_pr = Some(Box::new(convert(&tpr, "tblPr")?));
        }
        let styles = self.shared.styles_mut()?;
        match styles
            .style
            .iter_mut()
            .find(|s| s.style_id.as_deref() == Some(def.id.as_str()))
        {
            Some(existing) => *existing = style,
            None => styles.style.push(style),
        }
        Ok(())
    }

    /// The definition of a style in the styles part.
    pub fn style(&self, style_id: &str) -> Option<&wml::CT_Style> {
        self.styles()?
            .style
            .iter()
            .find(|s| s.style_id.as_deref() == Some(style_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Border, FontSize, TableBorders};

    #[test]
    fn styles_are_added_and_replaced() {
        let mut doc = Document::new();
        let mut def = StyleDefinition::character("Code", "Code");
        def.run = RunFormat {
            font: Some("Consolas".into()),
            size: Some(FontSize(10.0)),
            ..Default::default()
        };
        doc.add_style(&def).unwrap();
        let style = doc.style("Code").unwrap();
        assert_eq!(style.type_, Some(ST_StyleType::Character));
        assert_eq!(
            style.based_on.as_ref().unwrap().val.as_deref(),
            Some("DefaultParagraphFont")
        );
        assert!(style.p_pr.is_none());
        assert_eq!(style.r_pr.as_ref().unwrap().r_pr_base.len(), 3);

        def.run.size = None;
        def.name = "Code text".into();
        doc.add_style(&def).unwrap();
        let count = doc.style_ids().iter().filter(|id| **id == "Code").count();
        assert_eq!(count, 1);
        assert_eq!(
            doc.style("Code").unwrap().name.as_ref().unwrap().val.as_deref(),
            Some("Code text")
        );

        let mut table = StyleDefinition::table("Grid2", "Grid 2");
        table.table.borders = Some(TableBorders::all(Border::single(0.5, "808080")));
        doc.add_style(&table).unwrap();
        assert!(
            doc.style("Grid2")
                .unwrap()
                .tbl_pr
                .as_ref()
                .unwrap()
                .tbl_borders
                .is_some()
        );
        assert!(doc.style_ids().contains(&"TableNormal"));

        let mut heading = StyleDefinition::paragraph("MyHeading", "My heading");
        heading.based_on = Some("Heading1".into());
        heading.next = Some("Normal".into());
        doc.add_style(&heading).unwrap();
        assert!(doc.style_ids().contains(&"Heading1"));
        assert!(doc.add_style(&StyleDefinition::paragraph(" ", "blank")).is_err());
    }
}
