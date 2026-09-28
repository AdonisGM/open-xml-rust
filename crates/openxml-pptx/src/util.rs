//! Internal helpers: typed XML fragments and shape-tree navigation.

use openxml_core::Length;
use openxml_schema::{dml, pml};
use openxml_xml::{Ns, RawElement, XmlRead, XmlReader, XmlWriter};

use crate::shape::{coord, coordinate};

/// Namespace declarations for fragments.
pub(crate) const FRAGMENT_NS: &str = concat!(
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#
);

/// Parses a PresentationML/DrawingML fragment (a single element using the
/// `p:`, `a:` and `r:` prefixes) into a typed value.
pub(crate) fn parse_fragment<T: XmlRead>(fragment: &str) -> Result<T, openxml_xml::Error> {
    let xml = format!("<wrapper {FRAGMENT_NS}>{fragment}</wrapper>");
    let mut r = XmlReader::new(&xml);
    r.root()?;
    let child = r.next_child()?.ok_or(openxml_xml::Error::NoRootElement)?;
    T::read_xml(&mut r, &child)
}

/// Parses a fragment built by this crate; such fragments are always valid.
pub(crate) fn fragment<T: XmlRead>(fragment: &str) -> T {
    parse_fragment(fragment).unwrap_or_else(|e| panic!("invalid built-in fragment ({e}): {fragment}"))
}

/// Escapes text for use inside XML attribute values and content.
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    openxml_xml::escape_attr(&mut out, s);
    out
}

type C = pml::CT_GroupShape_Choice;

/// The non-visual drawing properties (`cNvPr`) of a shape-tree member.
pub(crate) fn c_nv_pr(c: &C) -> Option<&dml::CT_NonVisualDrawingProps> {
    match c {
        C::Sp(s) => s.nv_sp_pr.as_ref()?.c_nv_pr.as_deref(),
        C::Pic(p) => p.nv_pic_pr.as_ref()?.c_nv_pr.as_deref(),
        C::GraphicFrame(f) => f.nv_graphic_frame_pr.as_ref()?.c_nv_pr.as_deref(),
        C::GrpSp(g) => g.nv_grp_sp_pr.as_ref()?.c_nv_pr.as_deref(),
        C::CxnSp(x) => x.nv_cxn_sp_pr.as_ref()?.c_nv_pr.as_deref(),
        _ => None,
    }
}

/// The non-visual drawing properties (`cNvPr`) of a shape-tree member, mutably.
pub(crate) fn c_nv_pr_mut(c: &mut C) -> Option<&mut dml::CT_NonVisualDrawingProps> {
    match c {
        C::Sp(s) => s.nv_sp_pr.as_mut()?.c_nv_pr.as_deref_mut(),
        C::Pic(p) => p.nv_pic_pr.as_mut()?.c_nv_pr.as_deref_mut(),
        C::GraphicFrame(f) => f.nv_graphic_frame_pr.as_mut()?.c_nv_pr.as_deref_mut(),
        C::GrpSp(g) => g.nv_grp_sp_pr.as_mut()?.c_nv_pr.as_deref_mut(),
        C::CxnSp(x) => x.nv_cxn_sp_pr.as_mut()?.c_nv_pr.as_deref_mut(),
        _ => None,
    }
}

/// The identifier of a shape-tree member.
pub(crate) fn choice_id(c: &C) -> Option<u32> {
    c_nv_pr(c)?.id
}

/// Finds a member by identifier, searching groups recursively.
pub(crate) fn find(tree: &pml::CT_GroupShape, id: u32) -> Option<&C> {
    for c in &tree.choice {
        if choice_id(c) == Some(id) {
            return Some(c);
        }
        if let C::GrpSp(g) = c
            && let Some(found) = find(g, id)
        {
            return Some(found);
        }
    }
    None
}

/// Finds a member by identifier, searching groups recursively.
pub(crate) fn find_mut(tree: &mut pml::CT_GroupShape, id: u32) -> Option<&mut C> {
    let pos = tree.choice.iter().position(|c| choice_id(c) == Some(id));
    if let Some(i) = pos {
        return tree.choice.get_mut(i);
    }
    for c in &mut tree.choice {
        if let C::GrpSp(g) = c
            && let Some(found) = find_mut(g, id)
        {
            return Some(found);
        }
    }
    None
}

/// A rectangle `(x, y, width, height)`.
pub(crate) type Bounds = (Length, Length, Length, Length);

fn xfrm_bounds(off: Option<&dml::CT_Point2D>, ext: Option<&dml::CT_PositiveSize2D>) -> Option<Bounds> {
    let off = off?;
    let ext = ext?;
    Some((
        coordinate(off.x.as_ref()?)?,
        coordinate(off.y.as_ref()?)?,
        Length::emu(ext.cx?),
        Length::emu(ext.cy?),
    ))
}

/// Position and size of a shape-tree member, when it specifies them.
pub(crate) fn bounds(c: &C) -> Option<Bounds> {
    match c {
        C::Sp(s) => {
            let x = s.sp_pr.as_ref()?.xfrm.as_deref()?;
            xfrm_bounds(x.off.as_deref(), x.ext.as_deref())
        }
        C::Pic(p) => {
            let x = p.sp_pr.as_ref()?.xfrm.as_deref()?;
            xfrm_bounds(x.off.as_deref(), x.ext.as_deref())
        }
        C::CxnSp(s) => {
            let x = s.sp_pr.as_ref()?.xfrm.as_deref()?;
            xfrm_bounds(x.off.as_deref(), x.ext.as_deref())
        }
        C::GraphicFrame(f) => {
            let x = f.xfrm.as_deref()?;
            xfrm_bounds(x.off.as_deref(), x.ext.as_deref())
        }
        C::GrpSp(g) => {
            let x = g.grp_sp_pr.as_ref()?.xfrm.as_deref()?;
            xfrm_bounds(x.off.as_deref(), x.ext.as_deref())
        }
        _ => None,
    }
}

fn point(x: Length, y: Length) -> Box<dml::CT_Point2D> {
    Box::new(dml::CT_Point2D {
        x: Some(coord(x)),
        y: Some(coord(y)),
        ..Default::default()
    })
}

fn size(w: Length, h: Length) -> Box<dml::CT_PositiveSize2D> {
    Box::new(dml::CT_PositiveSize2D {
        cx: Some(w.as_emu().max(0)),
        cy: Some(h.as_emu().max(0)),
        ..Default::default()
    })
}

/// Moves and resizes a shape-tree member (groups keep their child coordinate space).
pub(crate) fn set_bounds(c: &mut C, b: Bounds) {
    let (x, y, w, h) = b;
    match c {
        C::Sp(s) => {
            let xf = s
                .sp_pr
                .get_or_insert_with(Box::default)
                .xfrm
                .get_or_insert_with(Box::default);
            xf.off = Some(point(x, y));
            xf.ext = Some(size(w, h));
        }
        C::Pic(p) => {
            let xf = p
                .sp_pr
                .get_or_insert_with(Box::default)
                .xfrm
                .get_or_insert_with(Box::default);
            xf.off = Some(point(x, y));
            xf.ext = Some(size(w, h));
        }
        C::CxnSp(s) => {
            let xf = s
                .sp_pr
                .get_or_insert_with(Box::default)
                .xfrm
                .get_or_insert_with(Box::default);
            xf.off = Some(point(x, y));
            xf.ext = Some(size(w, h));
        }
        C::GraphicFrame(f) => {
            let xf = f.xfrm.get_or_insert_with(Box::default);
            xf.off = Some(point(x, y));
            xf.ext = Some(size(w, h));
        }
        C::GrpSp(g) => {
            let xf = g
                .grp_sp_pr
                .get_or_insert_with(Box::default)
                .xfrm
                .get_or_insert_with(Box::default);
            xf.off = Some(point(x, y));
            xf.ext = Some(size(w, h));
        }
        _ => {}
    }
}

/// The group transform `(off, ext, chOff, chExt)` of a group, when complete.
pub(crate) fn group_transform(g: &pml::CT_GroupShape) -> Option<(Bounds, Bounds)> {
    let x = g.grp_sp_pr.as_ref()?.xfrm.as_deref()?;
    let outer = xfrm_bounds(x.off.as_deref(), x.ext.as_deref())?;
    let inner = xfrm_bounds(x.ch_off.as_deref(), x.ch_ext.as_deref())?;
    Some((outer, inner))
}

/// Sets the full group transform.
pub(crate) fn set_group_transform(g: &mut pml::CT_GroupShape, outer: Bounds, inner: Bounds) {
    let xf = g
        .grp_sp_pr
        .get_or_insert_with(Box::default)
        .xfrm
        .get_or_insert_with(Box::default);
    xf.off = Some(point(outer.0, outer.1));
    xf.ext = Some(size(outer.2, outer.3));
    xf.ch_off = Some(point(inner.0, inner.1));
    xf.ch_ext = Some(size(inner.2, inner.3));
}

/// The largest `cTn` identifier of a slide's timing tree (including nodes
/// the schema types do not model).
pub(crate) fn max_time_node_id(timing: &pml::CT_SlideTiming) -> u32 {
    RawElement::from_typed(timing, Ns::P, "timing")
        .descendants()
        .into_iter()
        .filter(|e| &*e.name.local == "cTn")
        .filter_map(|e| e.attr(Ns::NONE, "id")?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// A time node as raw XML, for generic inspection.
pub(crate) fn node_raw(c: &pml::CT_TimeNodeList_Choice) -> RawElement {
    let mut w = XmlWriter::new();
    c.write_choice(&mut w);
    RawElement::parse(&w.finish()).expect("the writer produces well-formed XML")
}

/// Shape identifiers targeted (`p:spTgt/@spid`) anywhere inside `raw`.
pub(crate) fn raw_targets(raw: &RawElement) -> Vec<u32> {
    raw.descendants()
        .into_iter()
        .filter(|e| e.name.is(Ns::P, "spTgt"))
        .filter_map(|e| e.attr(Ns::NONE, "spid")?.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_parse_without_namespace_attributes() {
        let sp: pml::CT_Shape = parse_fragment(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="7" name="x"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#,
        )
        .unwrap();
        assert!(sp.extra_attrs.is_empty());
        assert_eq!(sp.nv_sp_pr.unwrap().c_nv_pr.unwrap().id, Some(7));
        assert!(parse_fragment::<pml::CT_Shape>("").is_err());
        assert_eq!(xml_escape("a<b&\"c\""), "a&lt;b&amp;&quot;c&quot;");
    }

    #[test]
    fn finding_and_moving_shapes() {
        let mut tree: pml::CT_GroupShape = parse_fragment(concat!(
            r#"<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#,
            r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="5" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="6" name="inner"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>"#,
            r#"<p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="30" cy="40"/></a:xfrm></p:spPr></p:sp></p:grpSp>"#,
            r#"</p:spTree>"#
        ))
        .unwrap();
        assert!(find(&tree, 6).is_some());
        assert!(find(&tree, 99).is_none());
        let inner = find_mut(&mut tree, 6).unwrap();
        assert_eq!(
            bounds(inner),
            Some((Length::emu(10), Length::emu(20), Length::emu(30), Length::emu(40)))
        );
        set_bounds(
            inner,
            (Length::emu(1), Length::emu(2), Length::emu(3), Length::emu(4)),
        );
        assert_eq!(bounds(find(&tree, 6).unwrap()).unwrap().0, Length::emu(1));
        c_nv_pr_mut(find_mut(&mut tree, 5).unwrap()).unwrap().descr = Some("d".into());
        assert_eq!(
            c_nv_pr(find(&tree, 5).unwrap()).unwrap().descr.as_deref(),
            Some("d")
        );
        let group = find_mut(&mut tree, 5).unwrap();
        let pml::CT_GroupShape_Choice::GrpSp(g) = group else {
            panic!()
        };
        assert!(group_transform(g).is_none());
        let b = (Length::emu(0), Length::emu(0), Length::emu(10), Length::emu(10));
        set_group_transform(g, b, b);
        assert_eq!(group_transform(g), Some((b, b)));
    }
}
