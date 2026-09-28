//! Cell comments (notes): the comments part (ECMA-376 Part 1 §18.7) and
//! the legacy VML drawing that gives each note its box on the sheet
//! (Part 4 §19).
//!
//! Excel's newer *threaded comments* live in separate parts; they are
//! preserved untouched but not created.
//!
//! ```
//! use openxml_xlsx::Workbook;
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.add_comment("B2", "Reviewer", "Check this value")?;
//! let comments = sheet.as_view().comments()?;
//! assert_eq!(comments[0].author, "Reviewer");
//! assert_eq!(comments[0].text.text(), "Check this value");
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{Package, PartName};
use openxml_schema::sml;
use openxml_xml::{Ns, RawElement, RawNode};

use crate::cell_ref::{CellRef, ToCellRef};
use crate::parts::SideValue;
use crate::rich_text::RichText;
use crate::util::{column_px, remove_relationship_and_orphans, row_px};
use crate::worksheet::{Worksheet, WorksheetMut};

/// Relationship type of threaded comments (Office 2019+).
const THREADED_COMMENTS: &str = "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment";

/// A cell comment (note).
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// The cell the comment belongs to.
    pub cell: CellRef,
    /// Author name.
    pub author: String,
    /// The text.
    pub text: RichText,
    /// Whether the note is always shown (not only on hover).
    pub visible: bool,
}

const VML_NAMESPACES: &str = r#"xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel""#;

/// The shape type of Excel notes (a text box).
const NOTE_SHAPETYPE: &str = r##"<v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/><v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype>"##;

fn parse_fragment(xml: &str) -> RawElement {
    let root = RawElement::parse(&format!("<xml {VML_NAMESPACES}>{xml}</xml>"))
        .expect("the VML templates are well-formed");
    root.elements().next().cloned().expect("one element")
}

/// A new VML drawing for notes with shape ids from block `idmap`.
fn new_vml(idmap: u32) -> RawElement {
    RawElement::parse(&format!(
        r#"<xml {VML_NAMESPACES}><o:shapelayout v:ext="edit"><o:idmap v:ext="edit" data="{idmap}"/></o:shapelayout>{NOTE_SHAPETYPE}</xml>"#
    ))
    .expect("the VML template is well-formed")
}

/// The id blocks (`o:idmap data`) a VML drawing claims.
fn idmaps(vml: &RawElement) -> Vec<u32> {
    vml.descendants()
        .into_iter()
        .filter(|e| e.name.is(Ns::O, "idmap"))
        .filter_map(|e| e.attr(Ns::NONE, "data"))
        .flat_map(|d| {
            d.split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect()
}

/// Number of an `_x0000_sNNNN` shape id.
fn shape_number(id: &str) -> Option<u32> {
    id.strip_prefix("_x0000_s")?.parse().ok()
}

/// The next free shape number of a VML drawing.
fn next_shape_number(vml: &RawElement) -> u32 {
    let max = vml
        .descendants()
        .into_iter()
        .filter_map(|e| {
            e.attr(Ns::NONE, "id")
                .or_else(|| e.attr(Ns::O, "spid"))
                .and_then(shape_number)
        })
        .max();
    match max {
        Some(m) => m + 1,
        None => idmaps(vml).first().copied().unwrap_or(1) * 1024 + 1,
    }
}

fn is_note(shape: &RawElement) -> bool {
    shape.name.is(Ns::V, "shape")
        && shape
            .child(Ns::XVML, "ClientData")
            .is_some_and(|c| c.attr(Ns::NONE, "ObjectType") == Some("Note"))
}

/// Zero-based (row, column) of a note shape.
fn note_cell(shape: &RawElement) -> Option<(u32, u32)> {
    let data = shape.child(Ns::XVML, "ClientData")?;
    let row = data.child(Ns::XVML, "Row")?.text().trim().parse().ok()?;
    let col = data.child(Ns::XVML, "Column")?.text().trim().parse().ok()?;
    Some((row, col))
}

fn note_visible(shape: &RawElement) -> bool {
    let styled = shape
        .attr(Ns::NONE, "style")
        .is_some_and(|s| s.replace(' ', "").contains("visibility:visible"));
    let flagged = shape
        .child(Ns::XVML, "ClientData")
        .is_some_and(|c| c.child(Ns::XVML, "Visible").is_some());
    styled || flagged
}

fn set_note_visible(shape: &mut RawElement, visible: bool) {
    let style = shape.attr(Ns::NONE, "style").unwrap_or("").to_owned();
    let mut parts: Vec<String> = style
        .split(';')
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty() && !p.replace(' ', "").starts_with("visibility:"))
        .collect();
    parts.push(format!(
        "visibility:{}",
        if visible { "visible" } else { "hidden" }
    ));
    shape.set_attr(Ns::NONE, "style", parts.join(";"));
    let Some(data) = shape.children.iter_mut().find_map(|c| match c {
        RawNode::Element(e) if e.name.is(Ns::XVML, "ClientData") => Some(e),
        _ => None,
    }) else {
        return;
    };
    data.children
        .retain(|c| !matches!(c, RawNode::Element(e) if e.name.is(Ns::XVML, "Visible")));
    if visible {
        let flag = parse_fragment("<x:Visible/>");
        data.children.push(RawNode::Element(flag));
    }
}

/// A note shape for the cell at zero-based `(row, col)`.
fn note_shape(
    ws: &sml::CT_Worksheet,
    number: u32,
    z: usize,
    row: u32,
    col: u32,
    visible: bool,
) -> RawElement {
    // Excel's default note box: two columns right of the cell, four rows high.
    let left_col = col + 1;
    let top_row = row.saturating_sub(1);
    let top_off = if row == 0 { 2 } else { 10 };
    let x_px: f64 = (1..=left_col).map(|c| column_px(ws, c)).sum::<f64>() + 15.0;
    let y_px: f64 = (1..=top_row).map(|r| row_px(ws, r)).sum::<f64>() + f64::from(top_off);
    let pt = |px: f64| format!("{}", (px * 0.75 * 100.0).round() / 100.0);
    let anchor = format!(
        "{}, 15, {}, {}, {}, 15, {}, 4",
        left_col,
        top_row,
        top_off,
        col + 3,
        top_row + 4
    );
    let visibility = if visible { "visible" } else { "hidden" };
    let flag = if visible { "<x:Visible/>" } else { "" };
    parse_fragment(&format!(
        r##"<v:shape id="_x0000_s{number}" type="#_x0000_t202" style="position:absolute;margin-left:{x}pt;margin-top:{y}pt;width:108pt;height:59.25pt;z-index:{z};visibility:{visibility}" fillcolor="#ffffe1" o:insetmode="auto"><v:fill color2="#ffffe1"/><v:shadow on="t" color="black" obscured="t"/><v:path o:connecttype="none"/><v:textbox style="mso-direction-alt:auto"><div style="text-align:left"></div></v:textbox><x:ClientData ObjectType="Note"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{anchor}</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{row}</x:Row><x:Column>{col}</x:Column>{flag}</x:ClientData></v:shape>"##,
        x = pt(x_px),
        y = pt(y_px),
    ))
}

/// Whether a VML drawing still holds shapes (anything besides the layout and shape types).
fn has_shapes(vml: &RawElement) -> bool {
    vml.elements()
        .any(|e| !(e.name.is(Ns::O, "shapelayout") || e.name.is(Ns::V, "shapetype")))
}

fn comments_part_of(pkg: &Package, sheet: &PartName) -> Option<(String, PartName)> {
    let rels = pkg.relationships(Some(sheet))?;
    rels.by_type(rel_types::COMMENTS)
        .filter(|r| !r.is_external())
        .find_map(|r| {
            PartName::resolve(Some(sheet), &r.target)
                .ok()
                .filter(|p| pkg.contains(p))
                .map(|p| (r.id.clone(), p))
        })
}

fn vml_part_of(ws: &sml::CT_Worksheet, pkg: &Package, sheet: &PartName) -> Option<PartName> {
    let rid = ws.legacy_drawing.as_ref()?.r_id.as_deref()?;
    pkg.relationship_target(Some(sheet), rid)
        .filter(|p| pkg.contains(p))
}

fn comment_cell(c: &sml::CT_Comment) -> Option<CellRef> {
    CellRef::parse(c.ref_.as_deref()?).ok()
}

impl Worksheet<'_> {
    /// The comments part of the sheet, if any.
    pub fn comments_part(&self) -> Option<PartName> {
        comments_part_of(self.env.package, self.part).map(|(_, p)| p)
    }

    /// Whether the sheet has threaded comments (Office 365), which are
    /// preserved but not exposed by [`Worksheet::comments`] beyond their
    /// legacy note placeholders.
    pub fn has_threaded_comments(&self) -> bool {
        self.env
            .package
            .relationships(Some(self.part))
            .is_some_and(|r| r.by_type(THREADED_COMMENTS).next().is_some())
    }

    /// The comments of the sheet in document order.
    pub fn comments(&self) -> Result<Vec<Comment>> {
        let Some(part) = self.comments_part() else {
            return Ok(Vec::new());
        };
        let comments = self.env.side.peek_comments(self.env.package, &part)?;
        let vml = match vml_part_of(self.data, self.env.package, self.part) {
            Some(p) => Some(self.env.side.peek_vml(self.env.package, &p)?),
            None => None,
        };
        let authors = comments
            .authors
            .as_ref()
            .map(|a| a.author.as_slice())
            .unwrap_or(&[]);
        let mut out = Vec::new();
        for c in comments.comment_list.iter().flat_map(|l| l.comment.iter()) {
            let Some(cell) = comment_cell(c) else { continue };
            let visible = vml.as_ref().is_some_and(|v| {
                v.elements()
                    .filter(|e| is_note(e))
                    .find(|e| note_cell(e) == Some((cell.row() - 1, cell.col() - 1)))
                    .is_some_and(note_visible)
            });
            out.push(Comment {
                cell,
                author: c
                    .author_id
                    .and_then(|i| authors.get(i as usize))
                    .cloned()
                    .unwrap_or_default(),
                text: c.text.as_deref().map(RichText::from_rst).unwrap_or_default(),
                visible,
            });
        }
        Ok(out)
    }

    /// The comment of a cell.
    pub fn comment(&self, at: impl ToCellRef) -> Result<Option<Comment>> {
        let at = at.to_cell_ref()?;
        Ok(self.comments()?.into_iter().find(|c| c.cell == at))
    }
}

impl WorksheetMut<'_> {
    /// Adds a plain-text comment to a cell (replacing an existing one).
    pub fn add_comment(&mut self, at: impl ToCellRef, author: &str, text: &str) -> Result<()> {
        self.add_rich_comment(at, author, &RichText::new().push(text))
    }

    /// Adds a comment with formatted text to a cell (replacing an existing one).
    pub fn add_rich_comment(&mut self, at: impl ToCellRef, author: &str, text: &RichText) -> Result<()> {
        let at = at.to_cell_ref()?;
        let comments_part = self.comments_part_or_create()?;
        let vml_part = self.vml_part_or_create()?;
        let comments = self.side.comments(self.package, &comments_part)?;
        let authors = &mut comments.authors.get_or_insert_with(Box::default).author;
        let author_id = match authors.iter().position(|a| a == author) {
            Some(i) => i,
            None => {
                authors.push(author.to_owned());
                authors.len() - 1
            }
        } as u32;
        let list = &mut comments.comment_list.get_or_insert_with(Box::default).comment;
        let key = |c: &sml::CT_Comment| comment_cell(c).map(|r| (r.row(), r.col()));
        let record = sml::CT_Comment {
            ref_: Some(at.to_string()),
            author_id: Some(author_id),
            text: Some(Box::new(text.to_rst())),
            ..Default::default()
        };
        match list.iter().position(|c| key(c) == Some((at.row(), at.col()))) {
            Some(i) => {
                list[i].author_id = record.author_id;
                list[i].text = record.text;
            }
            None => {
                let i = list
                    .iter()
                    .position(|c| key(c).is_some_and(|k| k > (at.row(), at.col())))
                    .unwrap_or(list.len());
                list.insert(i, record);
            }
        }
        let (row, col) = (at.row() - 1, at.col() - 1);
        let exists = self
            .side
            .peek_vml(self.package, &vml_part)?
            .elements()
            .any(|e| is_note(e) && note_cell(e) == Some((row, col)));
        if !exists {
            let data: &sml::CT_Worksheet = self.data;
            let vml = self.side.vml(self.package, &vml_part)?;
            if vml
                .child(Ns::V, "shapetype")
                .is_none_or(|t| t.attr(Ns::NONE, "id") != Some("_x0000_t202"))
                && !vml
                    .elements()
                    .any(|e| e.name.is(Ns::V, "shapetype") && e.attr(Ns::NONE, "id") == Some("_x0000_t202"))
            {
                vml.children
                    .push(RawNode::Element(parse_fragment(NOTE_SHAPETYPE)));
            }
            let number = next_shape_number(vml);
            let z = vml.elements().filter(|e| e.name.is(Ns::V, "shape")).count() + 1;
            vml.children
                .push(RawNode::Element(note_shape(data, number, z, row, col, false)));
        }
        Ok(())
    }

    /// Shows a note permanently or only on hover. Returns whether the cell has a note.
    pub fn set_comment_visible(&mut self, at: impl ToCellRef, visible: bool) -> Result<bool> {
        let at = at.to_cell_ref()?;
        let Some(vml_part) = vml_part_of(self.data, self.package, self.part) else {
            return Ok(false);
        };
        let vml = self.side.vml(self.package, &vml_part)?;
        let key = Some((at.row() - 1, at.col() - 1));
        let Some(shape) = vml.children.iter_mut().find_map(|c| match c {
            RawNode::Element(e) if is_note(e) && note_cell(e) == key => Some(e),
            _ => None,
        }) else {
            return Ok(false);
        };
        set_note_visible(shape, visible);
        Ok(true)
    }

    /// Removes the comment of a cell. The comments and VML parts are
    /// removed when they become empty. Returns whether there was a comment.
    pub fn remove_comment(&mut self, at: impl ToCellRef) -> Result<bool> {
        let at = at.to_cell_ref()?;
        let Some((rel_id, part)) = comments_part_of(self.package, self.part) else {
            return Ok(false);
        };
        let comments = self.side.comments(self.package, &part)?;
        let Some(list) = comments.comment_list.as_mut() else {
            return Ok(false);
        };
        let before = list.comment.len();
        list.comment.retain(|c| comment_cell(c) != Some(at));
        if list.comment.len() == before {
            return Ok(false);
        }
        if list.comment.is_empty() {
            for gone in remove_relationship_and_orphans(self.package, self.part, &rel_id) {
                self.side.forget(&gone);
            }
        }
        if let Some(vml_part) = vml_part_of(self.data, self.package, self.part) {
            let vml = self.side.vml(self.package, &vml_part)?;
            let key = Some((at.row() - 1, at.col() - 1));
            vml.children
                .retain(|c| !matches!(c, RawNode::Element(e) if is_note(e) && note_cell(e) == key));
            if !has_shapes(vml) {
                let rid = self
                    .data
                    .legacy_drawing
                    .take()
                    .and_then(|l| l.r_id)
                    .unwrap_or_default();
                for gone in remove_relationship_and_orphans(self.package, self.part, &rid) {
                    self.side.forget(&gone);
                }
            }
        }
        Ok(true)
    }

    fn comments_part_or_create(&mut self) -> Result<PartName> {
        if let Some((_, p)) = comments_part_of(self.package, self.part) {
            return Ok(p);
        }
        let name = self.package.next_part_name("/xl/comments{}.xml")?;
        let value = sml::CT_Comments {
            authors: Some(Box::default()),
            comment_list: Some(Box::default()),
            ..Default::default()
        };
        self.side
            .create(self.package, &name, SideValue::Comments(Box::new(value)))?;
        self.package
            .add_relationship(Some(self.part), rel_types::COMMENTS, &name)?;
        Ok(name)
    }

    fn vml_part_or_create(&mut self) -> Result<PartName> {
        if let Some(p) = vml_part_of(self.data, self.package, self.part) {
            return Ok(p);
        }
        // Shape id blocks must not overlap between the VML drawings of a workbook.
        let mut used = Vec::new();
        let vml_parts: Vec<PartName> = self
            .package
            .parts()
            .filter(|(_, p)| p.content_type() == ct::VML_DRAWING)
            .map(|(n, _)| n.clone())
            .collect();
        for p in vml_parts {
            if let Ok(v) = self.side.peek_vml(self.package, &p) {
                used.extend(idmaps(&v));
            }
        }
        let idmap = used.iter().max().map_or(1, |m| m + 1);
        let name = self.package.next_part_name("/xl/drawings/vmlDrawing{}.vml")?;
        self.side
            .create(self.package, &name, SideValue::Vml(Box::new(new_vml(idmap))))?;
        let rid = self
            .package
            .add_relationship(Some(self.part), rel_types::VML_DRAWING, &name)?;
        if self.data.legacy_drawing.is_some() {
            return Err(Error::InvalidDocument(
                "the sheet's legacy drawing points to a missing part".into(),
            ));
        }
        self.data.legacy_drawing = Some(Box::new(sml::CT_LegacyDrawing {
            r_id: Some(rid),
            ..Default::default()
        }));
        Ok(name)
    }
}

/// Moves the comments of a sheet when rows or columns are inserted or
/// deleted: `map` receives a 1-based row (or column) and returns its new
/// number, `None` when it was deleted (its comment is dropped).
pub(crate) fn remap_comments(comments: &mut sml::CT_Comments, rows: bool, map: &dyn Fn(u32) -> Option<u32>) {
    let Some(list) = comments.comment_list.as_mut() else {
        return;
    };
    list.comment.retain_mut(|c| {
        let Some(cell) = comment_cell(c) else {
            return true;
        };
        let moved = if rows {
            map(cell.row()).and_then(|r| CellRef::new(r, cell.col()).ok())
        } else {
            map(cell.col()).and_then(|col| CellRef::new(cell.row(), col).ok())
        };
        match moved {
            Some(m) => {
                c.ref_ = Some(m.to_string());
                true
            }
            None => false,
        }
    });
}

/// Moves the note shapes of a VML drawing like [`remap_comments`].
pub(crate) fn remap_notes(vml: &mut RawElement, rows: bool, map: &dyn Fn(u32) -> Option<u32>) {
    vml.children.retain_mut(|node| {
        let RawNode::Element(shape) = node else {
            return true;
        };
        if !is_note(shape) {
            return true;
        }
        let Some((row, col)) = note_cell(shape) else {
            return true;
        };
        let old = if rows { row } else { col };
        let Some(new) = map(old + 1).map(|n| n - 1) else {
            return false;
        };
        let delta = i64::from(new) - i64::from(old);
        let Some(data) = shape.children.iter_mut().find_map(|c| match c {
            RawNode::Element(e) if e.name.is(Ns::XVML, "ClientData") => Some(e),
            _ => None,
        }) else {
            return true;
        };
        for child in data.children.iter_mut() {
            let RawNode::Element(e) = child else { continue };
            let target = if rows { "Row" } else { "Column" };
            if e.name.is(Ns::XVML, target) {
                e.children = vec![RawNode::Text(new.to_string())];
            } else if e.name.is(Ns::XVML, "Anchor") {
                let mut values: Vec<i64> = e
                    .text()
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                if values.len() == 8 {
                    let (a, b) = if rows { (2, 6) } else { (0, 4) };
                    values[a] = (values[a] + delta).max(0);
                    values[b] = (values[b] + delta).max(0);
                    let text = values.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
                    e.children = vec![RawNode::Text(text)];
                }
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vml_templates() {
        let mut vml = new_vml(3);
        assert_eq!(idmaps(&vml), [3]);
        assert_eq!(next_shape_number(&vml), 3 * 1024 + 1);
        let ws = sml::CT_Worksheet::default();
        vml.children
            .push(RawNode::Element(note_shape(&ws, 3073, 1, 1, 2, false)));
        assert_eq!(next_shape_number(&vml), 3074);
        let shape = vml.elements().find(|e| is_note(e)).unwrap();
        assert_eq!(note_cell(shape), Some((1, 2)));
        assert!(!note_visible(shape));
        assert!(has_shapes(&vml));
        let xml = vml.to_xml();
        assert!(xml.starts_with("<xml xmlns:v="), "{xml}");
        assert_eq!(
            xml.matches("xmlns:v=").count(),
            1,
            "namespaces are declared once: {xml}"
        );
        assert!(xml.contains("<x:Row>1</x:Row><x:Column>2</x:Column>"), "{xml}");
        assert!(
            xml.contains("margin-left:155.25pt"),
            "D column left edge + 15px: {xml}"
        );
        let back = RawElement::parse(&xml).unwrap();
        assert_eq!(back, vml);
    }

    #[test]
    fn visibility_and_remapping() {
        let ws = sml::CT_Worksheet::default();
        let mut vml = new_vml(1);
        vml.children
            .push(RawNode::Element(note_shape(&ws, 1025, 1, 4, 0, false)));
        let shape = match vml.children.last_mut().unwrap() {
            RawNode::Element(e) => e,
            RawNode::Text(_) => unreachable!(),
        };
        set_note_visible(shape, true);
        assert!(note_visible(shape));
        assert!(
            shape
                .attr(Ns::NONE, "style")
                .unwrap()
                .ends_with("visibility:visible")
        );
        set_note_visible(shape, false);
        assert!(!note_visible(shape));
        // Insert two rows above row 5 (zero-based row 4).
        remap_notes(&mut vml, true, &|r| Some(if r >= 3 { r + 2 } else { r }));
        let shape = vml.elements().find(|e| is_note(e)).unwrap();
        assert_eq!(note_cell(shape), Some((6, 0)));
        let anchor = shape
            .child(Ns::XVML, "ClientData")
            .unwrap()
            .child(Ns::XVML, "Anchor")
            .unwrap()
            .text();
        assert_eq!(anchor, "1, 15, 5, 10, 3, 15, 9, 4");
        remap_notes(&mut vml, true, &|r| (r != 7).then_some(r));
        assert!(!has_shapes(&vml), "the note of a deleted row is dropped");
    }
}
