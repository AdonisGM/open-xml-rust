//! Connectors (`p:cxnSp`) between shapes.

use openxml_core::{Error, Length, Result};
use openxml_schema::{dml, pml};

use crate::drawing::{Line, ShapeType};
use crate::shape;
use crate::slide::SlideMut;
use crate::util;

/// The routing of a connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectorKind {
    /// A straight line (`straightConnector1`).
    Straight,
    /// A line with right-angle bends (`bentConnector3`).
    Elbow,
    /// A curved line (`curvedConnector3`).
    Curved,
}

impl ConnectorKind {
    fn geometry(self) -> ShapeType {
        match self {
            ConnectorKind::Straight => ShapeType::StraightConnector1,
            ConnectorKind::Elbow => ShapeType::BentConnector3,
            ConnectorKind::Curved => ShapeType::CurvedConnector3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ConnectorKind::Straight => "Straight Connector",
            ConnectorKind::Elbow => "Connector: Elbow",
            ConnectorKind::Curved => "Connector: Curved",
        }
    }
}

/// A side of a shape a connector attaches to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// Middle of the top edge.
    Top,
    /// Middle of the left edge.
    Left,
    /// Middle of the bottom edge.
    Bottom,
    /// Middle of the right edge.
    Right,
}

impl Side {
    /// Index of the connection site of this side on a shape with `geometry`
    /// (rectangle-like presets number their sites top, left, bottom, right;
    /// ellipses have eight sites starting at the top, counter-clockwise).
    pub fn site_index(self, geometry: Option<ShapeType>) -> u32 {
        let base = match self {
            Side::Top => 0,
            Side::Left => 1,
            Side::Bottom => 2,
            Side::Right => 3,
        };
        if geometry == Some(ShapeType::Ellipse) {
            base * 2
        } else {
            base
        }
    }

    fn point(self, (x, y, w, h): util::Bounds) -> (Length, Length) {
        let half = |v: Length| Length::emu(v.as_emu() / 2);
        match self {
            Side::Top => (x + half(w), y),
            Side::Left => (x, y + half(h)),
            Side::Bottom => (x + half(w), y + h),
            Side::Right => (x + w, y + half(h)),
        }
    }
}

/// Transform of a line from `start` to `end`.
fn line_xfrm(start: (Length, Length), end: (Length, Length)) -> dml::CT_Transform2D {
    let (x0, y0) = (start.0.as_emu(), start.1.as_emu());
    let (x1, y1) = (end.0.as_emu(), end.1.as_emu());
    let mut x = shape::transform(
        Length::emu(x0.min(x1)),
        Length::emu(y0.min(y1)),
        Length::emu((x1 - x0).abs()),
        Length::emu((y1 - y0).abs()),
    );
    x.flip_h = (x1 < x0).then_some(true);
    x.flip_v = (y1 < y0).then_some(true);
    x
}

pub(crate) fn new_connector(
    id: u32,
    kind: ConnectorKind,
    start: (Length, Length),
    end: (Length, Length),
) -> pml::CT_Connector {
    let name = format!("{} {}", kind.name(), id.saturating_sub(1));
    let mut cxn: pml::CT_Connector = util::fragment(&format!(
        concat!(
            r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr/>"#,
            r#"<p:style><a:lnRef idx="1"><a:schemeClr val="accent1"/></a:lnRef>"#,
            r#"<a:fillRef idx="0"><a:schemeClr val="accent1"/></a:fillRef>"#,
            r#"<a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef>"#,
            r#"<a:fontRef idx="minor"><a:schemeClr val="tx1"/></a:fontRef></p:style></p:cxnSp>"#
        ),
        id = id,
        name = util::xml_escape(&name),
    ));
    cxn.sp_pr = Some(Box::new(dml::CT_ShapeProperties {
        xfrm: Some(Box::new(line_xfrm(start, end))),
        geometry: Some(dml::EG_Geometry::PrstGeom(Box::new(dml::CT_PresetGeometry2D {
            prst: Some(kind.geometry()),
            av_lst: Some(Box::default()),
            ..Default::default()
        }))),
        ..Default::default()
    }));
    cxn
}

/// Mutable access to a connector.
pub struct ConnectorMut<'a> {
    cxn: &'a mut pml::CT_Connector,
}

impl<'a> ConnectorMut<'a> {
    pub(crate) fn new(cxn: &'a mut pml::CT_Connector) -> Self {
        ConnectorMut { cxn }
    }

    fn props(&mut self) -> &mut dml::CT_NonVisualConnectorProperties {
        self.cxn
            .nv_cxn_sp_pr
            .get_or_insert_with(Box::default)
            .c_nv_cxn_sp_pr
            .get_or_insert_with(Box::default)
    }

    fn props_ref(&self) -> Option<&dml::CT_NonVisualConnectorProperties> {
        self.cxn.nv_cxn_sp_pr.as_ref()?.c_nv_cxn_sp_pr.as_deref()
    }

    /// Shape identifier.
    pub fn id(&self) -> u32 {
        self.cxn
            .nv_cxn_sp_pr
            .as_ref()
            .and_then(|n| n.c_nv_pr.as_ref())
            .and_then(|c| c.id)
            .unwrap_or(0)
    }

    /// Sets the line (colour, width, dash, arrowheads).
    pub fn set_line(&mut self, line: Line) -> &mut Self {
        self.cxn.sp_pr.get_or_insert_with(Box::default).ln = Some(Box::new(line.to_dml()));
        self
    }

    /// The line set on the connector itself.
    pub fn line_format(&self) -> Option<Line> {
        Some(Line::from_dml(self.cxn.sp_pr.as_ref()?.ln.as_deref()?))
    }

    /// Glues the start of the connector to connection site `site` of shape `shape_id`.
    pub fn connect_start(&mut self, shape_id: u32, site: u32) -> &mut Self {
        self.props().st_cxn = Some(Box::new(dml::CT_Connection {
            id: Some(shape_id),
            idx: Some(site),
            ..Default::default()
        }));
        self
    }

    /// Glues the end of the connector to connection site `site` of shape `shape_id`.
    pub fn connect_end(&mut self, shape_id: u32, site: u32) -> &mut Self {
        self.props().end_cxn = Some(Box::new(dml::CT_Connection {
            id: Some(shape_id),
            idx: Some(site),
            ..Default::default()
        }));
        self
    }

    /// The shape and site the start is glued to.
    pub fn start_connection(&self) -> Option<(u32, u32)> {
        let c = self.props_ref()?.st_cxn.as_deref()?;
        Some((c.id?, c.idx?))
    }

    /// The shape and site the end is glued to.
    pub fn end_connection(&self) -> Option<(u32, u32)> {
        let c = self.props_ref()?.end_cxn.as_deref()?;
        Some((c.id?, c.idx?))
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &pml::CT_Connector {
        self.cxn
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut pml::CT_Connector {
        self.cxn
    }
}

fn geometry_of(c: &pml::CT_GroupShape_Choice) -> Option<ShapeType> {
    let sp_pr = match c {
        pml::CT_GroupShape_Choice::Sp(s) => s.sp_pr.as_deref(),
        pml::CT_GroupShape_Choice::Pic(p) => p.sp_pr.as_deref(),
        _ => None,
    }?;
    match sp_pr.geometry.as_ref()? {
        dml::EG_Geometry::PrstGeom(g) => g.prst,
        _ => None,
    }
}

impl SlideMut<'_> {
    /// Adds a free connector from `start` to `end` (slide coordinates).
    pub fn add_connector(
        &mut self,
        kind: ConnectorKind,
        start: (Length, Length),
        end: (Length, Length),
    ) -> ConnectorMut<'_> {
        let id = self.next_id();
        let tree = self.tree_mut();
        tree.choice
            .push(pml::CT_GroupShape_Choice::CxnSp(Box::new(new_connector(
                id, kind, start, end,
            ))));
        match tree.choice.last_mut() {
            Some(pml::CT_GroupShape_Choice::CxnSp(c)) => ConnectorMut::new(c),
            _ => unreachable!("just pushed a connector"),
        }
    }

    /// Connects two shapes: the connector runs from a side of `from` to a side
    /// of `to` and is glued to both (`stCxn` / `endCxn`), so PowerPoint keeps
    /// it attached when the shapes move.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{ArrowHead, ArrowKind, ConnectorKind, LayoutKind, Line, Presentation, Rgb, ShapeType, Side};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let a = slide.add_shape(ShapeType::Rect, Length::cm(1.0), Length::cm(1.0), Length::cm(4.0), Length::cm(2.0)).id();
    /// let b = slide.add_shape(ShapeType::Ellipse, Length::cm(10.0), Length::cm(5.0), Length::cm(4.0), Length::cm(2.0)).id();
    /// let mut c = slide.connect_shapes(ConnectorKind::Elbow, a, Side::Right, b, Side::Left)?;
    /// c.set_line(Line::solid(Rgb(0, 0, 0), Length::pt(1.5)).tail(ArrowHead::new(ArrowKind::Triangle)));
    /// assert_eq!(c.end_connection(), Some((b, 2)));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn connect_shapes(
        &mut self,
        kind: ConnectorKind,
        from: u32,
        from_side: Side,
        to: u32,
        to_side: Side,
    ) -> Result<ConnectorMut<'_>> {
        let tree = self.tree_mut();
        let endpoint = |id: u32, side: Side| -> Result<((Length, Length), u32)> {
            let c = util::find(tree, id).ok_or_else(|| Error::NotFound(format!("shape {id}")))?;
            let b = util::bounds(c)
                .ok_or_else(|| Error::InvalidArgument(format!("shape {id} has no position")))?;
            Ok((side.point(b), side.site_index(geometry_of(c))))
        };
        let (start, start_site) = endpoint(from, from_side)?;
        let (end, end_site) = endpoint(to, to_side)?;
        let mut c = self.add_connector(kind, start, end);
        c.connect_start(from, start_site).connect_end(to, end_site);
        Ok(c)
    }

    /// The connector with the given shape identifier (searching groups).
    pub fn connector_mut(&mut self, shape_id: u32) -> Option<ConnectorMut<'_>> {
        match util::find_mut(self.tree_mut(), shape_id)? {
            pml::CT_GroupShape_Choice::CxnSp(c) => Some(ConnectorMut::new(c)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_geometry() {
        let c = new_connector(
            5,
            ConnectorKind::Curved,
            (Length::emu(100), Length::emu(50)),
            (Length::emu(10), Length::emu(80)),
        );
        let x = c.sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap();
        assert_eq!(x.flip_h, Some(true));
        assert_eq!(x.flip_v, None);
        assert_eq!(x.ext.as_ref().unwrap().cx, Some(90));
        assert_eq!(x.ext.as_ref().unwrap().cy, Some(30));
        assert_eq!(
            c.nv_cxn_sp_pr
                .as_ref()
                .unwrap()
                .c_nv_pr
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("Connector: Curved 4")
        );
        assert!(c.style.is_some());
    }

    #[test]
    fn sites() {
        assert_eq!(Side::Right.site_index(Some(ShapeType::Rect)), 3);
        assert_eq!(Side::Right.site_index(Some(ShapeType::Ellipse)), 6);
        assert_eq!(Side::Top.site_index(None), 0);
        let b = (Length::emu(0), Length::emu(0), Length::emu(10), Length::emu(20));
        assert_eq!(Side::Bottom.point(b), (Length::emu(5), Length::emu(20)));
        assert_eq!(Side::Right.point(b), (Length::emu(10), Length::emu(10)));
        assert_eq!(Side::Left.point(b), (Length::emu(0), Length::emu(10)));
        assert_eq!(Side::Top.point(b), (Length::emu(5), Length::emu(0)));
    }

    #[test]
    fn gluing() {
        let mut c = new_connector(
            2,
            ConnectorKind::Straight,
            (Length::ZERO, Length::ZERO),
            (Length::emu(5), Length::emu(5)),
        );
        let mut m = ConnectorMut::new(&mut c);
        assert_eq!(m.id(), 2);
        assert_eq!(m.start_connection(), None);
        m.connect_start(3, 1).connect_end(4, 2);
        assert_eq!(m.start_connection(), Some((3, 1)));
        assert_eq!(m.end_connection(), Some((4, 2)));
        assert_eq!(m.line_format(), None);
        m.set_line(Line::none());
        assert_eq!(m.line_format().unwrap().color, None);
        assert!(m.raw().sp_pr.is_some());
        assert!(m.raw_mut().style.is_some());
    }
}
