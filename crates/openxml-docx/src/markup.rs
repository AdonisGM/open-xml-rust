//! Locating text inside paragraphs for annotations (comments, bookmarks,
//! tracked changes) and generic editing of paragraph content trees.

use std::collections::BTreeMap;
use std::ops::Range;

use openxml_core::{Error, Result};
use openxml_schema::wml::{
    self, CT_RunTrackChange_Choice, EG_BlockLevelElts, EG_ContentBlockContent, EG_PContent,
};

use crate::{text, walk};

/// The part of a paragraph an annotation (comment, bookmark, tracked
/// deletion, …) covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSpan<'a> {
    /// The whole content of the paragraph.
    Paragraph,
    /// Runs `first..=last`, as indexed by [`crate::Paragraph::runs`]. A run
    /// nested in a hyperlink, field or content control selects the whole
    /// container.
    Runs(usize, usize),
    /// The first occurrence of this text among the direct runs of the
    /// paragraph; runs are split at the boundaries when needed.
    Text(&'a str),
}

/// Resolves a span to a range of indices into `p.p_content`, splitting runs
/// when the span is a text range.
pub(crate) fn resolve_span(p: &mut wml::CT_P, span: TextSpan<'_>) -> Result<Range<usize>> {
    match span {
        TextSpan::Paragraph => Ok(0..p.p_content.len()),
        TextSpan::Runs(first, last) => {
            if first > last {
                return Err(Error::InvalidArgument(format!(
                    "run range {first}..={last} is empty"
                )));
            }
            // Top-level item holding each visible run.
            let mut owners = Vec::new();
            for (i, item) in p.p_content.iter().enumerate() {
                let n = text::runs(std::slice::from_ref(item)).len();
                owners.extend(std::iter::repeat_n(i, n));
            }
            let count = owners.len();
            let (Some(&a), Some(&b)) = (owners.get(first), owners.get(last)) else {
                return Err(Error::NotFound(format!(
                    "runs {first}..={last} (the paragraph has {count})"
                )));
            };
            Ok(a..b + 1)
        }
        TextSpan::Text(needle) => {
            let (start, end) = walk::find_in_direct_runs(p, needle)
                .ok_or_else(|| Error::NotFound(format!("text {needle:?} in the paragraph")))?;
            walk::isolate_range(p, start, end)
                .ok_or_else(|| Error::InvalidArgument(format!("text {needle:?} cannot be isolated in runs")))
        }
    }
}

/// Wraps `p_content[range]` between two markers.
pub(crate) fn wrap_range(
    p: &mut wml::CT_P,
    range: Range<usize>,
    start: EG_PContent,
    end: EG_PContent,
) -> usize {
    p.p_content.insert(range.end, end);
    p.p_content.insert(range.start, start);
    // Index just after the end marker.
    range.end + 2
}

/// A flattened view of the body used to find the text between range markers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    CommentStart(i64),
    CommentEnd(i64),
    BookmarkStart(i64, String),
    BookmarkEnd(i64),
    Text(String),
    ParagraphEnd,
}

fn push_marker(item: &EG_PContent, out: &mut Vec<Event>) -> bool {
    match item {
        EG_PContent::CommentRangeStart(m) => out.push(Event::CommentStart(m.id.unwrap_or_default())),
        EG_PContent::CommentRangeEnd(m) => out.push(Event::CommentEnd(m.id.unwrap_or_default())),
        EG_PContent::BookmarkStart(b) => out.push(Event::BookmarkStart(
            b.id.unwrap_or_default(),
            b.name.clone().unwrap_or_default(),
        )),
        EG_PContent::BookmarkEnd(m) => out.push(Event::BookmarkEnd(m.id.unwrap_or_default())),
        _ => return false,
    }
    true
}

fn push_track_change_events(items: &[CT_RunTrackChange_Choice], out: &mut Vec<Event>) {
    for item in items {
        match item {
            CT_RunTrackChange_Choice::WR(r) => out.push(Event::Text(text::run_text(r))),
            CT_RunTrackChange_Choice::CommentRangeStart(m) => {
                out.push(Event::CommentStart(m.id.unwrap_or_default()))
            }
            CT_RunTrackChange_Choice::CommentRangeEnd(m) => {
                out.push(Event::CommentEnd(m.id.unwrap_or_default()))
            }
            CT_RunTrackChange_Choice::BookmarkStart(b) => out.push(Event::BookmarkStart(
                b.id.unwrap_or_default(),
                b.name.clone().unwrap_or_default(),
            )),
            CT_RunTrackChange_Choice::BookmarkEnd(m) => {
                out.push(Event::BookmarkEnd(m.id.unwrap_or_default()))
            }
            CT_RunTrackChange_Choice::Ins(x) | CT_RunTrackChange_Choice::MoveTo(x) => {
                push_track_change_events(&x.choice, out)
            }
            CT_RunTrackChange_Choice::Sdt(x) => {
                if let Some(c) = &x.sdt_content {
                    push_content_events(&c.p_content, out);
                }
            }
            CT_RunTrackChange_Choice::CustomXml(x) => push_content_events(&x.p_content, out),
            CT_RunTrackChange_Choice::SmartTag(x) => push_content_events(&x.p_content, out),
            _ => {}
        }
    }
}

fn push_content_events(items: &[EG_PContent], out: &mut Vec<Event>) {
    for item in items {
        if push_marker(item, out) {
            continue;
        }
        match item {
            EG_PContent::R(r) => out.push(Event::Text(text::run_text(r))),
            EG_PContent::Hyperlink(h) => push_content_events(&h.p_content, out),
            EG_PContent::FldSimple(f) => push_content_events(&f.p_content, out),
            EG_PContent::CustomXml(x) => push_content_events(&x.p_content, out),
            EG_PContent::SmartTag(x) => push_content_events(&x.p_content, out),
            EG_PContent::Dir(x) => push_content_events(&x.p_content, out),
            EG_PContent::Bdo(x) => push_content_events(&x.p_content, out),
            EG_PContent::Sdt(x) => {
                if let Some(c) = &x.sdt_content {
                    push_content_events(&c.p_content, out);
                }
            }
            EG_PContent::Ins(x) | EG_PContent::MoveTo(x) => push_track_change_events(&x.choice, out),
            _ => {}
        }
    }
}

fn push_paragraph_events(p: &wml::CT_P, out: &mut Vec<Event>) {
    push_content_events(&p.p_content, out);
    out.push(Event::ParagraphEnd);
}

fn push_table_events(t: &wml::CT_Tbl, out: &mut Vec<Event>) {
    for row in text::rows(t) {
        for cell in text::cells(row) {
            push_block_events(&cell.block_level_elts, out);
        }
    }
}

fn push_block_content_events(items: &[EG_ContentBlockContent], out: &mut Vec<Event>) {
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => push_paragraph_events(p, out),
            EG_ContentBlockContent::Tbl(t) => push_table_events(t, out),
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = &s.sdt_content {
                    push_block_content_events(&c.content_block_content, out);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => push_block_content_events(&x.content_block_content, out),
            EG_ContentBlockContent::CommentRangeStart(m) => {
                out.push(Event::CommentStart(m.id.unwrap_or_default()))
            }
            EG_ContentBlockContent::CommentRangeEnd(m) => {
                out.push(Event::CommentEnd(m.id.unwrap_or_default()))
            }
            EG_ContentBlockContent::BookmarkStart(b) => out.push(Event::BookmarkStart(
                b.id.unwrap_or_default(),
                b.name.clone().unwrap_or_default(),
            )),
            EG_ContentBlockContent::BookmarkEnd(m) => out.push(Event::BookmarkEnd(m.id.unwrap_or_default())),
            _ => {}
        }
    }
}

/// Flattens a block container into range events and text.
pub(crate) fn push_block_events(items: &[EG_BlockLevelElts], out: &mut Vec<Event>) {
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => push_paragraph_events(p, out),
            EG_BlockLevelElts::Tbl(t) => push_table_events(t, out),
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = &s.sdt_content {
                    push_block_content_events(&c.content_block_content, out);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => push_block_content_events(&x.content_block_content, out),
            EG_BlockLevelElts::CommentRangeStart(m) => {
                out.push(Event::CommentStart(m.id.unwrap_or_default()))
            }
            EG_BlockLevelElts::CommentRangeEnd(m) => out.push(Event::CommentEnd(m.id.unwrap_or_default())),
            EG_BlockLevelElts::BookmarkStart(b) => out.push(Event::BookmarkStart(
                b.id.unwrap_or_default(),
                b.name.clone().unwrap_or_default(),
            )),
            EG_BlockLevelElts::BookmarkEnd(m) => out.push(Event::BookmarkEnd(m.id.unwrap_or_default())),
            _ => {}
        }
    }
}

/// Text enclosed by each range, keyed by identifier. `start`/`end` select
/// the kind of range. Paragraph ends inside a range become `\n`.
pub(crate) fn range_texts(
    events: &[Event],
    start: impl Fn(&Event) -> Option<i64>,
    end: impl Fn(&Event) -> Option<i64>,
) -> BTreeMap<i64, String> {
    let mut open: Vec<i64> = Vec::new();
    let mut out: BTreeMap<i64, String> = BTreeMap::new();
    for e in events {
        if let Some(id) = start(e) {
            open.push(id);
            out.entry(id).or_default();
        } else if let Some(id) = end(e) {
            open.retain(|o| *o != id);
        } else {
            match e {
                Event::Text(t) => {
                    for id in &open {
                        out.get_mut(id).expect("opened").push_str(t);
                    }
                }
                Event::ParagraphEnd => {
                    for id in &open {
                        out.get_mut(id).expect("opened").push('\n');
                    }
                }
                _ => {}
            }
        }
    }
    for text in out.values_mut() {
        while text.ends_with('\n') {
            text.pop();
        }
    }
    out
}

/// Removes, at every depth of paragraph content, the items for which
/// `remove` returns true. Runs are also offered to `edit_run`, and runs left
/// without content are removed.
pub(crate) fn retain_content(
    items: &mut Vec<EG_PContent>,
    remove: &dyn Fn(&EG_PContent) -> bool,
    edit_run: &mut dyn FnMut(&mut wml::CT_R) -> bool,
) -> bool {
    let mut changed = false;
    let before = items.len();
    items.retain(|i| !remove(i));
    changed |= items.len() != before;
    let mut i = 0;
    while i < items.len() {
        let mut drop = false;
        match &mut items[i] {
            EG_PContent::R(r) => {
                if edit_run(r) {
                    changed = true;
                    drop = r.run_inner_content.is_empty();
                }
            }
            EG_PContent::Hyperlink(h) => changed |= retain_content(&mut h.p_content, remove, edit_run),
            EG_PContent::FldSimple(f) => changed |= retain_content(&mut f.p_content, remove, edit_run),
            EG_PContent::CustomXml(x) => changed |= retain_content(&mut x.p_content, remove, edit_run),
            EG_PContent::SmartTag(x) => changed |= retain_content(&mut x.p_content, remove, edit_run),
            EG_PContent::Dir(x) => changed |= retain_content(&mut x.p_content, remove, edit_run),
            EG_PContent::Bdo(x) => changed |= retain_content(&mut x.p_content, remove, edit_run),
            EG_PContent::Sdt(x) => {
                if let Some(c) = &mut x.sdt_content {
                    changed |= retain_content(&mut c.p_content, remove, edit_run);
                }
            }
            EG_PContent::Ins(x) | EG_PContent::Del(x) | EG_PContent::MoveFrom(x) | EG_PContent::MoveTo(x) => {
                for c in &mut x.choice {
                    if let CT_RunTrackChange_Choice::WR(r) = c
                        && edit_run(r)
                    {
                        changed = true;
                    }
                }
                x.choice.retain(
                    |c| !matches!(c, CT_RunTrackChange_Choice::WR(r) if r.run_inner_content.is_empty()),
                );
            }
            _ => {}
        }
        if drop {
            items.remove(i);
        } else {
            i += 1;
        }
    }
    changed
}

/// Applies [`retain_content`] to every paragraph of a block container and
/// removes the block-level items for which `remove_block` returns true.
pub(crate) fn retain_in_blocks(
    items: &mut Vec<EG_BlockLevelElts>,
    remove_block: &dyn Fn(&EG_BlockLevelElts) -> bool,
    remove: &dyn Fn(&EG_PContent) -> bool,
    edit_run: &mut dyn FnMut(&mut wml::CT_R) -> bool,
) -> bool {
    let before = items.len();
    items.retain(|i| !remove_block(i));
    let mut changed = items.len() != before;
    walk::walk_blocks(items, &mut |p| {
        changed |= retain_content(&mut p.p_content, remove, edit_run);
    });
    changed
}

/// Inserts `new` right after the first item (at any depth of paragraph
/// content) matching `pred`. Returns whether an item was found.
pub(crate) fn insert_after(
    items: &mut Vec<EG_PContent>,
    pred: &dyn Fn(&EG_PContent) -> bool,
    new: EG_PContent,
) -> Option<EG_PContent> {
    if let Some(i) = items.iter().position(pred) {
        items.insert(i + 1, new);
        return None;
    }
    let mut new = Some(new);
    for item in items.iter_mut() {
        let inner = match item {
            EG_PContent::Hyperlink(h) => &mut h.p_content,
            EG_PContent::FldSimple(f) => &mut f.p_content,
            EG_PContent::CustomXml(x) => &mut x.p_content,
            EG_PContent::SmartTag(x) => &mut x.p_content,
            EG_PContent::Sdt(x) => match &mut x.sdt_content {
                Some(c) => &mut c.p_content,
                None => continue,
            },
            _ => continue,
        };
        new = Some(insert_after(inner, pred, new.take().expect("still pending"))?);
    }
    new
}

/// Whether a run contains an item matching `pred`.
pub(crate) fn run_has(r: &wml::CT_R, pred: impl Fn(&wml::EG_RunInnerContent) -> bool) -> bool {
    r.run_inner_content.iter().any(pred)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(xml: &str) -> wml::CT_P {
        let doc = wml::elements::DOCUMENT
            .parse(&format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{xml}</w:body></w:document>"#
            ))
            .unwrap();
        let Some(EG_BlockLevelElts::P(p)) = doc.body.unwrap().block_level_elts.into_iter().next() else {
            panic!()
        };
        *p
    }

    #[test]
    fn resolves_spans() {
        let xml = r#"<w:p><w:r><w:t>One </w:t></w:r><w:hyperlink><w:r><w:t>two</w:t></w:r><w:r><w:t>2</w:t></w:r></w:hyperlink><w:r><w:t xml:space="preserve"> three</w:t></w:r></w:p>"#;
        let mut p = paragraph(xml);
        assert_eq!(resolve_span(&mut p, TextSpan::Paragraph).unwrap(), 0..3);
        assert_eq!(resolve_span(&mut p, TextSpan::Runs(0, 0)).unwrap(), 0..1);
        assert_eq!(resolve_span(&mut p, TextSpan::Runs(1, 2)).unwrap(), 1..2);
        assert_eq!(resolve_span(&mut p, TextSpan::Runs(2, 3)).unwrap(), 1..3);
        assert!(resolve_span(&mut p, TextSpan::Runs(3, 4)).is_err());
        assert!(resolve_span(&mut p, TextSpan::Runs(2, 1)).is_err());
        assert!(resolve_span(&mut p, TextSpan::Text("two")).is_err());
        let r = resolve_span(&mut p, TextSpan::Text("thr")).unwrap();
        assert_eq!(r, 3..4);
        assert_eq!(text::paragraph_text(&p), "One two2 three");
    }

    #[test]
    fn range_texts_span_paragraphs() {
        let xml = r#"<w:p><w:r><w:t>a</w:t></w:r><w:commentRangeStart w:id="1"/><w:r><w:t>b</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="2" w:name="x"/><w:r><w:t>c</w:t></w:r><w:commentRangeEnd w:id="1"/><w:bookmarkEnd w:id="2"/></w:p>"#;
        let doc = wml::elements::DOCUMENT
            .parse(&format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{xml}</w:body></w:document>"#
            ))
            .unwrap();
        let mut events = Vec::new();
        push_block_events(&doc.body.unwrap().block_level_elts, &mut events);
        let comments = range_texts(
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
        assert_eq!(comments[&1], "b\nc");
        let bookmarks = range_texts(
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
        assert_eq!(bookmarks[&2], "c");
    }

    #[test]
    fn retains_and_inserts_at_depth() {
        let xml = r#"<w:p><w:commentRangeStart w:id="3"/><w:hyperlink><w:commentRangeStart w:id="4"/><w:r><w:t>x</w:t></w:r></w:hyperlink><w:r><w:commentReference w:id="3"/></w:r></w:p>"#;
        let mut p = paragraph(xml);
        let is_start = |id: i64| move |c: &EG_PContent| matches!(c, EG_PContent::CommentRangeStart(m) if m.id == Some(id));
        let marker = || EG_PContent::BookmarkEnd(Box::default());
        assert!(insert_after(&mut p.p_content, &is_start(4), marker()).is_none());
        assert!(insert_after(&mut p.p_content, &is_start(9), marker()).is_some());
        let changed = retain_content(&mut p.p_content, &is_start(3), &mut |r| {
            let before = r.run_inner_content.len();
            r.run_inner_content
                .retain(|c| !matches!(c, wml::EG_RunInnerContent::CommentReference(_)));
            r.run_inner_content.len() != before
        });
        assert!(changed);
        // The start marker and the emptied reference run are gone.
        assert_eq!(p.p_content.len(), 1);
        let EG_PContent::Hyperlink(h) = &p.p_content[0] else {
            panic!()
        };
        assert_eq!(h.p_content.len(), 3);
        assert!(matches!(h.p_content[1], EG_PContent::BookmarkEnd(_)));
        assert!(run_has(
            match &h.p_content[2] {
                EG_PContent::R(r) => r,
                _ => panic!(),
            },
            |c| matches!(c, wml::EG_RunInnerContent::T(_))
        ));
    }
}
