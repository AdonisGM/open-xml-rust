//! Shape formatting: fills, outlines, effects, transforms, geometry and text frames.

use openxml_core::Length;
use openxml_schema::dml;

use crate::drawing::{self, Fill, Line, Shadow, ShapeType};
use crate::paragraph::ParagraphMut;
use crate::shape::ShapeMut;
use crate::text;

/// Vertical placement of text inside a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAnchor {
    /// Top.
    Top,
    /// Middle.
    Middle,
    /// Bottom.
    Bottom,
    /// Justified vertically.
    Justified,
    /// Distributed vertically.
    Distributed,
}

impl TextAnchor {
    pub(crate) fn to_dml(self) -> dml::ST_TextAnchoringType {
        match self {
            TextAnchor::Top => dml::ST_TextAnchoringType::T,
            TextAnchor::Middle => dml::ST_TextAnchoringType::Ctr,
            TextAnchor::Bottom => dml::ST_TextAnchoringType::B,
            TextAnchor::Justified => dml::ST_TextAnchoringType::Just,
            TextAnchor::Distributed => dml::ST_TextAnchoringType::Dist,
        }
    }

    pub(crate) fn from_dml(a: dml::ST_TextAnchoringType) -> TextAnchor {
        match a {
            dml::ST_TextAnchoringType::T => TextAnchor::Top,
            dml::ST_TextAnchoringType::Ctr => TextAnchor::Middle,
            dml::ST_TextAnchoringType::B => TextAnchor::Bottom,
            dml::ST_TextAnchoringType::Just => TextAnchor::Justified,
            dml::ST_TextAnchoringType::Dist => TextAnchor::Distributed,
        }
    }
}

/// What happens to text that does not fit its shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Autofit {
    /// Text overflows the shape.
    None,
    /// Text is shrunk: font scale and line-spacing reduction (fractions, `1.0` and
    /// `0.0` mean no change). PowerPoint recomputes both when the text is edited.
    Shrink {
        /// Font scale (`1.0` = 100 %).
        font_scale: f64,
        /// Line spacing reduction (`0.2` = 20 %).
        line_spacing_reduction: f64,
    },
    /// The shape grows to fit its text.
    ResizeShape,
}

impl Autofit {
    /// Shrinking without a precomputed scale.
    pub const SHRINK: Autofit = Autofit::Shrink {
        font_scale: 1.0,
        line_spacing_reduction: 0.0,
    };

    fn to_dml(self) -> dml::EG_TextAutofit {
        match self {
            Autofit::None => dml::EG_TextAutofit::NoAutofit(Box::default()),
            Autofit::ResizeShape => dml::EG_TextAutofit::SpAutoFit(Box::default()),
            Autofit::Shrink {
                font_scale,
                line_spacing_reduction,
            } => {
                let scale = (font_scale < 1.0).then(|| {
                    dml::ST_TextFontScalePercentOrPercentString::TextFontScalePercent(thousandths_percent(
                        font_scale,
                    ))
                });
                let reduction = (line_spacing_reduction > 0.0).then(|| {
                    dml::ST_TextSpacingPercentOrPercentString::TextSpacingPercent(thousandths_percent(
                        line_spacing_reduction,
                    ))
                });
                dml::EG_TextAutofit::NormAutofit(Box::new(dml::CT_TextNormalAutofit {
                    font_scale: scale,
                    ln_spc_reduction: reduction,
                    ..Default::default()
                }))
            }
        }
    }

    fn from_dml(a: &dml::EG_TextAutofit) -> Option<Autofit> {
        Some(match a {
            dml::EG_TextAutofit::NoAutofit(_) => Autofit::None,
            dml::EG_TextAutofit::SpAutoFit(_) => Autofit::ResizeShape,
            dml::EG_TextAutofit::NormAutofit(n) => Autofit::Shrink {
                font_scale: match &n.font_scale {
                    Some(dml::ST_TextFontScalePercentOrPercentString::TextFontScalePercent(v)) => {
                        f64::from(*v) / 100_000.0
                    }
                    Some(dml::ST_TextFontScalePercentOrPercentString::Percentage(s)) => percent_string(s)?,
                    None => 1.0,
                },
                line_spacing_reduction: match &n.ln_spc_reduction {
                    Some(dml::ST_TextSpacingPercentOrPercentString::TextSpacingPercent(v)) => {
                        f64::from(*v) / 100_000.0
                    }
                    Some(dml::ST_TextSpacingPercentOrPercentString::Percentage(s)) => percent_string(s)?,
                    None => 0.0,
                },
            },
            dml::EG_TextAutofit::Other(_) => return None,
        })
    }
}

/// A fraction as thousandths of a percent (`1.0` → 100 000).
pub(crate) fn thousandths_percent(v: f64) -> i32 {
    (v * 100_000.0)
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Parses `"42%"` into `0.42`.
pub(crate) fn percent_string(s: &str) -> Option<f64> {
    s.trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .map(|v| v / 100.0)
}

/// Direction of text in a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextDirection {
    /// Horizontal (the default).
    Horizontal,
    /// Rotated 90° clockwise.
    Vertical,
    /// Rotated 270° clockwise.
    Vertical270,
    /// Letters stacked vertically.
    Stacked,
    /// East Asian vertical text.
    EastAsianVertical,
    /// Mongolian vertical text.
    MongolianVertical,
    /// Letters stacked vertically, right to left.
    StackedRightToLeft,
}

impl TextDirection {
    fn to_dml(self) -> dml::ST_TextVerticalType {
        use dml::ST_TextVerticalType as V;
        match self {
            TextDirection::Horizontal => V::Horz,
            TextDirection::Vertical => V::Vert,
            TextDirection::Vertical270 => V::Vert270,
            TextDirection::Stacked => V::WordArtVert,
            TextDirection::EastAsianVertical => V::EaVert,
            TextDirection::MongolianVertical => V::MongolianVert,
            TextDirection::StackedRightToLeft => V::WordArtVertRtl,
        }
    }

    fn from_dml(v: dml::ST_TextVerticalType) -> TextDirection {
        use dml::ST_TextVerticalType as V;
        match v {
            V::Horz => TextDirection::Horizontal,
            V::Vert => TextDirection::Vertical,
            V::Vert270 => TextDirection::Vertical270,
            V::WordArtVert => TextDirection::Stacked,
            V::EaVert => TextDirection::EastAsianVertical,
            V::MongolianVert => TextDirection::MongolianVertical,
            V::WordArtVertRtl => TextDirection::StackedRightToLeft,
        }
    }
}

/// A command of a freeform path, in path coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathCommand {
    /// Starts a new sub-path.
    MoveTo(i64, i64),
    /// Straight line to a point.
    LineTo(i64, i64),
    /// Cubic Bézier curve: two control points and the end point.
    CubicTo((i64, i64), (i64, i64), (i64, i64)),
    /// Closes the current sub-path.
    Close,
}

/// A custom (freeform) geometry: a path drawn in a `width` × `height`
/// coordinate space that is stretched over the shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Freeform {
    /// Width of the path coordinate space.
    pub width: i64,
    /// Height of the path coordinate space.
    pub height: i64,
    /// Path commands.
    pub commands: Vec<PathCommand>,
}

impl Freeform {
    /// A closed polygon through `points`.
    pub fn polygon(width: i64, height: i64, points: &[(i64, i64)]) -> Freeform {
        let mut commands = Vec::with_capacity(points.len() + 1);
        for (i, &(x, y)) in points.iter().enumerate() {
            commands.push(if i == 0 {
                PathCommand::MoveTo(x, y)
            } else {
                PathCommand::LineTo(x, y)
            });
        }
        if !points.is_empty() {
            commands.push(PathCommand::Close);
        }
        Freeform {
            width,
            height,
            commands,
        }
    }

    fn to_dml(&self) -> dml::CT_CustomGeometry2D {
        let pt = |(x, y): (i64, i64)| dml::CT_AdjPoint2D {
            x: Some(dml::ST_AdjCoordinate::Coordinate(
                dml::ST_Coordinate::CoordinateUnqualified(x),
            )),
            y: Some(dml::ST_AdjCoordinate::Coordinate(
                dml::ST_Coordinate::CoordinateUnqualified(y),
            )),
            ..Default::default()
        };
        let choice = self
            .commands
            .iter()
            .map(|c| match *c {
                PathCommand::MoveTo(x, y) => dml::CT_Path2D_Choice::MoveTo(Box::new(dml::CT_Path2DMoveTo {
                    pt: Some(Box::new(pt((x, y)))),
                    ..Default::default()
                })),
                PathCommand::LineTo(x, y) => dml::CT_Path2D_Choice::LnTo(Box::new(dml::CT_Path2DLineTo {
                    pt: Some(Box::new(pt((x, y)))),
                    ..Default::default()
                })),
                PathCommand::CubicTo(a, b, c) => {
                    dml::CT_Path2D_Choice::CubicBezTo(Box::new(dml::CT_Path2DCubicBezierTo {
                        pt: vec![pt(a), pt(b), pt(c)],
                        ..Default::default()
                    }))
                }
                PathCommand::Close => dml::CT_Path2D_Choice::Close(Box::default()),
            })
            .collect();
        let guide = |s: &str| Some(dml::ST_AdjCoordinate::GeomGuideName(s.to_owned()));
        dml::CT_CustomGeometry2D {
            av_lst: Some(Box::default()),
            gd_lst: Some(Box::default()),
            ah_lst: Some(Box::default()),
            cxn_lst: Some(Box::default()),
            rect: Some(Box::new(dml::CT_GeomRect {
                l: guide("l"),
                t: guide("t"),
                r: guide("r"),
                b: guide("b"),
                ..Default::default()
            })),
            path_lst: Some(Box::new(dml::CT_Path2DList {
                path: vec![dml::CT_Path2D {
                    w: Some(self.width.max(0)),
                    h: Some(self.height.max(0)),
                    choice,
                    ..Default::default()
                }],
                ..Default::default()
            })),
            ..Default::default()
        }
    }
}

fn coord32(v: Length) -> dml::ST_Coordinate32 {
    dml::ST_Coordinate32::Coordinate32Unqualified(
        v.as_emu().clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    )
}

pub(crate) fn coord32_value(c: &dml::ST_Coordinate32) -> Option<Length> {
    match c {
        dml::ST_Coordinate32::Coordinate32Unqualified(v) => Some(Length::emu(i64::from(*v))),
        dml::ST_Coordinate32::UniversalMeasure(s) => crate::shape::universal_measure(s),
    }
}

/// Default text insets of DrawingML shapes: 0.1" left/right, 0.05" top/bottom.
const DEFAULT_INSETS: (i64, i64, i64, i64) = (91_440, 45_720, 91_440, 45_720);

impl ShapeMut<'_> {
    // ----- fill, line and effects -------------------------------------------------

    /// Sets the fill of the shape.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{Color, Fill, Gradient, LayoutKind, Presentation, Rgb, SchemeColor, ShapeType};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let mut shape = slide.add_shape(ShapeType::RoundRect, Length::cm(1.0), Length::cm(1.0), Length::cm(6.0), Length::cm(3.0));
    /// shape.set_fill(Fill::Gradient(Gradient::linear(Rgb(0x1F, 0x4E, 0x79), SchemeColor::Accent2, 90.0)));
    /// assert!(matches!(shape.fill_format(), Some(Fill::Gradient(_))));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_fill(&mut self, fill: Fill) -> &mut Self {
        self.sp_pr().fill_properties = Some(fill.to_dml());
        self
    }

    /// Fills the shape with a picture already related to the slide (see
    /// [`crate::SlideMut::set_shape_picture_fill`]).
    pub(crate) fn set_blip_fill(&mut self, r_id: String) -> &mut Self {
        self.sp_pr().fill_properties = Some(dml::EG_FillProperties::BlipFill(Box::new(
            crate::picture::stretched_blip(r_id),
        )));
        self
    }

    /// The fill set on the shape itself (not inherited from its style), when
    /// it is one of the kinds [`Fill`] describes.
    pub fn fill_format(&self) -> Option<Fill> {
        Fill::from_dml(self.shape.sp_pr.as_ref()?.fill_properties.as_ref()?)
    }

    /// Sets the outline.
    pub fn set_line(&mut self, line: Line) -> &mut Self {
        self.sp_pr().ln = Some(Box::new(line.to_dml()));
        self
    }

    /// The outline set on the shape itself.
    pub fn line_format(&self) -> Option<Line> {
        Some(Line::from_dml(self.shape.sp_pr.as_ref()?.ln.as_deref()?))
    }

    /// Adds an outer shadow (`Some`) or removes all effects (`None`).
    pub fn set_shadow(&mut self, shadow: Option<Shadow>) -> &mut Self {
        self.sp_pr().effect_properties = shadow.map(Shadow::effect_list);
        self
    }

    /// The outer shadow set on the shape itself.
    pub fn shadow(&self) -> Option<Shadow> {
        match self.shape.sp_pr.as_ref()?.effect_properties.as_ref()? {
            dml::EG_EffectProperties::EffectLst(l) => l.outer_shdw.as_deref().map(Shadow::from_dml),
            _ => None,
        }
    }

    // ----- transform --------------------------------------------------------------

    fn xfrm(&mut self) -> &mut dml::CT_Transform2D {
        self.sp_pr().xfrm.get_or_insert_with(Box::default)
    }

    /// Rotates the shape clockwise, in degrees.
    pub fn set_rotation(&mut self, degrees: f64) -> &mut Self {
        let a = drawing::angle(degrees);
        self.xfrm().rot = (a != 0).then_some(a);
        self
    }

    /// Clockwise rotation in degrees.
    pub fn rotation(&self) -> f64 {
        self.shape
            .sp_pr
            .as_ref()
            .and_then(|p| p.xfrm.as_ref())
            .and_then(|x| x.rot)
            .map_or(0.0, drawing::degrees)
    }

    /// Mirrors the shape horizontally and/or vertically.
    pub fn set_flip(&mut self, horizontal: bool, vertical: bool) -> &mut Self {
        let x = self.xfrm();
        x.flip_h = horizontal.then_some(true);
        x.flip_v = vertical.then_some(true);
        self
    }

    /// Whether the shape is mirrored `(horizontally, vertically)`.
    pub fn flip(&self) -> (bool, bool) {
        let x = self.shape.sp_pr.as_ref().and_then(|p| p.xfrm.as_ref());
        (
            x.and_then(|x| x.flip_h).unwrap_or(false),
            x.and_then(|x| x.flip_v).unwrap_or(false),
        )
    }

    // ----- geometry -----------------------------------------------------------------

    /// Changes the preset geometry (adjust values are reset).
    pub fn set_geometry(&mut self, shape: ShapeType) -> &mut Self {
        self.sp_pr().geometry = Some(dml::EG_Geometry::PrstGeom(Box::new(dml::CT_PresetGeometry2D {
            prst: Some(shape),
            av_lst: Some(Box::default()),
            ..Default::default()
        })));
        self
    }

    /// The preset geometry, when the shape uses one.
    pub fn geometry(&self) -> Option<ShapeType> {
        match self.shape.sp_pr.as_ref()?.geometry.as_ref()? {
            dml::EG_Geometry::PrstGeom(g) => g.prst,
            _ => None,
        }
    }

    /// Sets an adjust value of the preset geometry, e.g. `("adj", 16667)` for
    /// the corner radius of a rounded rectangle (values are in the preset's
    /// own units, usually 1/100 000 of the shape size).
    pub fn set_adjust(&mut self, name: &str, value: i64) -> &mut Self {
        if let Some(dml::EG_Geometry::PrstGeom(g)) = self.sp_pr().geometry.as_mut() {
            let list = g.av_lst.get_or_insert_with(Box::default);
            let fmla = format!("val {value}");
            match list.gd.iter_mut().find(|gd| gd.name.as_deref() == Some(name)) {
                Some(gd) => gd.fmla = Some(fmla),
                None => list.gd.push(dml::CT_GeomGuide {
                    name: Some(name.to_owned()),
                    fmla: Some(fmla),
                    ..Default::default()
                }),
            }
        }
        self
    }

    /// The adjust values of the preset geometry as `(name, value)` pairs.
    pub fn adjusts(&self) -> Vec<(String, i64)> {
        let Some(dml::EG_Geometry::PrstGeom(g)) = self.shape.sp_pr.as_ref().and_then(|p| p.geometry.as_ref())
        else {
            return Vec::new();
        };
        g.av_lst
            .iter()
            .flat_map(|l| &l.gd)
            .filter_map(|gd| {
                let v = gd.fmla.as_deref()?.strip_prefix("val ")?.trim().parse().ok()?;
                Some((gd.name.clone()?, v))
            })
            .collect()
    }

    /// Replaces the geometry with a freeform path.
    pub fn set_freeform(&mut self, path: &Freeform) -> &mut Self {
        self.sp_pr().geometry = Some(dml::EG_Geometry::CustGeom(Box::new(path.to_dml())));
        self
    }

    // ----- text frame ---------------------------------------------------------------

    fn body_pr(&mut self) -> &mut dml::CT_TextBodyProperties {
        self.body().body_pr.get_or_insert_with(Box::default)
    }

    fn body_pr_ref(&self) -> Option<&dml::CT_TextBodyProperties> {
        self.shape.tx_body.as_ref()?.body_pr.as_deref()
    }

    /// Sets the vertical placement of the text.
    pub fn set_text_anchor(&mut self, anchor: TextAnchor) -> &mut Self {
        self.body_pr().anchor = Some(anchor.to_dml());
        self
    }

    /// Vertical placement of the text, when set on the shape.
    pub fn text_anchor(&self) -> Option<TextAnchor> {
        self.body_pr_ref()?.anchor.map(TextAnchor::from_dml)
    }

    /// Sets the distances between the shape border and its text.
    pub fn set_text_insets(&mut self, left: Length, top: Length, right: Length, bottom: Length) -> &mut Self {
        let pr = self.body_pr();
        pr.l_ins = Some(coord32(left));
        pr.t_ins = Some(coord32(top));
        pr.r_ins = Some(coord32(right));
        pr.b_ins = Some(coord32(bottom));
        self
    }

    /// Text insets `(left, top, right, bottom)`, defaults applied.
    pub fn text_insets(&self) -> (Length, Length, Length, Length) {
        let pr = self.body_pr_ref();
        let get = |v: Option<&dml::ST_Coordinate32>, default: i64| {
            v.and_then(coord32_value).unwrap_or(Length::emu(default))
        };
        let (l, t, r, b) = DEFAULT_INSETS;
        (
            get(pr.and_then(|p| p.l_ins.as_ref()), l),
            get(pr.and_then(|p| p.t_ins.as_ref()), t),
            get(pr.and_then(|p| p.r_ins.as_ref()), r),
            get(pr.and_then(|p| p.b_ins.as_ref()), b),
        )
    }

    /// Sets how overflowing text is handled.
    pub fn set_autofit(&mut self, autofit: Autofit) -> &mut Self {
        self.body_pr().text_autofit = Some(autofit.to_dml());
        self
    }

    /// How overflowing text is handled, when set on the shape.
    pub fn autofit(&self) -> Option<Autofit> {
        Autofit::from_dml(self.body_pr_ref()?.text_autofit.as_ref()?)
    }

    /// Sets the direction of the text.
    pub fn set_text_direction(&mut self, direction: TextDirection) -> &mut Self {
        self.body_pr().vert = Some(direction.to_dml());
        self
    }

    /// Direction of the text.
    pub fn text_direction(&self) -> TextDirection {
        self.body_pr_ref()
            .and_then(|p| p.vert)
            .map_or(TextDirection::Horizontal, TextDirection::from_dml)
    }

    /// Lays the text out in `count` columns (1–16) separated by `spacing`.
    pub fn set_columns(&mut self, count: u32, spacing: Length) -> &mut Self {
        let pr = self.body_pr();
        pr.num_col = Some(count.clamp(1, 16) as i32);
        pr.spc_col = Some(spacing.as_emu().clamp(0, i64::from(i32::MAX)) as i32);
        self
    }

    /// Number of text columns and the spacing between them.
    pub fn columns(&self) -> (u32, Length) {
        let pr = self.body_pr_ref();
        (
            pr.and_then(|p| p.num_col).map_or(1, |n| n.max(1) as u32),
            Length::emu(pr.and_then(|p| p.spc_col).map_or(0, i64::from)),
        )
    }

    /// Wraps text at the shape border (`true`) or lets lines run on.
    pub fn set_word_wrap(&mut self, wrap: bool) -> &mut Self {
        self.body_pr().wrap = Some(if wrap {
            dml::ST_TextWrappingType::Square
        } else {
            dml::ST_TextWrappingType::None
        });
        self
    }

    /// Applies an outer shadow to all text (`None` removes text effects).
    pub fn set_text_shadow(&mut self, shadow: Option<Shadow>) -> &mut Self {
        text::for_each_run_props(self.body(), |p| {
            p.effect_properties = shadow.map(Shadow::effect_list)
        });
        self
    }

    // ----- paragraphs ---------------------------------------------------------------

    /// Number of paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.shape.tx_body.as_ref().map_or(0, |b| b.p.len())
    }

    /// The paragraph at `index`.
    pub fn paragraph_mut(&mut self, index: usize) -> Option<ParagraphMut<'_>> {
        self.body().p.get_mut(index).map(ParagraphMut::new)
    }

    /// Appends a paragraph. When the shape holds a single empty paragraph, that
    /// paragraph is reused so the new text is not preceded by a blank line.
    ///
    /// ```
    /// use openxml_core::{FontSize, Length};
    /// use openxml_pptx::{Bullet, LayoutKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let mut tb = slide.add_text_box(Length::cm(1.0), Length::cm(1.0), Length::cm(10.0), Length::cm(4.0), "");
    /// tb.add_paragraph("First").set_bullet(Bullet::Char { char: '•', font: None });
    /// let mut p = tb.add_paragraph("");
    /// p.add_run("bold ").bold(true);
    /// p.add_run("and big").size(FontSize(28.0));
    /// assert_eq!(tb.text(), "First\nbold and big");
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_paragraph(&mut self, text: &str) -> ParagraphMut<'_> {
        let body = self.body();
        let reuse = body.p.len() == 1 && text::paragraph_text(&body.p[0]).is_empty();
        let p = text::paragraph(text, 0);
        if reuse {
            let old = &mut body.p[0];
            old.text_run = p.text_run;
        } else {
            body.p.push(p);
        }
        ParagraphMut::new(body.p.last_mut().expect("a paragraph exists"))
    }

    /// Removes all text, leaving one empty paragraph.
    pub fn clear_text(&mut self) -> &mut Self {
        text::set_paragraphs(self.body(), Vec::new());
        self
    }

    // ----- accessibility --------------------------------------------------------------

    /// Sets the alternative text: a short title and a description.
    pub fn set_alt_text(&mut self, title: Option<&str>, description: &str) -> &mut Self {
        if let Some(nv) = self.shape.nv_sp_pr.as_mut().and_then(|n| n.c_nv_pr.as_mut()) {
            nv.title = title.map(str::to_owned);
            nv.descr = Some(description.to_owned());
        }
        self
    }

    /// The alternative text description.
    pub fn alt_text(&self) -> Option<&str> {
        self.shape.nv_sp_pr.as_ref()?.c_nv_pr.as_ref()?.descr.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::{Color, SchemeColor};
    use crate::text::Rgb;
    use openxml_schema::pml;

    fn shape() -> pml::CT_Shape {
        crate::shape::new_auto_shape(
            3,
            ShapeType::Rect,
            Length::ZERO,
            Length::ZERO,
            Length::cm(2.0),
            Length::cm(1.0),
        )
    }

    #[test]
    fn fills_lines_and_effects() {
        let mut sp = shape();
        let mut m = ShapeMut::new(&mut sp);
        assert_eq!(m.fill_format(), None);
        m.set_fill(Fill::solid(SchemeColor::Accent3));
        assert_eq!(
            m.fill_format(),
            Some(Fill::Solid(Color::Scheme(SchemeColor::Accent3)))
        );
        m.set_line(Line::solid(Rgb(1, 2, 3), Length::pt(2.0)));
        assert_eq!(m.line_format().unwrap().width, Length::pt(2.0));
        assert_eq!(m.shadow(), None);
        m.set_shadow(Some(Shadow::default()));
        assert_eq!(m.shadow().unwrap().direction, 45.0);
        m.set_shadow(None);
        assert_eq!(m.shadow(), None);
    }

    #[test]
    fn transforms_and_geometry() {
        let mut sp = shape();
        let mut m = ShapeMut::new(&mut sp);
        assert_eq!(m.rotation(), 0.0);
        m.set_rotation(-90.0).set_flip(true, false);
        assert_eq!(m.rotation(), 270.0);
        assert_eq!(m.flip(), (true, false));
        m.set_rotation(0.0);
        assert!(
            m.raw()
                .sp_pr
                .as_ref()
                .unwrap()
                .xfrm
                .as_ref()
                .unwrap()
                .rot
                .is_none()
        );
        assert_eq!(m.geometry(), Some(ShapeType::Rect));
        m.set_geometry(ShapeType::RoundRect)
            .set_adjust("adj", 30000)
            .set_adjust("adj", 25000);
        assert_eq!(m.adjusts(), vec![("adj".to_owned(), 25000)]);
        m.set_freeform(&Freeform::polygon(100, 100, &[(0, 100), (50, 0), (100, 100)]));
        assert_eq!(m.geometry(), None);
        assert!(m.adjusts().is_empty());
        let empty = Freeform::polygon(1, 1, &[]);
        assert!(empty.commands.is_empty());
    }

    #[test]
    fn text_frames() {
        let mut sp = shape();
        let mut m = ShapeMut::new(&mut sp);
        assert_eq!(
            m.text_anchor(),
            Some(TextAnchor::Middle),
            "autoshapes centre text"
        );
        m.set_text_anchor(TextAnchor::Bottom);
        assert_eq!(m.text_anchor(), Some(TextAnchor::Bottom));
        assert_eq!(m.text_insets().0, Length::inches(0.1));
        m.set_text_insets(Length::ZERO, Length::pt(1.0), Length::ZERO, Length::ZERO);
        assert_eq!(m.text_insets().1, Length::pt(1.0));
        for a in [Autofit::None, Autofit::ResizeShape, Autofit::SHRINK] {
            m.set_autofit(a);
            assert_eq!(m.autofit(), Some(a));
        }
        let shrink = Autofit::Shrink {
            font_scale: 0.925,
            line_spacing_reduction: 0.1,
        };
        m.set_autofit(shrink);
        assert_eq!(m.autofit(), Some(shrink));
        for d in [
            TextDirection::Horizontal,
            TextDirection::Vertical,
            TextDirection::Vertical270,
            TextDirection::Stacked,
            TextDirection::EastAsianVertical,
            TextDirection::MongolianVertical,
            TextDirection::StackedRightToLeft,
        ] {
            m.set_text_direction(d);
            assert_eq!(m.text_direction(), d);
        }
        assert_eq!(m.columns(), (1, Length::ZERO));
        m.set_columns(40, Length::cm(1.0)).set_word_wrap(false);
        assert_eq!(m.columns(), (16, Length::cm(1.0)));
        m.set_text("x").set_text_shadow(Some(Shadow::default()));
        assert_eq!(percent_string("50%"), Some(0.5));
        assert_eq!(thousandths_percent(0.5), 50_000);
    }

    #[test]
    fn paragraphs_and_alt_text() {
        let mut sp = shape();
        let mut m = ShapeMut::new(&mut sp);
        assert_eq!(m.paragraph_count(), 1);
        m.add_paragraph("one");
        m.add_paragraph("two");
        assert_eq!(m.text(), "one\ntwo");
        assert_eq!(m.paragraph_mut(1).unwrap().text(), "two");
        assert!(m.paragraph_mut(2).is_none());
        m.clear_text();
        assert_eq!(m.paragraph_count(), 1);
        assert_eq!(m.text(), "");
        m.set_alt_text(Some("Logo"), "Company logo");
        assert_eq!(m.alt_text(), Some("Company logo"));
    }
}
