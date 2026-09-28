//! Comments (the comments part and the range markup that anchors them).

use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::w3cdtf_now;
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_PContent, EG_RunInnerContent};

use crate::document::{Document, Shared, ensure_part};
use crate::markup::{self, Event, TextSpan};
use crate::paragraph::ParagraphMut;
use crate::run::RunMut;
use crate::util::string_val;
use crate::{text, walk};

/// A comment to add to a document.
///
/// ```
/// use openxml_docx::{Document, NewComment, TextSpan};
///
/// let mut doc = Document::new();
/// doc.add_paragraph("The quick brown fox");
/// let id = doc.add_comment(0, TextSpan::Text("brown"), &NewComment::new("Ann", "Which shade?"))?;
/// let comments = doc.comments();
/// assert_eq!(comments[0].id, id);
/// assert_eq!(comments[0].anchored_text.as_deref(), Some("brown"));
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewComment {
    /// Author name.
    pub author: String,
    /// Author initials; derived from the author name when `None`.
    pub initials: Option<String>,
    /// Date and time (`YYYY-MM-DDThh:mm:ssZ`); the current time when `None`.
    pub date: Option<String>,
    /// Comment text; each line becomes a paragraph.
    pub text: String,
}

impl NewComment {
    /// A comment by `author` with `text`, dated now.
    pub fn new(author: &str, text: &str) -> Self {
        NewComment {
            author: author.to_owned(),
            text: text.to_owned(),
            ..Default::default()
        }
    }
}

/// A comment read from a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Comment identifier.
    pub id: i64,
    /// Author name.
    pub author: Option<String>,
    /// Author initials.
    pub initials: Option<String>,
    /// Date and time.
    pub date: Option<String>,
    /// Text of the comment (paragraphs separated by `\n`).
    pub text: String,
    /// Text of the commented range in the body, or `None` when the comment
    /// only has a reference mark (no `commentRangeStart`/`commentRangeEnd`).
    pub anchored_text: Option<String>,
}

/// Initials of a name: the first letter of each word.
fn initials_of(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

impl Shared {
    fn comments_mut(&mut self) -> Result<&mut wml::CT_Comments> {
        ensure_part(
            &mut self.package,
            &self.main_part,
            &mut self.comments,
            "/word/comments.xml",
            "/word/comments{}.xml",
            ct::WML_COMMENTS,
            rel_types::COMMENTS,
            Default::default,
        )
    }

    /// Adds a comment to the comments part and returns its id.
    pub(crate) fn add_comment_entry(&mut self, comment: &NewComment) -> Result<i64> {
        let text_style = self.resolve_style("CommentText")?;
        let ref_style = self.resolve_style("CommentReference")?;
        let id = self.new_id();
        let mut blocks = Vec::new();
        for (i, line) in comment.text.split('\n').enumerate() {
            let mut p = wml::CT_P {
                p_pr: Some(Box::new(wml::CT_PPr {
                    p_style: Some(string_val(&text_style)),
                    ..Default::default()
                })),
                ..Default::default()
            };
            if i == 0 {
                let mut r = wml::CT_R::default();
                RunMut::new(&mut r).style(&ref_style);
                r.run_inner_content
                    .push(EG_RunInnerContent::AnnotationRef(Box::default()));
                p.p_content.push(EG_PContent::R(Box::new(r)));
            }
            if !line.is_empty() {
                let mut r = wml::CT_R::default();
                RunMut::new(&mut r).add_text(line);
                p.p_content.push(EG_PContent::R(Box::new(r)));
            }
            blocks.push(EG_BlockLevelElts::P(Box::new(p)));
        }
        let entry = wml::CT_Comment {
            id: Some(id),
            author: Some(comment.author.clone()),
            date: Some(comment.date.clone().unwrap_or_else(w3cdtf_now)),
            initials: Some(
                comment
                    .initials
                    .clone()
                    .unwrap_or_else(|| initials_of(&comment.author)),
            ),
            block_level_elts: blocks,
            ..Default::default()
        };
        self.comments_mut()?.comment.push(entry);
        Ok(id)
    }

    /// The run holding the reference mark of comment `id`.
    pub(crate) fn comment_reference_run(&mut self, id: i64) -> Result<wml::CT_R> {
        let style = self.resolve_style("CommentReference")?;
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).style(&style);
        r.run_inner_content
            .push(EG_RunInnerContent::CommentReference(Box::new(wml::CT_Markup {
                id: Some(id),
                ..Default::default()
            })));
        Ok(r)
    }
}

fn is_reference(r: &wml::CT_R, id: i64) -> bool {
    markup::run_has(
        r,
        |c| matches!(c, EG_RunInnerContent::CommentReference(m) if m.id == Some(id)),
    )
}

impl ParagraphMut<'_> {
    /// Adds a comment anchored to `span` of this paragraph: the range is
    /// marked with `commentRangeStart`/`commentRangeEnd` and followed by a
    /// reference run in the `CommentReference` style. Returns the comment id.
    pub fn add_comment(&mut self, span: TextSpan<'_>, comment: &NewComment) -> Result<i64> {
        let range = markup::resolve_span(self.p, span)?;
        let id = self.shared.add_comment_entry(comment)?;
        let marker = || wml::CT_MarkupRange {
            id: Some(id),
            ..Default::default()
        };
        let after = markup::wrap_range(
            self.p,
            range,
            EG_PContent::CommentRangeStart(Box::new(marker())),
            EG_PContent::CommentRangeEnd(Box::new(marker())),
        );
        let run = self.shared.comment_reference_run(id)?;
        self.p.p_content.insert(after, EG_PContent::R(Box::new(run)));
        Ok(id)
    }
}

impl Document {
    /// Adds a comment anchored to `span` of the `paragraph`-th body
    /// paragraph (in [`Document::paragraphs`] order). Returns the comment id.
    pub fn add_comment(&mut self, paragraph: usize, span: TextSpan<'_>, comment: &NewComment) -> Result<i64> {
        let count = self.paragraphs().len();
        let mut p = self
            .paragraph_mut(paragraph)
            .ok_or_else(|| Error::NotFound(format!("paragraph {paragraph} (the body has {count})")))?;
        p.add_comment(span, comment)
    }

    /// Adds a reply to comment `parent`.
    ///
    /// ECMA-376 has no reply threading (Word's `commentsExtended` part is a
    /// Microsoft extension), so the reply is a separate comment anchored to
    /// the same range, right after the parent. Returns the reply's id.
    pub fn reply_to_comment(&mut self, parent: i64, reply: &NewComment) -> Result<i64> {
        let exists = self
            .shared
            .comments
            .as_ref()
            .is_some_and(|c| c.value.comment.iter().any(|c| c.id == Some(parent)));
        if !exists {
            return Err(Error::NotFound(format!("comment {parent}")));
        }
        let id = self.shared.add_comment_entry(reply)?;
        let run = self.shared.comment_reference_run(id)?;
        let marker = || wml::CT_MarkupRange {
            id: Some(id),
            ..Default::default()
        };
        let mut start = Some(EG_PContent::CommentRangeStart(Box::new(marker())));
        let mut end = Some(EG_PContent::CommentRangeEnd(Box::new(marker())));
        let mut reference = Some(EG_PContent::R(Box::new(run)));
        let body = self.body_mut();
        walk::walk_blocks(&mut body.block_level_elts, &mut |p| {
            if let Some(new) = start.take() {
                start = markup::insert_after(
                    &mut p.p_content,
                    &|c| matches!(c, EG_PContent::CommentRangeStart(m) if m.id == Some(parent)),
                    new,
                );
            }
            if let Some(new) = end.take() {
                end = markup::insert_after(
                    &mut p.p_content,
                    &|c| matches!(c, EG_PContent::CommentRangeEnd(m) if m.id == Some(parent)),
                    new,
                );
            }
            if let Some(new) = reference.take() {
                reference = markup::insert_after(
                    &mut p.p_content,
                    &|c| matches!(c, EG_PContent::R(r) if is_reference(r, parent)),
                    new,
                );
            }
        });
        if reference.is_some() {
            return Err(Error::NotFound(format!(
                "the reference mark of comment {parent} in the body"
            )));
        }
        Ok(id)
    }

    /// Comments of the document with the text they are anchored to.
    pub fn comments(&self) -> Vec<Comment> {
        let Some(part) = self.shared.comments.as_ref() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        markup::push_block_events(&self.body().block_level_elts, &mut events);
        let anchored = markup::range_texts(
            &events,
            |e| {
                if let Event::CommentStart(id) = e {
                    Some(*id)
                } else {
                    None
                }
            },
            |e| {
                if let Event::CommentEnd(id) = e {
                    Some(*id)
                } else {
                    None
                }
            },
        );
        part.value
            .comment
            .iter()
            .map(|c| {
                let id = c.id.unwrap_or_default();
                Comment {
                    id,
                    author: c.author.clone(),
                    initials: c.initials.clone(),
                    date: c.date.clone(),
                    text: text::blocks_text(&text::blocks(&c.block_level_elts)),
                    anchored_text: anchored.get(&id).cloned(),
                }
            })
            .collect()
    }

    /// Removes a comment: its entry in the comments part and its range
    /// markers and reference marks in the body and notes.
    pub fn remove_comment(&mut self, id: i64) -> Result<()> {
        let removed = match self.shared.comments.as_mut() {
            Some(part) => {
                let before = part.value.comment.len();
                part.value.comment.retain(|c| c.id != Some(id));
                let removed = part.value.comment.len() != before;
                part.dirty |= removed;
                removed
            }
            None => false,
        };
        if !removed {
            return Err(Error::NotFound(format!("comment {id}")));
        }
        let remove = |c: &EG_PContent| matches!(c, EG_PContent::CommentRangeStart(m) | EG_PContent::CommentRangeEnd(m) if m.id == Some(id));
        let remove_block = |c: &EG_BlockLevelElts| matches!(c, EG_BlockLevelElts::CommentRangeStart(m) | EG_BlockLevelElts::CommentRangeEnd(m) if m.id == Some(id));
        let mut edit_run = |r: &mut wml::CT_R| {
            let before = r.run_inner_content.len();
            r.run_inner_content
                .retain(|c| !matches!(c, EG_RunInnerContent::CommentReference(m) if m.id == Some(id)));
            r.run_inner_content.len() != before
        };
        if let Some(body) = self.main.body.as_mut()
            && markup::retain_in_blocks(&mut body.block_level_elts, &remove_block, &remove, &mut edit_run)
        {
            self.main_dirty = true;
        }
        if let Some(part) = self.shared.footnotes.as_mut() {
            for note in &mut part.value.footnote {
                part.dirty |= markup::retain_in_blocks(
                    &mut note.block_level_elts,
                    &remove_block,
                    &remove,
                    &mut edit_run,
                );
            }
        }
        if let Some(part) = self.shared.endnotes.as_mut() {
            for note in &mut part.value.endnote {
                part.dirty |= markup::retain_in_blocks(
                    &mut note.block_level_elts,
                    &remove_block,
                    &remove,
                    &mut edit_run,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_come_from_words() {
        assert_eq!(initials_of("Ann Lee"), "AL");
        assert_eq!(initials_of("  émile zola "), "ÉZ");
        assert_eq!(initials_of(""), "");
    }

    #[test]
    fn comment_entries_follow_word_structure() {
        let mut doc = Document::new();
        let id = doc
            .shared
            .add_comment_entry(&NewComment {
                author: "Ann Lee".into(),
                initials: None,
                date: Some("2024-01-02T03:04:05Z".into()),
                text: "First\n\nThird".into(),
            })
            .unwrap();
        let part = doc.shared.comments.as_ref().unwrap();
        let c = &part.value.comment[0];
        assert_eq!(c.id, Some(id));
        assert_eq!(c.initials.as_deref(), Some("AL"));
        assert_eq!(c.date.as_deref(), Some("2024-01-02T03:04:05Z"));
        assert_eq!(c.block_level_elts.len(), 3);
        let EG_BlockLevelElts::P(p) = &c.block_level_elts[0] else {
            panic!()
        };
        assert_eq!(
            p.p_pr.as_ref().unwrap().p_style.as_ref().unwrap().val.as_deref(),
            Some("CommentText")
        );
        let EG_PContent::R(r) = &p.p_content[0] else {
            panic!()
        };
        assert!(matches!(
            r.run_inner_content[0],
            EG_RunInnerContent::AnnotationRef(_)
        ));
        assert_eq!(crate::Run::new(r).style_id(), Some("CommentReference"));
        assert!(doc.style_ids().contains(&"CommentText"));
    }
}
