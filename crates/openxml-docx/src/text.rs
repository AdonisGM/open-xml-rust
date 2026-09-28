//! Traversal of WordprocessingML content and plain-text extraction.
//!
//! Content controls (`w:sdt`), custom XML markup, smart tags, hyperlinks,
//! simple fields and tracked insertions are transparent containers: their
//! content counts as part of the surrounding text. Tracked deletions
//! (`w:del`, `w:moveFrom`) and field instructions are not text.

use openxml_schema::wml::{
    self, CT_RunTrackChange_Choice, EG_BlockLevelElts, EG_ContentBlockContent, EG_ContentCellContent,
    EG_ContentRowContent, EG_PContent, EG_RunInnerContent,
};

/// A block-level item: a paragraph or a table.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BlockRef<'a> {
    /// A paragraph.
    P(&'a wml::CT_P),
    /// A table.
    Tbl(&'a wml::CT_Tbl),
}

fn push_content_blocks<'a>(items: &'a [EG_ContentBlockContent], out: &mut Vec<BlockRef<'a>>) {
    for item in items {
        match item {
            EG_ContentBlockContent::P(p) => out.push(BlockRef::P(p)),
            EG_ContentBlockContent::Tbl(t) => out.push(BlockRef::Tbl(t)),
            EG_ContentBlockContent::Sdt(sdt) => {
                if let Some(c) = &sdt.sdt_content {
                    push_content_blocks(&c.content_block_content, out);
                }
            }
            EG_ContentBlockContent::CustomXml(x) => push_content_blocks(&x.content_block_content, out),
            _ => {}
        }
    }
}

/// Paragraphs and tables of a block-level container in document order,
/// looking through content controls and custom XML.
pub(crate) fn blocks(items: &[EG_BlockLevelElts]) -> Vec<BlockRef<'_>> {
    let mut out = Vec::new();
    for item in items {
        match item {
            EG_BlockLevelElts::P(p) => out.push(BlockRef::P(p)),
            EG_BlockLevelElts::Tbl(t) => out.push(BlockRef::Tbl(t)),
            EG_BlockLevelElts::Sdt(sdt) => {
                if let Some(c) = &sdt.sdt_content {
                    push_content_blocks(&c.content_block_content, &mut out);
                }
            }
            EG_BlockLevelElts::CustomXml(x) => push_content_blocks(&x.content_block_content, &mut out),
            _ => {}
        }
    }
    out
}

fn push_rows<'a>(items: &'a [EG_ContentRowContent], out: &mut Vec<&'a wml::CT_Row>) {
    for item in items {
        match item {
            EG_ContentRowContent::Tr(r) => out.push(r),
            EG_ContentRowContent::Sdt(sdt) => {
                if let Some(c) = &sdt.sdt_content {
                    push_rows(&c.content_row_content, out);
                }
            }
            EG_ContentRowContent::CustomXml(x) => push_rows(&x.content_row_content, out),
            _ => {}
        }
    }
}

/// Rows of a table (looking through content controls and custom XML).
pub(crate) fn rows(t: &wml::CT_Tbl) -> Vec<&wml::CT_Row> {
    let mut out = Vec::new();
    push_rows(&t.content_row_content, &mut out);
    out
}

fn push_cells<'a>(items: &'a [EG_ContentCellContent], out: &mut Vec<&'a wml::CT_Tc>) {
    for item in items {
        match item {
            EG_ContentCellContent::Tc(c) => out.push(c),
            EG_ContentCellContent::Sdt(sdt) => {
                if let Some(c) = &sdt.sdt_content {
                    push_cells(&c.content_cell_content, out);
                }
            }
            EG_ContentCellContent::CustomXml(x) => push_cells(&x.content_cell_content, out),
            _ => {}
        }
    }
}

/// Cells of a row (looking through content controls and custom XML).
pub(crate) fn cells(r: &wml::CT_Row) -> Vec<&wml::CT_Tc> {
    let mut out = Vec::new();
    push_cells(&r.content_cell_content, &mut out);
    out
}

fn push_mut_rows<'a>(items: &'a mut [EG_ContentRowContent], out: &mut Vec<&'a mut wml::CT_Row>) {
    for item in items {
        match item {
            EG_ContentRowContent::Tr(r) => out.push(r),
            EG_ContentRowContent::Sdt(sdt) => {
                if let Some(c) = &mut sdt.sdt_content {
                    push_mut_rows(&mut c.content_row_content, out);
                }
            }
            EG_ContentRowContent::CustomXml(x) => push_mut_rows(&mut x.content_row_content, out),
            _ => {}
        }
    }
}

/// Mutable rows of a table.
pub(crate) fn rows_mut(t: &mut wml::CT_Tbl) -> Vec<&mut wml::CT_Row> {
    let mut out = Vec::new();
    push_mut_rows(&mut t.content_row_content, &mut out);
    out
}

fn push_mut_cells<'a>(items: &'a mut [EG_ContentCellContent], out: &mut Vec<&'a mut wml::CT_Tc>) {
    for item in items {
        match item {
            EG_ContentCellContent::Tc(c) => out.push(c),
            EG_ContentCellContent::Sdt(sdt) => {
                if let Some(c) = &mut sdt.sdt_content {
                    push_mut_cells(&mut c.content_cell_content, out);
                }
            }
            EG_ContentCellContent::CustomXml(x) => push_mut_cells(&mut x.content_cell_content, out),
            _ => {}
        }
    }
}

/// Mutable cells of a row.
pub(crate) fn cells_mut(r: &mut wml::CT_Row) -> Vec<&mut wml::CT_Tc> {
    let mut out = Vec::new();
    push_mut_cells(&mut r.content_cell_content, &mut out);
    out
}

fn push_track_change_runs<'a>(items: &'a [CT_RunTrackChange_Choice], out: &mut Vec<&'a wml::CT_R>) {
    for item in items {
        match item {
            CT_RunTrackChange_Choice::WR(r) => out.push(r),
            CT_RunTrackChange_Choice::CustomXml(x) => push_runs(&x.p_content, out),
            CT_RunTrackChange_Choice::SmartTag(x) => push_runs(&x.p_content, out),
            CT_RunTrackChange_Choice::Sdt(x) => {
                if let Some(c) = &x.sdt_content {
                    push_runs(&c.p_content, out);
                }
            }
            CT_RunTrackChange_Choice::Dir(x) => push_runs(&x.p_content, out),
            CT_RunTrackChange_Choice::Bdo(x) => push_runs(&x.p_content, out),
            CT_RunTrackChange_Choice::Ins(x) | CT_RunTrackChange_Choice::MoveTo(x) => {
                push_track_change_runs(&x.choice, out)
            }
            _ => {}
        }
    }
}

fn push_runs<'a>(items: &'a [EG_PContent], out: &mut Vec<&'a wml::CT_R>) {
    for item in items {
        match item {
            EG_PContent::R(r) => out.push(r),
            EG_PContent::Hyperlink(h) => push_runs(&h.p_content, out),
            EG_PContent::FldSimple(f) => push_runs(&f.p_content, out),
            EG_PContent::CustomXml(x) => push_runs(&x.p_content, out),
            EG_PContent::SmartTag(x) => push_runs(&x.p_content, out),
            EG_PContent::Sdt(x) => {
                if let Some(c) = &x.sdt_content {
                    push_runs(&c.p_content, out);
                }
            }
            EG_PContent::Dir(x) => push_runs(&x.p_content, out),
            EG_PContent::Bdo(x) => push_runs(&x.p_content, out),
            EG_PContent::Ins(x) | EG_PContent::MoveTo(x) => push_track_change_runs(&x.choice, out),
            _ => {}
        }
    }
}

/// Visible runs of paragraph content in document order (deleted runs excluded).
pub(crate) fn runs(items: &[EG_PContent]) -> Vec<&wml::CT_R> {
    let mut out = Vec::new();
    push_runs(items, &mut out);
    out
}

fn push_runs_mut<'a>(items: &'a mut [EG_PContent], out: &mut Vec<&'a mut wml::CT_R>) {
    for item in items {
        match item {
            EG_PContent::R(r) => out.push(r),
            EG_PContent::Hyperlink(h) => push_runs_mut(&mut h.p_content, out),
            EG_PContent::FldSimple(f) => push_runs_mut(&mut f.p_content, out),
            EG_PContent::CustomXml(x) => push_runs_mut(&mut x.p_content, out),
            EG_PContent::SmartTag(x) => push_runs_mut(&mut x.p_content, out),
            EG_PContent::Sdt(x) => {
                if let Some(c) = &mut x.sdt_content {
                    push_runs_mut(&mut c.p_content, out);
                }
            }
            EG_PContent::Dir(x) => push_runs_mut(&mut x.p_content, out),
            EG_PContent::Bdo(x) => push_runs_mut(&mut x.p_content, out),
            _ => {}
        }
    }
}

/// Mutable visible runs of paragraph content (runs inside tracked changes are not edited).
pub(crate) fn runs_mut(items: &mut [EG_PContent]) -> Vec<&mut wml::CT_R> {
    let mut out = Vec::new();
    push_runs_mut(items, &mut out);
    out
}

/// Appends the text of a run.
pub(crate) fn push_run_text(r: &wml::CT_R, out: &mut String) {
    for c in &r.run_inner_content {
        match c {
            EG_RunInnerContent::T(t) => out.push_str(&t.value),
            EG_RunInnerContent::Tab(_) | EG_RunInnerContent::Ptab(_) => out.push('\t'),
            EG_RunInnerContent::Br(_) | EG_RunInnerContent::Cr(_) => out.push('\n'),
            EG_RunInnerContent::NoBreakHyphen(_) => out.push('-'),
            EG_RunInnerContent::Sym(s) => {
                if let Some(ch) = s
                    .char
                    .as_ref()
                    .and_then(|h| h.to_u64())
                    .and_then(|v| char::from_u32(v as u32))
                {
                    out.push(ch);
                }
            }
            _ => {}
        }
    }
}

/// Text of a run.
pub(crate) fn run_text(r: &wml::CT_R) -> String {
    let mut s = String::new();
    push_run_text(r, &mut s);
    s
}

/// Text of a paragraph.
pub(crate) fn paragraph_text(p: &wml::CT_P) -> String {
    let mut s = String::new();
    for r in runs(&p.p_content) {
        push_run_text(r, &mut s);
    }
    s
}

/// Text of a table cell: its blocks joined by newlines.
pub(crate) fn cell_text(c: &wml::CT_Tc) -> String {
    blocks_text(&blocks(&c.block_level_elts))
}

/// Text of a table: cells separated by tabs, rows by newlines.
pub(crate) fn table_text(t: &wml::CT_Tbl) -> String {
    rows(t)
        .iter()
        .map(|r| {
            cells(r)
                .iter()
                .map(|c| cell_text(c))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text of a sequence of blocks joined by newlines.
pub(crate) fn blocks_text(blocks: &[BlockRef<'_>]) -> String {
    blocks
        .iter()
        .map(|b| match b {
            BlockRef::P(p) => paragraph_text(p),
            BlockRef::Tbl(t) => table_text(t),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// All `w:t` text nodes of a run list (for search and replace).
pub(crate) fn text_nodes_mut(runs: Vec<&mut wml::CT_R>) -> Vec<&mut wml::CT_Text> {
    let mut out = Vec::new();
    for r in runs {
        for c in &mut r.run_inner_content {
            if let EG_RunInnerContent::T(t) = c {
                out.push(&mut **t);
            }
        }
    }
    out
}

/// Replaces every occurrence of `from` by `to` in a sequence of text nodes,
/// also when an occurrence spans several nodes. Returns the number of
/// replacements.
pub(crate) fn replace_in_nodes(nodes: &mut [&mut wml::CT_Text], from: &str, to: &str) -> usize {
    if from.is_empty() || nodes.is_empty() {
        return 0;
    }
    let full: String = nodes.iter().map(|t| t.value.as_str()).collect();
    let matches: Vec<usize> = full.match_indices(from).map(|(i, _)| i).collect();
    if matches.is_empty() {
        return 0;
    }
    // Start offset of every node inside `full`.
    let lens: Vec<usize> = nodes.iter().map(|t| t.value.len()).collect();
    let mut starts = Vec::with_capacity(nodes.len());
    let mut acc = 0;
    for len in &lens {
        starts.push(acc);
        acc += len;
    }
    let locate = |pos: usize, is_end: bool| -> (usize, usize) {
        // For an end position prefer the node where the match ends (not the next one).
        let idx = if is_end {
            starts.iter().rposition(|&s| s < pos).unwrap_or(0)
        } else {
            starts
                .iter()
                .enumerate()
                .rposition(|(i, &s)| s <= pos && (pos < s + lens[i]))
                .unwrap_or(0)
        };
        (idx, pos - starts[idx])
    };
    // Apply from the last match so earlier offsets stay valid.
    for &m in matches.iter().rev() {
        let (si, so) = locate(m, false);
        let (ei, eo) = locate(m + from.len(), true);
        if si == ei {
            nodes[si].value.replace_range(so..eo, to);
        } else {
            nodes[si].value.truncate(so);
            nodes[si].value.push_str(to);
            for node in nodes.iter_mut().take(ei).skip(si + 1) {
                node.value.clear();
            }
            nodes[ei].value.replace_range(..eo, "");
        }
        crate::util::fix_space(nodes[si]);
        crate::util::fix_space(nodes[ei]);
    }
    matches.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxml_schema::wml::elements::DOCUMENT;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn body(xml: &str) -> wml::CT_Body {
        let doc = DOCUMENT
            .parse(&format!(
                r#"<w:document xmlns:w="{W}"><w:body>{xml}</w:body></w:document>"#
            ))
            .unwrap();
        *doc.body.unwrap()
    }

    #[test]
    fn paragraph_text_includes_containers_but_not_deletions() {
        let b = body(
            r#"<w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r>
               <w:hyperlink><w:r><w:t>link</w:t></w:r></w:hyperlink>
               <w:ins w:id="1" w:author="x"><w:r><w:t>ins</w:t></w:r></w:ins>
               <w:del w:id="2" w:author="x"><w:r><w:delText>gone</w:delText></w:r></w:del>
               <w:sdt><w:sdtContent><w:r><w:t>sdt</w:t></w:r></w:sdtContent></w:sdt>
               <w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple>
               <w:r><w:instrText>IGNORED</w:instrText><w:br/><w:noBreakHyphen/><w:sym w:font="Symbol" w:char="0041"/></w:r></w:p>"#,
        );
        let bl = blocks(&b.block_level_elts);
        let BlockRef::P(p) = bl[0] else { panic!() };
        assert_eq!(paragraph_text(p), "A\tBlinkinssdt1\n-A");
    }

    #[test]
    fn blocks_look_through_content_controls() {
        let b = body(
            r#"<w:p><w:r><w:t>1</w:t></w:r></w:p>
               <w:sdt><w:sdtContent><w:p><w:r><w:t>2</w:t></w:r></w:p>
                 <w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
                 <w:sdt><w:sdtContent><w:tr><w:tc><w:p><w:r><w:t>d</w:t></w:r></w:p></w:tc></w:tr></w:sdtContent></w:sdt></w:tbl>
               </w:sdtContent></w:sdt>"#,
        );
        let bl = blocks(&b.block_level_elts);
        assert_eq!(bl.len(), 3);
        assert_eq!(blocks_text(&bl), "1\n2\na\tb\nc\nd");
        let BlockRef::Tbl(t) = bl[2] else { panic!() };
        assert_eq!(rows(t).len(), 2);
        assert_eq!(cells(rows(t)[0]).len(), 2);
    }

    #[test]
    fn replace_within_and_across_nodes() {
        let mk = |parts: &[&str]| -> Vec<wml::CT_Text> {
            parts
                .iter()
                .map(|p| wml::CT_Text {
                    value: (*p).to_owned(),
                    ..Default::default()
                })
                .collect()
        };
        let mut v = mk(&["Hello wor", "ld, world!"]);
        let mut refs: Vec<&mut wml::CT_Text> = v.iter_mut().collect();
        assert_eq!(replace_in_nodes(&mut refs, "world", "Rust"), 2);
        let joined: String = v.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(joined, "Hello Rust, Rust!");
        assert_eq!(v[0].value, "Hello Rust");

        let mut v = mk(&["ab", "c", "de"]);
        let mut refs: Vec<&mut wml::CT_Text> = v.iter_mut().collect();
        assert_eq!(replace_in_nodes(&mut refs, "bcd", "X"), 1);
        assert_eq!(
            v.iter().map(|t| t.value.as_str()).collect::<Vec<_>>(),
            ["aX", "", "e"]
        );

        let mut v = mk(&["aaa"]);
        let mut refs: Vec<&mut wml::CT_Text> = v.iter_mut().collect();
        assert_eq!(replace_in_nodes(&mut refs, "a", "bb"), 3);
        assert_eq!(v[0].value, "bbbbbb");

        let mut v = mk(&["x"]);
        let mut refs: Vec<&mut wml::CT_Text> = v.iter_mut().collect();
        assert_eq!(replace_in_nodes(&mut refs, "", "y"), 0);
        assert_eq!(replace_in_nodes(&mut refs, "z", "y"), 0);
        assert_eq!(replace_in_nodes(&mut [], "z", "y"), 0);

        let mut v = mk(&["end", " x"]);
        let mut refs: Vec<&mut wml::CT_Text> = v.iter_mut().collect();
        assert_eq!(replace_in_nodes(&mut refs, "d x", "d "), 1);
        assert_eq!(v[0].value, "end ");
        assert_eq!(v[0].xml_space.as_deref(), Some("preserve"));
    }

    #[test]
    fn mutable_traversals_reach_nested_runs() {
        let mut b = body(
            r#"<w:p><w:r><w:t>a</w:t></w:r><w:hyperlink><w:r><w:t>b</w:t></w:r></w:hyperlink><w:sdt><w:sdtContent><w:r><w:t>c</w:t></w:r></w:sdtContent></w:sdt></w:p>
               <w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#,
        );
        let EG_BlockLevelElts::P(p) = &mut b.block_level_elts[0] else {
            panic!()
        };
        assert_eq!(runs_mut(&mut p.p_content).len(), 3);
        assert_eq!(text_nodes_mut(runs_mut(&mut p.p_content)).len(), 3);
        let EG_BlockLevelElts::Tbl(t) = &mut b.block_level_elts[1] else {
            panic!()
        };
        let mut rs = rows_mut(t);
        assert_eq!(cells_mut(rs[0]).len(), 1);
        assert_eq!(rs.len(), 1);
        assert_eq!(run_text(&wml::CT_R::default()), "");
        let _ = &mut rs;
    }
}
