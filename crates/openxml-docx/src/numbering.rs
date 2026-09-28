//! Bulleted and numbered lists (the numbering definitions part).

use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::wml::{self, ST_NumberFormat};

use crate::document::{Document, Shared, Typed};
use crate::paragraph::ParagraphMut;
use crate::util::{decimal, signed_twips, string_val, twips};

/// Kind of list created by [`crate::Document::add_list_item`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListKind {
    /// Bulleted list (•, ◦, ▪ by level).
    Bullet,
    /// Numbered list (1., a., i. by level).
    Numbered,
}

const BULLETS: [&str; 3] = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
const NUMBER_FORMATS: [&str; 3] = ["decimal", "lowerLetter", "lowerRoman"];

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// Builds a nine-level abstract numbering definition.
pub(crate) fn abstract_num(id: i64, kind: ListKind) -> wml::CT_AbstractNum {
    let mut levels = String::new();
    for level in 0..9usize {
        let (fmt, text) = match kind {
            ListKind::Bullet => ("bullet".to_owned(), BULLETS[level % 3].to_owned()),
            ListKind::Numbered => (NUMBER_FORMATS[level % 3].to_owned(), format!("%{}.", level + 1)),
        };
        levels.push_str(&format!(
            r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
            escape(&text),
            720 * (level + 1)
        ));
    }
    let xml = format!(
        r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="{id}"><w:multiLevelType w:val="hybridMultilevel"/>{levels}</w:abstractNum></w:numbering>"#
    );
    let mut numbering = wml::elements::NUMBERING
        .parse(&xml)
        .expect("the list template is valid");
    numbering.abstract_num.remove(0)
}

impl Shared {
    pub(crate) fn numbering_mut(&mut self) -> Result<&mut wml::CT_Numbering> {
        if self.numbering.is_none() {
            let default = PartName::new("/word/numbering.xml")?;
            let name = if self.package.contains(&default) {
                self.package.next_part_name("/word/numbering{}.xml")?
            } else {
                default
            };
            self.package
                .add_part(name.clone(), ct::WML_NUMBERING, Vec::new())?;
            self.package
                .add_relationship(Some(&self.main_part), rel_types::NUMBERING, &name)?;
            self.numbering = Some(Typed {
                name,
                value: wml::CT_Numbering::default(),
                dirty: true,
            });
        }
        let n = self.numbering.as_mut().expect("created above");
        n.dirty = true;
        Ok(&mut n.value)
    }

    fn cached_list(&mut self, kind: ListKind) -> &mut Option<i64> {
        match kind {
            ListKind::Bullet => &mut self.bullet_list,
            ListKind::Numbered => &mut self.numbered_list,
        }
    }

    /// Adds a `w:num` instance for `abstract_id`; with `restart` the first level restarts at 1.
    pub(crate) fn add_num(numbering: &mut wml::CT_Numbering, abstract_id: i64, restart: bool) -> i64 {
        let num_id = numbering.num.iter().filter_map(|n| n.num_id).max().unwrap_or(0) + 1;
        let lvl_override = if restart {
            vec![wml::CT_NumLvl {
                ilvl: Some(0),
                start_override: Some(decimal(1)),
                ..Default::default()
            }]
        } else {
            Vec::new()
        };
        numbering.num.push(wml::CT_Num {
            num_id: Some(num_id),
            abstract_num_id: Some(decimal(abstract_id)),
            lvl_override,
            ..Default::default()
        });
        num_id
    }

    /// Numbering instance id used for list items of `kind`, created on first use.
    pub(crate) fn list_num_id(&mut self, kind: ListKind) -> Result<i64> {
        if let Some(id) = *self.cached_list(kind) {
            return Ok(id);
        }
        let numbering = self.numbering_mut()?;
        let abstract_id = numbering
            .abstract_num
            .iter()
            .filter_map(|a| a.abstract_num_id)
            .max()
            .map_or(0, |m| m + 1);
        numbering.abstract_num.push(abstract_num(abstract_id, kind));
        let num_id = Self::add_num(numbering, abstract_id, false);
        *self.cached_list(kind) = Some(num_id);
        Ok(num_id)
    }

    /// Makes the next list item of `kind` start a new list.
    pub(crate) fn restart_list(&mut self, kind: ListKind) -> Result<()> {
        let Some(current) = *self.cached_list(kind) else {
            // Nothing to restart: the first item will create a fresh list anyway.
            return Ok(());
        };
        let numbering = self.numbering_mut()?;
        let abstract_id = numbering
            .num
            .iter()
            .find(|n| n.num_id == Some(current))
            .and_then(|n| n.abstract_num_id.as_ref())
            .and_then(|a| a.val)
            .unwrap_or(0);
        let num_id = Self::add_num(numbering, abstract_id, true);
        *self.cached_list(kind) = Some(num_id);
        Ok(())
    }
}

/// One level of a list definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevel {
    /// Number format (`Bullet` for bullets).
    pub format: ST_NumberFormat,
    /// Level text: `%1.` style placeholders for numbers (`%2` is the number
    /// of level 2), or the bullet character.
    pub text: String,
    /// First number.
    pub start: u32,
    /// Left indentation of the text.
    pub indent: Length,
    /// Hanging indentation of the number or bullet.
    pub hanging: Length,
    /// Font of the number or bullet (e.g. `Symbol` or `Wingdings`).
    pub font: Option<String>,
    /// Paragraph style linked to the level (e.g. `Heading1` in an outline).
    pub style: Option<String>,
}

impl ListLevel {
    fn indent_for(level: u8) -> Length {
        Length::twips(720 * (i64::from(level) + 1))
    }

    /// A bullet level with the usual indentation for `level` (0–8).
    pub fn bullet(bullet: char, level: u8) -> Self {
        ListLevel {
            format: ST_NumberFormat::Bullet,
            text: bullet.to_string(),
            start: 1,
            indent: Self::indent_for(level),
            hanging: Length::twips(360),
            font: None,
            style: None,
        }
    }

    /// A numbered level: `text` such as `%1.` or `(%2)`.
    pub fn numbered(format: ST_NumberFormat, text: &str, level: u8) -> Self {
        ListLevel {
            format,
            text: text.to_owned(),
            start: 1,
            indent: Self::indent_for(level),
            hanging: Length::twips(360),
            font: None,
            style: None,
        }
    }

    fn to_ct(&self, ilvl: usize) -> wml::CT_Lvl {
        let fonts = self.font.as_ref().map(|f| wml::CT_RPr {
            r_pr_base: vec![wml::EG_RPrBase::RFonts(Box::new(wml::CT_Fonts {
                ascii: Some(f.clone()),
                h_ansi: Some(f.clone()),
                hint: Some(wml::ST_Hint::Default),
                ..Default::default()
            }))],
            ..Default::default()
        });
        wml::CT_Lvl {
            ilvl: Some(ilvl as i64),
            start: Some(decimal(i64::from(self.start))),
            num_fmt: Some(Box::new(wml::CT_NumFmt {
                val: Some(self.format),
                ..Default::default()
            })),
            p_style: self.style.as_deref().map(string_val),
            lvl_text: Some(Box::new(wml::CT_LevelText {
                val: Some(self.text.clone()),
                ..Default::default()
            })),
            lvl_jc: Some(Box::new(wml::CT_Jc {
                val: Some(wml::ST_Jc::Left),
                ..Default::default()
            })),
            p_pr: Some(Box::new(wml::CT_PPrGeneral {
                ind: Some(Box::new(wml::CT_Ind {
                    left: Some(signed_twips(self.indent)),
                    hanging: Some(twips(self.hanging)),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            r_pr: fonts.map(Box::new),
            ..Default::default()
        }
    }
}

/// A custom list (numbering) definition of up to nine levels.
///
/// ```
/// use openxml_docx::{Document, ListDefinition, ListLevel, NumberFormat};
///
/// let mut doc = Document::new();
/// let mut steps = ListDefinition::numbered();
/// steps.levels[0] = ListLevel::numbered(NumberFormat::UpperRoman, "%1)", 0);
/// steps.levels[0].start = 3;
/// let list = doc.add_list_definition(&steps)?;
/// doc.add_list_paragraph("Third step", list, 0)?;
/// assert_eq!(doc.paragraphs()[0].numbering(), Some((list, 0)));
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListDefinition {
    /// Levels 0 to 8 (at least one).
    pub levels: Vec<ListLevel>,
}

impl ListDefinition {
    /// Nine bullet levels (•, ◦, ▪).
    pub fn bullets() -> Self {
        ListDefinition {
            levels: (0..9u8)
                .map(|l| {
                    let bullet = BULLETS[usize::from(l) % 3]
                        .chars()
                        .next()
                        .expect("a bullet character");
                    ListLevel::bullet(bullet, l)
                })
                .collect(),
        }
    }

    /// Nine numbered levels (1., a., i.).
    pub fn numbered() -> Self {
        let formats = [
            ST_NumberFormat::Decimal,
            ST_NumberFormat::LowerLetter,
            ST_NumberFormat::LowerRoman,
        ];
        ListDefinition {
            levels: (0..9u8)
                .map(|l| ListLevel::numbered(formats[usize::from(l) % 3], &format!("%{}.", l + 1), l))
                .collect(),
        }
    }

    /// A legal-style outline: 1., 1.1., 1.1.1., …
    pub fn outline() -> Self {
        ListDefinition {
            levels: (0..9u8)
                .map(|l| {
                    let text: String = (1..=l + 1).map(|n| format!("%{n}.")).collect();
                    let mut level = ListLevel::numbered(ST_NumberFormat::Decimal, &text, l);
                    // Outline numbers hang from the margin and widen with the depth.
                    level.indent = Length::twips(432 + 144 * i64::from(l));
                    level.hanging = level.indent;
                    level
                })
                .collect(),
        }
    }

    fn to_ct(&self, id: i64) -> wml::CT_AbstractNum {
        wml::CT_AbstractNum {
            abstract_num_id: Some(id),
            multi_level_type: Some(Box::new(wml::CT_MultiLevelType {
                val: Some(if self.levels.len() > 1 {
                    wml::ST_MultiLevelType::Multilevel
                } else {
                    wml::ST_MultiLevelType::SingleLevel
                }),
                ..Default::default()
            })),
            lvl: self.levels.iter().enumerate().map(|(i, l)| l.to_ct(i)).collect(),
            ..Default::default()
        }
    }
}

impl Shared {
    /// Adds an abstract definition and a numbering instance for it.
    pub(crate) fn add_list_definition(&mut self, def: &ListDefinition) -> Result<i64> {
        if def.levels.is_empty() || def.levels.len() > 9 {
            return Err(Error::InvalidArgument(format!(
                "a list definition has 1 to 9 levels, not {}",
                def.levels.len()
            )));
        }
        let numbering = self.numbering_mut()?;
        let abstract_id = numbering
            .abstract_num
            .iter()
            .filter_map(|a| a.abstract_num_id)
            .max()
            .map_or(0, |m| m + 1);
        numbering.abstract_num.push(def.to_ct(abstract_id));
        Ok(Self::add_num(numbering, abstract_id, false))
    }
}

impl ParagraphMut<'_> {
    /// Makes the paragraph an item of list `num_id` (from
    /// [`Document::add_list_definition`]) at `level` (0–8).
    pub fn set_list(&mut self, num_id: i64, level: u8) -> Result<&mut Self> {
        if level > 8 {
            return Err(Error::InvalidArgument(format!(
                "list level {level} is not in 0..=8"
            )));
        }
        Ok(self.set_numbering(num_id, level))
    }
}

impl Document {
    /// Adds a custom list definition and returns the numbering id to use
    /// with [`Document::add_list_paragraph`] or [`ParagraphMut::set_list`].
    pub fn add_list_definition(&mut self, def: &ListDefinition) -> Result<i64> {
        self.shared.add_list_definition(def)
    }

    /// Appends a paragraph in the `ListParagraph` style as an item of list
    /// `num_id` at `level`.
    pub fn add_list_paragraph(&mut self, text: &str, num_id: i64, level: u8) -> Result<ParagraphMut<'_>> {
        let known = self
            .numbering()
            .is_some_and(|n| n.num.iter().any(|x| x.num_id == Some(num_id)));
        if !known {
            return Err(Error::NotFound(format!("numbering definition {num_id}")));
        }
        if level > 8 {
            return Err(Error::InvalidArgument(format!(
                "list level {level} is not in 0..=8"
            )));
        }
        let mut p = self.add_paragraph(text);
        p.set_style("ListParagraph")?;
        p.set_list(num_id, level)?;
        Ok(p)
    }

    /// A new numbering instance of the same list as `num_id` whose first
    /// level restarts at its start value. Returns the new numbering id.
    pub fn restart_numbering(&mut self, num_id: i64) -> Result<i64> {
        let numbering = self.shared.numbering_mut()?;
        let abstract_id = numbering
            .num
            .iter()
            .find(|n| n.num_id == Some(num_id))
            .and_then(|n| n.abstract_num_id.as_ref())
            .and_then(|a| a.val)
            .ok_or_else(|| Error::NotFound(format!("numbering definition {num_id}")))?;
        Ok(Shared::add_num(numbering, abstract_id, true))
    }

    /// Numbers the headings `Heading1`…`Heading{levels}` as an outline
    /// (1., 1.1., …, or `def` when given): each level of the definition is
    /// linked to its heading style and each heading style refers to the
    /// numbering. Returns the numbering id.
    pub fn number_headings(&mut self, levels: u8, def: Option<&ListDefinition>) -> Result<i64> {
        if !(1..=9).contains(&levels) {
            return Err(Error::InvalidArgument(format!(
                "{levels} heading levels (1 to 9)"
            )));
        }
        let mut def = def.cloned().unwrap_or_else(ListDefinition::outline);
        if def.levels.len() < usize::from(levels) {
            return Err(Error::InvalidArgument(format!(
                "the definition has {} levels, {levels} needed",
                def.levels.len()
            )));
        }
        def.levels.truncate(usize::from(levels));
        let mut styles = Vec::new();
        for (i, level) in def.levels.iter_mut().enumerate() {
            let id = self.shared.resolve_style(&format!("Heading{}", i + 1))?;
            level.style = Some(id.clone());
            styles.push(id);
        }
        let num_id = self.shared.add_list_definition(&def)?;
        let all = self.shared.styles_mut()?;
        for (i, id) in styles.iter().enumerate() {
            if let Some(style) = all
                .style
                .iter_mut()
                .find(|s| s.style_id.as_deref() == Some(id.as_str()))
            {
                let ppr = style.p_pr.get_or_insert_with(Default::default);
                ppr.num_pr = Some(Box::new(wml::CT_NumPr {
                    ilvl: (i > 0).then(|| decimal(i as i64)),
                    num_id: Some(decimal(num_id)),
                    ..Default::default()
                }));
            }
        }
        Ok(num_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abstract_definitions_have_nine_levels() {
        let bullets = abstract_num(4, ListKind::Bullet);
        assert_eq!(bullets.abstract_num_id, Some(4));
        assert_eq!(bullets.lvl.len(), 9);
        let lvl0 = &bullets.lvl[0];
        assert_eq!(
            lvl0.num_fmt.as_ref().unwrap().val,
            Some(wml::ST_NumberFormat::Bullet)
        );
        assert_eq!(lvl0.lvl_text.as_ref().unwrap().val.as_deref(), Some("\u{2022}"));

        let numbers = abstract_num(0, ListKind::Numbered);
        let fmts: Vec<_> = numbers
            .lvl
            .iter()
            .take(3)
            .map(|l| l.num_fmt.as_ref().unwrap().val.unwrap())
            .collect();
        assert_eq!(
            fmts,
            [
                wml::ST_NumberFormat::Decimal,
                wml::ST_NumberFormat::LowerLetter,
                wml::ST_NumberFormat::LowerRoman
            ]
        );
        assert_eq!(
            numbers.lvl[2].lvl_text.as_ref().unwrap().val.as_deref(),
            Some("%3.")
        );
        assert_eq!(escape(r#"<&">"#), "&lt;&amp;&quot;>");
    }

    #[test]
    fn custom_definitions() {
        let outline = ListDefinition::outline();
        assert_eq!(outline.levels[2].text, "%1.%2.%3.");
        let ct = outline.to_ct(5);
        assert_eq!(ct.abstract_num_id, Some(5));
        assert_eq!(ct.lvl.len(), 9);
        assert_eq!(
            ct.multi_level_type.unwrap().val,
            Some(wml::ST_MultiLevelType::Multilevel)
        );
        let mut single = ListDefinition {
            levels: vec![ListLevel::bullet('\u{F0B7}', 0)],
        };
        single.levels[0].font = Some("Symbol".into());
        let ct = single.to_ct(0);
        assert_eq!(
            ct.multi_level_type.unwrap().val,
            Some(wml::ST_MultiLevelType::SingleLevel)
        );
        assert!(ct.lvl[0].r_pr.is_some());
        assert_eq!(ListDefinition::bullets().levels[1].text, "\u{25E6}");
        assert_eq!(
            ListDefinition::numbered().levels[1].format,
            ST_NumberFormat::LowerLetter
        );

        let mut doc = Document::new();
        assert!(
            doc.add_list_definition(&ListDefinition { levels: vec![] })
                .is_err()
        );
        let id = doc.add_list_definition(&single).unwrap();
        assert!(doc.add_list_paragraph("x", id + 10, 0).is_err());
        assert!(doc.add_list_paragraph("x", id, 9).is_err());
        let again = doc.restart_numbering(id).unwrap();
        assert_ne!(again, id);
        assert!(doc.restart_numbering(999).is_err());
        let headings = doc.number_headings(3, None).unwrap();
        let h2 = doc.style("Heading2").unwrap();
        let num_pr = h2.p_pr.as_ref().unwrap().num_pr.as_ref().unwrap();
        assert_eq!(num_pr.num_id.as_ref().unwrap().val, Some(headings));
        assert_eq!(num_pr.ilvl.as_ref().unwrap().val, Some(1));
        assert!(doc.number_headings(0, None).is_err());
    }
}
