//! Mutable traversal of every paragraph of a block container, and helpers
//! that operate on the runs of one paragraph (splitting runs at character
//! offsets so that a text range occupies whole runs).

use openxml_schema::wml::{self, EG_BlockLevelElts, EG_ContentBlockContent, EG_PContent, EG_RunInnerContent};

use crate::text;

fn walk_content(items: &mut [EG_ContentBlockContent], f: &mut dyn FnMut(&mut wml::CT_P)) {
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => f(p),
            EG_ContentBlockContent::Tbl(t) => walk_table(t, f),
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut() {
                    walk_content(&mut c.content_block_content, f);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => walk_content(&mut x.content_block_content, f),
            _ => {}
        }
    }
}

fn walk_table(t: &mut wml::CT_Tbl, f: &mut dyn FnMut(&mut wml::CT_P)) {
    for row in text::rows_mut(t) {
        for cell in text::cells_mut(row) {
            walk_blocks(&mut cell.block_level_elts, f);
        }
    }
}

/// Calls `f` for every paragraph of a block container, recursing into
/// tables, content controls and custom XML.
pub(crate) fn walk_blocks(items: &mut [EG_BlockLevelElts], f: &mut dyn FnMut(&mut wml::CT_P)) {
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => f(p),
            EG_BlockLevelElts::Tbl(t) => walk_table(t, f),
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_mut() {
                    walk_content(&mut c.content_block_content, f);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => walk_content(&mut x.content_block_content, f),
            _ => {}
        }
    }
}

fn walk_content_ref<'a>(items: &'a [EG_ContentBlockContent], f: &mut dyn FnMut(&'a wml::CT_P)) {
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => f(p),
            EG_ContentBlockContent::Tbl(t) => walk_table_ref(t, f),
            EG_ContentBlockContent::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_ref() {
                    walk_content_ref(&c.content_block_content, f);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => walk_content_ref(&x.content_block_content, f),
            _ => {}
        }
    }
}

fn walk_table_ref<'a>(t: &'a wml::CT_Tbl, f: &mut dyn FnMut(&'a wml::CT_P)) {
    for row in text::rows(t) {
        for cell in text::cells(row) {
            walk_blocks_ref(&cell.block_level_elts, f);
        }
    }
}

/// Calls `f` for every paragraph of a block container (read-only).
pub(crate) fn walk_blocks_ref<'a>(items: &'a [EG_BlockLevelElts], f: &mut dyn FnMut(&'a wml::CT_P)) {
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => f(p),
            EG_BlockLevelElts::Tbl(t) => walk_table_ref(t, f),
            EG_BlockLevelElts::Sdt(s) => {
                if let Some(c) = s.sdt_content.as_ref() {
                    walk_content_ref(&c.content_block_content, f);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => walk_content_ref(&x.content_block_content, f),
            _ => {}
        }
    }
}

/// Number of characters of text in a run (as counted by [`text::run_text`]).
fn run_len(r: &wml::CT_R) -> usize {
    text::run_text(r).chars().count()
}

/// Splits a run into two at character offset `at` of its text. Only runs
/// made of text, tabs and breaks can be split; the formatting is copied.
fn split_run(r: &wml::CT_R, at: usize) -> Option<(wml::CT_R, wml::CT_R)> {
    let mut left = wml::CT_R {
        r_pr: r.r_pr.clone(),
        ..Default::default()
    };
    let mut right = left.clone();
    let mut pos = 0usize;
    for item in &r.run_inner_content {
        match item {
            EG_RunInnerContent::T(t) => {
                let len = t.value.chars().count();
                if pos + len <= at {
                    left.run_inner_content.push(item.clone());
                } else if pos >= at {
                    right.run_inner_content.push(item.clone());
                } else {
                    let cut = t
                        .value
                        .char_indices()
                        .nth(at - pos)
                        .map_or(t.value.len(), |(i, _)| i);
                    let (a, b) = t.value.split_at(cut);
                    left.run_inner_content
                        .push(EG_RunInnerContent::T(crate::util::text_node(a)));
                    right
                        .run_inner_content
                        .push(EG_RunInnerContent::T(crate::util::text_node(b)));
                }
                pos += len;
            }
            EG_RunInnerContent::Tab(_) | EG_RunInnerContent::Br(_) | EG_RunInnerContent::Cr(_) => {
                if pos < at {
                    left.run_inner_content.push(item.clone());
                } else {
                    right.run_inner_content.push(item.clone());
                }
                pos += 1;
            }
            EG_RunInnerContent::Other(_) => return None,
            // Runs holding drawings, fields, notes and the like are not split.
            _ => return None,
        }
    }
    Some((left, right))
}

/// Isolates the characters `start..end` of the text formed by the direct
/// runs of a paragraph, so that they occupy whole runs. Returns the range of
/// indices (into `p.p_content`) of those runs, or `None` when the range is
/// empty, out of bounds, or crosses content that cannot be split.
pub(crate) fn isolate_range(p: &mut wml::CT_P, start: usize, end: usize) -> Option<std::ops::Range<usize>> {
    if start >= end {
        return None;
    }
    // Split at `end` first, then at `start`, so indices before stay valid.
    for cut in [end, start] {
        let mut pos = 0usize;
        let mut i = 0usize;
        while i < p.p_content.len() {
            let EG_PContent::R(r) = &p.p_content[i] else {
                i += 1;
                continue;
            };
            let len = run_len(r);
            if cut > pos && cut < pos + len {
                let (a, b) = split_run(r, cut - pos)?;
                p.p_content[i] = EG_PContent::R(Box::new(a));
                p.p_content.insert(i + 1, EG_PContent::R(Box::new(b)));
                break;
            }
            pos += len;
            i += 1;
        }
    }
    let mut pos = 0usize;
    let mut first = None;
    let mut last = None;
    for (i, item) in p.p_content.iter().enumerate() {
        let EG_PContent::R(r) = item else { continue };
        let len = run_len(r);
        if len == 0 {
            continue;
        }
        if pos >= start && pos + len <= end {
            first.get_or_insert(i);
            last = Some(i);
        }
        pos += len;
    }
    let total: usize = p
        .p_content
        .iter()
        .filter_map(|c| {
            if let EG_PContent::R(r) = c {
                Some(run_len(r))
            } else {
                None
            }
        })
        .sum();
    if end > total {
        return None;
    }
    // Items between the first and the last run (markers, containers holding
    // no direct text) belong to the range too.
    let (first, last) = (first?, last?);
    Some(first..last + 1)
}

/// Character offsets (in the text of the direct runs) of the first
/// occurrence of `needle` among the direct runs of a paragraph. A match
/// cannot span content nested in containers (hyperlinks, fields, …): the
/// text on both sides of such content is searched separately.
pub(crate) fn find_in_direct_runs(p: &wml::CT_P, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    // Segments of direct-run text, with their start offsets.
    let mut segments: Vec<(usize, String)> = vec![(0, String::new())];
    let mut pos = 0usize;
    for item in &p.p_content {
        match item {
            EG_PContent::R(r) => {
                let t = text::run_text(r);
                pos += t.chars().count();
                segments.last_mut().expect("one segment").1.push_str(&t);
            }
            other => {
                let visible = text::runs(std::slice::from_ref(other))
                    .iter()
                    .any(|r| !text::run_text(r).is_empty());
                if visible {
                    segments.push((pos, String::new()));
                }
            }
        }
    }
    segments.into_iter().find_map(|(offset, segment)| {
        let byte = segment.find(needle)?;
        let start = offset + segment[..byte].chars().count();
        Some((start, start + needle.chars().count()))
    })
}

/// Largest numeric value of any namespace-qualified `…:id="N"` attribute in
/// an XML part. Annotation identifiers (comments, bookmarks, revisions,
/// content controls) must be unique; new ones are allocated above this.
pub(crate) fn max_numeric_id(xml: &[u8]) -> i64 {
    let mut max = 0i64;
    let needle = b":id=\"";
    let mut i = 0;
    while let Some(off) = xml[i..].windows(needle.len()).position(|w| w == needle) {
        let start = i + off + needle.len();
        i = start;
        let mut end = start;
        if xml.get(end) == Some(&b'-') {
            end += 1;
        }
        while xml.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if xml.get(end) != Some(&b'"') {
            continue;
        }
        if let Some(v) = std::str::from_utf8(&xml[start..end])
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
        {
            max = max.max(v);
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::RunMut;

    fn paragraph(parts: &[&str]) -> wml::CT_P {
        let mut p = wml::CT_P::default();
        for s in parts {
            let mut r = wml::CT_R::default();
            RunMut::new(&mut r).add_text(s);
            p.p_content.push(EG_PContent::R(Box::new(r)));
        }
        p
    }

    fn texts(p: &wml::CT_P) -> Vec<String> {
        p.p_content
            .iter()
            .filter_map(|c| {
                if let EG_PContent::R(r) = c {
                    Some(text::run_text(r))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn isolates_ranges_inside_and_across_runs() {
        let mut p = paragraph(&["Hello ", "wonderful world"]);
        let r = isolate_range(&mut p, 6, 15).unwrap();
        assert_eq!(texts(&p), ["Hello ", "wonderful", " world"]);
        assert_eq!(r, 1..2);

        let mut p = paragraph(&["abc", "def", "ghi"]);
        let r = isolate_range(&mut p, 1, 8).unwrap();
        assert_eq!(texts(&p), ["a", "bc", "def", "gh", "i"]);
        assert_eq!(r, 1..4);

        let mut p = paragraph(&["abc"]);
        assert_eq!(isolate_range(&mut p, 0, 3), Some(0..1));
        assert_eq!(texts(&p), ["abc"]);
        assert_eq!(isolate_range(&mut p, 2, 2), None);
        assert_eq!(isolate_range(&mut p, 1, 9), None);
    }

    #[test]
    fn splitting_keeps_formatting_and_unicode() {
        let mut p = wml::CT_P::default();
        let mut r = wml::CT_R::default();
        RunMut::new(&mut r).bold(true).add_text("Tiếng Việt");
        p.p_content.push(EG_PContent::R(Box::new(r)));
        let range = isolate_range(&mut p, 6, 10).unwrap();
        assert_eq!(texts(&p), ["Tiếng ", "Việt"]);
        let EG_PContent::R(r) = &p.p_content[range.start] else {
            panic!()
        };
        assert!(crate::run::Run::new(r).is_bold());
    }

    #[test]
    fn finds_text_in_direct_runs() {
        let p = paragraph(&["Rust ", "is fun"]);
        assert_eq!(find_in_direct_runs(&p, "is"), Some((5, 7)));
        assert_eq!(find_in_direct_runs(&p, "t i"), Some((3, 6)));
        assert_eq!(find_in_direct_runs(&p, "nope"), None);
        assert_eq!(find_in_direct_runs(&p, ""), None);
        // Text nested in a container splits the searchable text.
        let mut p = paragraph(&["foo ", "bar"]);
        let mut link_run = wml::CT_R::default();
        RunMut::new(&mut link_run).add_text("link");
        p.p_content.insert(
            1,
            EG_PContent::Hyperlink(Box::new(wml::CT_Hyperlink {
                p_content: vec![EG_PContent::R(Box::new(link_run))],
                ..Default::default()
            })),
        );
        assert_eq!(find_in_direct_runs(&p, "foo bar"), None);
        assert_eq!(find_in_direct_runs(&p, "bar"), Some((4, 7)));
        assert_eq!(isolate_range(&mut p, 4, 7), Some(2..3));
    }

    #[test]
    fn scans_numeric_ids() {
        let xml = br#"<w:p><w:bookmarkStart w:id="7" w:name="a"/><w:ins w:id="12"/><r:x r:id="rId99"/><w:footnote w:id="-1"/></w:p>"#;
        assert_eq!(max_numeric_id(xml), 12);
        assert_eq!(max_numeric_id(b"<none/>"), 0);
    }

    #[test]
    fn walks_all_paragraphs() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/><w:tbl><w:tr><w:tc><w:p/><w:p/></w:tc></w:tr></w:tbl><w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt></w:body></w:document>"#;
        let mut doc = wml::elements::DOCUMENT.parse(xml).unwrap();
        let mut n = 0;
        walk_blocks(&mut doc.body.as_mut().unwrap().block_level_elts, &mut |_| n += 1);
        assert_eq!(n, 4);
        let mut m = 0;
        walk_blocks_ref(&doc.body.as_ref().unwrap().block_level_elts, &mut |_| m += 1);
        assert_eq!(m, 4);
    }
}
