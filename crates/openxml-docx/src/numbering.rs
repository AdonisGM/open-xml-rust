//! Bulleted and numbered lists (the numbering definitions part).

use openxml_core::Result;
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::wml;

use crate::document::{Shared, Typed};
use crate::util::decimal;

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
    fn numbering_mut(&mut self) -> Result<&mut wml::CT_Numbering> {
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
    fn add_num(numbering: &mut wml::CT_Numbering, abstract_id: i64, restart: bool) -> i64 {
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
}
