//! Bookmarks.

use openxml_core::{Error, Result};
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_PContent};

use crate::document::Document;
use crate::markup::{self, Event, TextSpan};
use crate::paragraph::ParagraphMut;

/// A bookmark read from a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    /// Bookmark identifier.
    pub id: i64,
    /// Name (target of internal hyperlinks and references).
    pub name: String,
    /// Text enclosed by the bookmark (paragraphs separated by `\n`).
    pub text: String,
}

/// Checks a bookmark name: 1 to 40 characters without whitespace (the
/// limits Word enforces; names starting with `_` are hidden bookmarks).
fn check_name(name: &str) -> Result<()> {
    let len = name.chars().count();
    if len == 0 || len > 40 || name.chars().any(char::is_whitespace) {
        return Err(Error::InvalidArgument(format!(
            "invalid bookmark name {name:?}: 1 to 40 characters without spaces"
        )));
    }
    Ok(())
}

impl ParagraphMut<'_> {
    /// Adds a bookmark named `name` around `span` of this paragraph and
    /// returns its id. Uniqueness of the name is not checked here (see
    /// [`Document::add_bookmark`]).
    pub fn add_bookmark(&mut self, name: &str, span: TextSpan<'_>) -> Result<i64> {
        check_name(name)?;
        let range = markup::resolve_span(self.p, span)?;
        let id = self.shared.new_id();
        markup::wrap_range(
            self.p,
            range,
            EG_PContent::BookmarkStart(Box::new(wml::CT_Bookmark {
                id: Some(id),
                name: Some(name.to_owned()),
                ..Default::default()
            })),
            EG_PContent::BookmarkEnd(Box::new(wml::CT_MarkupRange {
                id: Some(id),
                ..Default::default()
            })),
        );
        Ok(id)
    }
}

impl Document {
    /// Adds a bookmark around `span` of the `paragraph`-th body paragraph.
    /// Fails when a bookmark with that name exists. Returns the bookmark id.
    ///
    /// ```
    /// use openxml_docx::{Document, TextSpan};
    ///
    /// let mut doc = Document::new();
    /// doc.add_paragraph("Results are below.");
    /// doc.add_bookmark(0, TextSpan::Text("Results"), "results")?;
    /// doc.add_paragraph("See ").add_internal_hyperlink("results", "results")?;
    /// assert_eq!(doc.bookmarks()[0].text, "Results");
    /// # Ok::<(), openxml_docx::Error>(())
    /// ```
    pub fn add_bookmark(&mut self, paragraph: usize, span: TextSpan<'_>, name: &str) -> Result<i64> {
        if self.bookmarks().iter().any(|b| b.name == name) {
            return Err(Error::InvalidArgument(format!(
                "bookmark {name:?} already exists"
            )));
        }
        let count = self.paragraphs().len();
        let mut p = self
            .paragraph_mut(paragraph)
            .ok_or_else(|| Error::NotFound(format!("paragraph {paragraph} (the body has {count})")))?;
        p.add_bookmark(name, span)
    }

    /// Bookmarks of the body in document order.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        let mut events = Vec::new();
        markup::push_block_events(&self.body().block_level_elts, &mut events);
        let texts = markup::range_texts(
            &events,
            |e| {
                if let Event::BookmarkStart(id, _) = e {
                    Some(*id)
                } else {
                    None
                }
            },
            |e| {
                if let Event::BookmarkEnd(id) = e {
                    Some(*id)
                } else {
                    None
                }
            },
        );
        events
            .iter()
            .filter_map(|e| match e {
                Event::BookmarkStart(id, name) => Some(Bookmark {
                    id: *id,
                    name: name.clone(),
                    text: texts.get(id).cloned().unwrap_or_default(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Removes the bookmark named `name` (its start and end markers).
    pub fn remove_bookmark(&mut self, name: &str) -> Result<()> {
        let id = self
            .bookmarks()
            .into_iter()
            .find(|b| b.name == name)
            .map(|b| b.id)
            .ok_or_else(|| Error::NotFound(format!("bookmark {name:?}")))?;
        let remove = |c: &EG_PContent| match c {
            EG_PContent::BookmarkStart(b) => b.id == Some(id),
            EG_PContent::BookmarkEnd(m) => m.id == Some(id),
            _ => false,
        };
        let remove_block = |c: &EG_BlockLevelElts| match c {
            EG_BlockLevelElts::BookmarkStart(b) => b.id == Some(id),
            EG_BlockLevelElts::BookmarkEnd(m) => m.id == Some(id),
            _ => false,
        };
        let body = self.body_mut();
        markup::retain_in_blocks(&mut body.block_level_elts, &remove_block, &remove, &mut |_| false);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(check_name("ok_name1").is_ok());
        assert!(check_name("_Toc123").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name("has space").is_err());
        assert!(check_name(&"x".repeat(41)).is_err());
        assert!(check_name(&"é".repeat(40)).is_ok());
    }

    #[test]
    fn add_list_and_remove() {
        let mut doc = Document::new();
        doc.add_paragraph("alpha beta gamma");
        doc.add_paragraph("delta");
        let id = doc.add_bookmark(0, TextSpan::Text("beta"), "b").unwrap();
        doc.add_bookmark(1, TextSpan::Paragraph, "d").unwrap();
        assert!(doc.add_bookmark(1, TextSpan::Paragraph, "d").is_err());
        let marks = doc.bookmarks();
        assert_eq!(marks.len(), 2);
        assert_eq!((marks[0].id, marks[0].text.as_str()), (id, "beta"));
        assert_eq!(marks[1].text, "delta");
        doc.remove_bookmark("b").unwrap();
        assert_eq!(doc.bookmarks().len(), 1);
        assert!(doc.remove_bookmark("b").is_err());
        assert_eq!(doc.text(), "alpha beta gamma\ndelta");
    }
}
