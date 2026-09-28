//! Graphic frames (`p:graphicFrame`) hosting arbitrary `a:graphicData`:
//! the entry point for charts, diagrams and other embedded objects.

use openxml_core::Length;
use openxml_schema::{dml, pml};
use openxml_xml::RawElement;

use crate::shape::{self, CHART_URI};
use crate::slide::SlideMut;

/// `a:graphicData` referring to a chart part through relationship `r_id`
/// (`<c:chart r:id="…"/>`). The chart part and the relationship are the
/// caller's responsibility.
pub fn chart_graphic_data(r_id: &str) -> dml::CT_GraphicalObjectData {
    let chart = RawElement::parse(&format!(
        concat!(
            r#"<c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" "#,
            r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="{}"/>"#
        ),
        crate::util::xml_escape(r_id)
    ))
    .expect("well-formed");
    dml::CT_GraphicalObjectData {
        uri: Some(CHART_URI.to_owned()),
        any: vec![chart],
        ..Default::default()
    }
}

/// A graphic frame with the given content.
pub(crate) fn new_graphic_frame(
    id: u32,
    name: &str,
    data: dml::CT_GraphicalObjectData,
    x: Length,
    y: Length,
    w: Length,
    h: Length,
) -> pml::CT_GraphicalObjectFrame {
    pml::CT_GraphicalObjectFrame {
        nv_graphic_frame_pr: Some(Box::new(pml::CT_GraphicalObjectFrameNonVisual {
            c_nv_pr: Some(Box::new(shape::nv_props(id, name))),
            c_nv_graphic_frame_pr: Some(Box::new(dml::CT_NonVisualGraphicFrameProperties {
                graphic_frame_locks: Some(Box::new(dml::CT_GraphicalObjectFrameLocking {
                    no_grp: Some(true),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            nv_pr: Some(Box::default()),
            ..Default::default()
        })),
        xfrm: Some(Box::new(shape::transform(x, y, w, h))),
        graphic: Some(Box::new(dml::CT_GraphicalObject {
            graphic_data: Some(Box::new(data)),
            ..Default::default()
        })),
        ..Default::default()
    }
}

impl SlideMut<'_> {
    /// Adds a graphic frame hosting `data` (an `a:graphicData` element such
    /// as [`chart_graphic_data`]) and returns its shape identifier. Parts and
    /// relationships the content refers to must be added by the caller.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{chart_graphic_data, LayoutKind, Presentation, ShapeKind};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let id = slide.add_graphic_frame("Chart 1", chart_graphic_data("rId2"), Length::cm(2.0), Length::cm(2.0), Length::cm(16.0), Length::cm(9.0));
    /// let info = slide.shapes().into_iter().find(|s| s.id == id).unwrap();
    /// assert_eq!(info.kind, ShapeKind::Chart);
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_graphic_frame(
        &mut self,
        name: &str,
        data: dml::CT_GraphicalObjectData,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> u32 {
        let id = self.next_id();
        self.tree_mut()
            .choice
            .push(pml::CT_GroupShape_Choice::GraphicFrame(Box::new(
                new_graphic_frame(id, name, data, x, y, w, h),
            )));
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxml_xml::Ns;

    #[test]
    fn chart_data() {
        let d = chart_graphic_data("rId5");
        assert_eq!(d.uri.as_deref(), Some(CHART_URI));
        assert_eq!(d.any[0].attr(Ns::R, "id"), Some("rId5"));
        let f = new_graphic_frame(
            3,
            "Chart 2",
            d,
            Length::ZERO,
            Length::ZERO,
            Length::cm(1.0),
            Length::cm(1.0),
        );
        assert_eq!(
            shape::describe(&pml::CT_GroupShape_Choice::GraphicFrame(Box::new(f))).kind,
            shape::ShapeKind::Chart
        );
    }
}
