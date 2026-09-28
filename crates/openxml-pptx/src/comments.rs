//! Legacy slide comments (`commentAuthors.xml` and `comments/commentN.xml`).
//!
//! Modern (threaded) comments of PowerPoint 365 live in other parts
//! (`modernComment_*.xml`, `authors.xml`); they are left untouched.

use openxml_core::part::{read_part, read_related, write_part};
use openxml_core::{Error, Length, Result};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::{dml, pml};

use crate::presentation::Presentation;
use crate::shape::coord;
use crate::slide::SlideMut;

/// EMUs per unit of a comment position (1/8 point).
const EMU_PER_POS_UNIT: f64 = 1587.5;

/// A comment author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentAuthor {
    /// Identifier.
    pub id: u32,
    /// Name.
    pub name: String,
    /// Initials.
    pub initials: String,
}

/// A legacy comment on a slide.
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// Identifier of the author.
    pub author_id: u32,
    /// Name of the author.
    pub author: String,
    /// Index of the comment among the author's comments (`idx`).
    pub index: u32,
    /// Date and time (ISO 8601).
    pub date: String,
    /// Position of the comment marker on the slide.
    pub position: (Length, Length),
    /// Text.
    pub text: String,
}

fn position_units(v: Length) -> i64 {
    (v.as_emu() as f64 / EMU_PER_POS_UNIT).round() as i64
}

fn position_length(c: &dml::ST_Coordinate) -> Length {
    let units = match crate::shape::coordinate(c) {
        Some(l) => l.as_emu(),
        None => 0,
    };
    Length::emu((units as f64 * EMU_PER_POS_UNIT).round() as i64)
}

impl Presentation {
    fn comment_authors_part(&self) -> Result<Option<(PartName, pml::CT_CommentAuthorList)>> {
        read_related(
            &self.package,
            Some(&self.part),
            rel_types::COMMENT_AUTHORS,
            &pml::elements::CM_AUTHOR_LST,
        )
    }

    /// The authors of legacy comments.
    pub fn comment_authors(&self) -> Result<Vec<CommentAuthor>> {
        Ok(self
            .comment_authors_part()?
            .map(|(_, list)| {
                list.cm_author
                    .iter()
                    .map(|a| CommentAuthor {
                        id: a.id.unwrap_or(0),
                        name: a.name.clone().unwrap_or_default(),
                        initials: a.initials.clone().unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The legacy comments of the slide at `index`, in document order.
    pub fn comments(&self, index: usize) -> Result<Vec<Comment>> {
        let slide = self
            .slides
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("slide {index}")))?;
        let Some((_, list)) = read_related(
            &self.package,
            Some(&slide.part),
            rel_types::COMMENTS,
            &pml::elements::CM_LST,
        )?
        else {
            return Ok(Vec::new());
        };
        let authors = self.comment_authors()?;
        Ok(list
            .cm
            .iter()
            .map(|c| {
                let author_id = c.author_id.unwrap_or(0);
                let pos = c.pos.as_deref();
                Comment {
                    author_id,
                    author: authors
                        .iter()
                        .find(|a| a.id == author_id)
                        .map(|a| a.name.clone())
                        .unwrap_or_default(),
                    index: c.idx.unwrap_or(0),
                    date: c.dt.clone().unwrap_or_default(),
                    position: (
                        pos.and_then(|p| p.x.as_ref())
                            .map_or(Length::ZERO, position_length),
                        pos.and_then(|p| p.y.as_ref())
                            .map_or(Length::ZERO, position_length),
                    ),
                    text: c.text.clone().unwrap_or_default(),
                }
            })
            .collect())
    }

    /// The comment author with this name, created when missing. Returns its
    /// identifier and the next comment index, which is reserved.
    fn reserve_comment_index(&mut self, name: &str, initials: &str) -> Result<(u32, u32)> {
        let (part, mut list) = match self.comment_authors_part()? {
            Some(found) => found,
            None => {
                let part = PartName::new("/ppt/commentAuthors.xml")?;
                let part = if self.package.contains(&part) {
                    self.package.next_part_name("/ppt/commentAuthors{}.xml")?
                } else {
                    part
                };
                self.package
                    .add_relationship(Some(&self.part), rel_types::COMMENT_AUTHORS, &part)?;
                (part, pml::CT_CommentAuthorList::default())
            }
        };
        let pos = match list
            .cm_author
            .iter()
            .position(|a| a.name.as_deref() == Some(name))
        {
            Some(p) => p,
            None => {
                let id = list
                    .cm_author
                    .iter()
                    .filter_map(|a| a.id)
                    .map(|i| i + 1)
                    .max()
                    .unwrap_or(0);
                list.cm_author.push(pml::CT_CommentAuthor {
                    id: Some(id),
                    name: Some(name.to_owned()),
                    initials: Some(initials.to_owned()),
                    last_idx: Some(0),
                    clr_idx: Some(id % 8),
                    ..Default::default()
                });
                list.cm_author.len() - 1
            }
        };
        let author = &mut list.cm_author[pos];
        let index = author.last_idx.unwrap_or(0) + 1;
        author.last_idx = Some(index);
        let id = author.id.unwrap_or(0);
        write_part(
            &mut self.package,
            &part,
            ct::PML_COMMENT_AUTHORS,
            &pml::elements::CM_AUTHOR_LST,
            &list,
        )?;
        Ok((id, index))
    }
}

impl SlideMut<'_> {
    fn comments_part(&self) -> Option<PartName> {
        self.pres
            .package
            .related_part(Some(&self.part), rel_types::COMMENTS)
            .filter(|p| self.pres.package.contains(p))
    }

    /// Adds a legacy comment at a position on the slide, dated now. The
    /// author is created when needed. Returns the comment's index.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// slide.add_comment("Ada Lovelace", "AL", "Check these numbers", Length::cm(2.0), Length::cm(3.0))?;
    /// let comments = deck.comments(0)?;
    /// assert_eq!(comments[0].author, "Ada Lovelace");
    /// assert_eq!(comments[0].text, "Check these numbers");
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_comment(
        &mut self,
        author: &str,
        initials: &str,
        text: &str,
        x: Length,
        y: Length,
    ) -> Result<u32> {
        let date = openxml_opc::w3cdtf_now();
        self.add_comment_dated(author, initials, text, x, y, &date)
    }

    /// Adds a legacy comment with an explicit date (`xsd:dateTime`, e.g. `2024-05-01T10:00:00Z`).
    pub fn add_comment_dated(
        &mut self,
        author: &str,
        initials: &str,
        text: &str,
        x: Length,
        y: Length,
        date: &str,
    ) -> Result<u32> {
        let (author_id, index) = self.pres.reserve_comment_index(author, initials)?;
        let slide_part = self.part.clone();
        let (part, mut list) = match self.comments_part() {
            Some(p) => {
                let list = read_part(&self.pres.package, &p, &pml::elements::CM_LST)?;
                (p, list)
            }
            None => {
                let p = self.pres.package.next_part_name("/ppt/comments/comment{}.xml")?;
                self.pres
                    .package
                    .add_part(p.clone(), ct::PML_COMMENTS, Vec::new())?;
                self.pres
                    .package
                    .add_relationship(Some(&slide_part), rel_types::COMMENTS, &p)?;
                (p, pml::CT_CommentList::default())
            }
        };
        list.cm.push(pml::CT_Comment {
            author_id: Some(author_id),
            dt: Some(date.to_owned()),
            idx: Some(index),
            pos: Some(Box::new(dml::CT_Point2D {
                x: Some(coord(Length::emu(position_units(x)))),
                y: Some(coord(Length::emu(position_units(y)))),
                ..Default::default()
            })),
            text: Some(text.to_owned()),
            ..Default::default()
        });
        write_part(
            &mut self.pres.package,
            &part,
            ct::PML_COMMENTS,
            &pml::elements::CM_LST,
            &list,
        )?;
        Ok(index)
    }

    /// Removes the comment of `author_id` with index `index`. The comments
    /// part is removed with its last comment. Returns whether a comment was removed.
    pub fn remove_comment(&mut self, author_id: u32, index: u32) -> Result<bool> {
        let Some(part) = self.comments_part() else {
            return Ok(false);
        };
        let mut list = read_part(&self.pres.package, &part, &pml::elements::CM_LST)?;
        let before = list.cm.len();
        list.cm
            .retain(|c| !(c.author_id == Some(author_id) && c.idx == Some(index)));
        if before == list.cm.len() {
            return Ok(false);
        }
        if list.cm.is_empty() {
            self.pres.package.remove_part(&part);
        } else {
            write_part(
                &mut self.pres.package,
                &part,
                ct::PML_COMMENTS,
                &pml::elements::CM_LST,
                &list,
            )?;
        }
        Ok(true)
    }

    /// Removes all legacy comments of the slide.
    pub fn clear_comments(&mut self) {
        if let Some(part) = self.comments_part() {
            self.pres.package.remove_part(&part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_use_eighth_points() {
        assert_eq!(position_units(Length::pt(10.0)), 80);
        assert_eq!(position_length(&coord(Length::emu(80))), Length::pt(10.0));
    }
}
