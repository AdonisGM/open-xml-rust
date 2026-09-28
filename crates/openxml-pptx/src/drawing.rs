//! DrawingML building blocks shared by shapes, pictures and tables:
//! colours, fills, outlines and shadows.

use openxml_core::Length;
use openxml_schema::{dml, shared_types};
use openxml_xml::HexBinary;

use crate::text::Rgb;

pub use openxml_schema::dml::ST_PresetPatternVal as PatternType;
pub use openxml_schema::dml::ST_ShapeType as ShapeType;

/// A colour slot of the theme (`a:schemeClr`), so that the colour follows the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchemeColor {
    /// Dark 1 (`dk1`).
    Dark1,
    /// Light 1 (`lt1`).
    Light1,
    /// Dark 2 (`dk2`).
    Dark2,
    /// Light 2 (`lt2`).
    Light2,
    /// Accent 1.
    Accent1,
    /// Accent 2.
    Accent2,
    /// Accent 3.
    Accent3,
    /// Accent 4.
    Accent4,
    /// Accent 5.
    Accent5,
    /// Accent 6.
    Accent6,
    /// Hyperlink.
    Hyperlink,
    /// Followed hyperlink.
    FollowedHyperlink,
    /// Text 1, mapped through the colour map (usually dark 1).
    Text1,
    /// Background 1 (usually light 1).
    Background1,
    /// Text 2 (usually dark 2).
    Text2,
    /// Background 2 (usually light 2).
    Background2,
}

impl SchemeColor {
    pub(crate) fn to_dml(self) -> dml::ST_SchemeColorVal {
        use dml::ST_SchemeColorVal as V;
        match self {
            SchemeColor::Dark1 => V::Dk1,
            SchemeColor::Light1 => V::Lt1,
            SchemeColor::Dark2 => V::Dk2,
            SchemeColor::Light2 => V::Lt2,
            SchemeColor::Accent1 => V::Accent1,
            SchemeColor::Accent2 => V::Accent2,
            SchemeColor::Accent3 => V::Accent3,
            SchemeColor::Accent4 => V::Accent4,
            SchemeColor::Accent5 => V::Accent5,
            SchemeColor::Accent6 => V::Accent6,
            SchemeColor::Hyperlink => V::Hlink,
            SchemeColor::FollowedHyperlink => V::FolHlink,
            SchemeColor::Text1 => V::Tx1,
            SchemeColor::Background1 => V::Bg1,
            SchemeColor::Text2 => V::Tx2,
            SchemeColor::Background2 => V::Bg2,
        }
    }

    pub(crate) fn from_dml(v: dml::ST_SchemeColorVal) -> Option<SchemeColor> {
        use dml::ST_SchemeColorVal as V;
        Some(match v {
            V::Dk1 => SchemeColor::Dark1,
            V::Lt1 => SchemeColor::Light1,
            V::Dk2 => SchemeColor::Dark2,
            V::Lt2 => SchemeColor::Light2,
            V::Accent1 => SchemeColor::Accent1,
            V::Accent2 => SchemeColor::Accent2,
            V::Accent3 => SchemeColor::Accent3,
            V::Accent4 => SchemeColor::Accent4,
            V::Accent5 => SchemeColor::Accent5,
            V::Accent6 => SchemeColor::Accent6,
            V::Hlink => SchemeColor::Hyperlink,
            V::FolHlink => SchemeColor::FollowedHyperlink,
            V::Tx1 => SchemeColor::Text1,
            V::Bg1 => SchemeColor::Background1,
            V::Tx2 => SchemeColor::Text2,
            V::Bg2 => SchemeColor::Background2,
            V::PhClr => return None,
        })
    }
}

/// A colour: either fixed RGB or a theme slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// A fixed colour.
    Rgb(Rgb),
    /// A theme colour.
    Scheme(SchemeColor),
}

impl From<Rgb> for Color {
    fn from(c: Rgb) -> Self {
        Color::Rgb(c)
    }
}

impl From<SchemeColor> for Color {
    fn from(c: SchemeColor) -> Self {
        Color::Scheme(c)
    }
}

impl Color {
    /// The colour with an alpha (opacity) modifier, `alpha` in `0.0..=1.0`.
    pub(crate) fn to_dml_alpha(self, alpha: Option<f64>) -> dml::EG_ColorChoice {
        let transform: Vec<dml::EG_ColorTransform> = alpha
            .map(|a| {
                vec![dml::EG_ColorTransform::Alpha(Box::new(
                    dml::CT_PositiveFixedPercentage {
                        val: Some(fixed_percentage(a)),
                        ..Default::default()
                    },
                ))]
            })
            .unwrap_or_default();
        match self {
            Color::Rgb(c) => dml::EG_ColorChoice::SrgbClr(Box::new(dml::CT_SRgbColor {
                val: Some(HexBinary(vec![c.0, c.1, c.2])),
                color_transform: transform,
                ..Default::default()
            })),
            Color::Scheme(s) => dml::EG_ColorChoice::SchemeClr(Box::new(dml::CT_SchemeColor {
                val: Some(s.to_dml()),
                color_transform: transform,
                ..Default::default()
            })),
        }
    }

    /// The colour as a DrawingML colour choice.
    pub(crate) fn to_dml(self) -> dml::EG_ColorChoice {
        self.to_dml_alpha(None)
    }

    /// The colour wrapped in `CT_Color`.
    pub(crate) fn to_ct(self) -> dml::CT_Color {
        dml::CT_Color {
            color_choice: Some(self.to_dml()),
            ..Default::default()
        }
    }

    /// Reads an sRGB, system (last colour) or scheme colour.
    pub(crate) fn from_dml(c: &dml::EG_ColorChoice) -> Option<Color> {
        match c {
            dml::EG_ColorChoice::SrgbClr(c) => {
                let b = c.val.as_ref()?.as_bytes();
                (b.len() == 3).then(|| Color::Rgb(Rgb(b[0], b[1], b[2])))
            }
            dml::EG_ColorChoice::SysClr(c) => {
                let b = c.last_clr.as_ref()?.as_bytes();
                (b.len() == 3).then(|| Color::Rgb(Rgb(b[0], b[1], b[2])))
            }
            dml::EG_ColorChoice::SchemeClr(c) => SchemeColor::from_dml(c.val?).map(Color::Scheme),
            _ => None,
        }
    }
}

/// A DrawingML percentage in thousandths of a percent (`100000` = 100%).
pub(crate) fn fixed_percentage(v: f64) -> dml::ST_PositiveFixedPercentage {
    dml::ST_PositiveFixedPercentage::PositiveFixedPercentageDecimal(
        (v.clamp(0.0, 1.0) * 100_000.0).round() as i32
    )
}

/// A signed DrawingML percentage (`100000` = 100%).
pub(crate) fn percentage(v: f64) -> dml::ST_Percentage {
    dml::ST_Percentage::PercentageDecimal((v * 100_000.0).round() as i32)
}

/// Reads a DrawingML percentage as a fraction (`1.0` = 100%).
pub(crate) fn percentage_value(p: &dml::ST_Percentage) -> Option<f64> {
    match p {
        dml::ST_Percentage::PercentageDecimal(v) => Some(f64::from(*v) / 100_000.0),
        dml::ST_Percentage::Percentage(s) => parse_percent_string(s),
    }
}

fn parse_percent_string(s: &shared_types::ST_Percentage) -> Option<f64> {
    s.trim_end_matches('%').parse::<f64>().ok().map(|v| v / 100.0)
}

/// An angle in 60 000ths of a degree.
pub(crate) fn angle(degrees: f64) -> i32 {
    (degrees.rem_euclid(360.0) * 60_000.0).round() as i32
}

/// Converts 60 000ths of a degree to degrees.
pub(crate) fn degrees(angle: i32) -> f64 {
    f64::from(angle) / 60_000.0
}

/// A gradient fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// Colour stops: position in `0.0..=1.0` and colour.
    pub stops: Vec<(f64, Color)>,
    /// Direction of a linear gradient, in degrees (0 = left to right, 90 = top to bottom).
    pub angle: f64,
}

impl Gradient {
    /// A linear gradient between two colours.
    pub fn linear(from: impl Into<Color>, to: impl Into<Color>, angle: f64) -> Self {
        Gradient {
            stops: vec![(0.0, from.into()), (1.0, to.into())],
            angle,
        }
    }
}

/// How the inside of a shape is painted.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// Transparent.
    None,
    /// A single colour.
    Solid(Color),
    /// A single colour with opacity (`0.0` transparent … `1.0` opaque).
    SolidAlpha(Color, f64),
    /// A gradient.
    Gradient(Gradient),
    /// A two-colour pattern.
    Pattern {
        /// The pattern.
        pattern: PatternType,
        /// Colour of the pattern lines.
        foreground: Color,
        /// Colour behind the pattern.
        background: Color,
    },
}

impl Fill {
    /// A solid fill.
    pub fn solid(color: impl Into<Color>) -> Self {
        Fill::Solid(color.into())
    }

    pub(crate) fn to_dml(&self) -> dml::EG_FillProperties {
        match self {
            Fill::None => dml::EG_FillProperties::NoFill(Box::default()),
            Fill::Solid(c) => solid(*c, None),
            Fill::SolidAlpha(c, a) => solid(*c, Some(*a)),
            Fill::Gradient(g) => dml::EG_FillProperties::GradFill(Box::new(gradient(g))),
            Fill::Pattern {
                pattern,
                foreground,
                background,
            } => dml::EG_FillProperties::PattFill(Box::new(dml::CT_PatternFillProperties {
                prst: Some(*pattern),
                fg_clr: Some(Box::new(foreground.to_ct())),
                bg_clr: Some(Box::new(background.to_ct())),
                ..Default::default()
            })),
        }
    }

    /// Reads the fills this module can produce.
    pub(crate) fn from_dml(f: &dml::EG_FillProperties) -> Option<Fill> {
        Some(match f {
            dml::EG_FillProperties::NoFill(_) => Fill::None,
            dml::EG_FillProperties::SolidFill(s) => {
                let choice = s.color_choice.as_ref()?;
                let color = Color::from_dml(choice)?;
                match color_alpha(choice) {
                    Some(a) if a < 1.0 => Fill::SolidAlpha(color, a),
                    _ => Fill::Solid(color),
                }
            }
            dml::EG_FillProperties::GradFill(g) => {
                let stops = g
                    .gs_lst
                    .as_ref()?
                    .gs
                    .iter()
                    .filter_map(|s| {
                        let pos = match s.pos.as_ref()? {
                            dml::ST_PositiveFixedPercentage::PositiveFixedPercentageDecimal(v) => {
                                f64::from(*v) / 100_000.0
                            }
                            dml::ST_PositiveFixedPercentage::PositiveFixedPercentage(s) => {
                                s.trim_end_matches('%').parse::<f64>().ok()? / 100.0
                            }
                        };
                        Some((pos, Color::from_dml(s.color_choice.as_ref()?)?))
                    })
                    .collect();
                let angle = match &g.shade_properties {
                    Some(dml::EG_ShadeProperties::Lin(l)) => l.ang.map(degrees).unwrap_or(0.0),
                    _ => 0.0,
                };
                Fill::Gradient(Gradient { stops, angle })
            }
            dml::EG_FillProperties::PattFill(p) => Fill::Pattern {
                pattern: p.prst?,
                foreground: Color::from_dml(p.fg_clr.as_ref()?.color_choice.as_ref()?)?,
                background: Color::from_dml(p.bg_clr.as_ref()?.color_choice.as_ref()?)?,
            },
            _ => return None,
        })
    }
}

impl From<Rgb> for Fill {
    fn from(c: Rgb) -> Self {
        Fill::Solid(Color::Rgb(c))
    }
}

fn solid(c: Color, alpha: Option<f64>) -> dml::EG_FillProperties {
    dml::EG_FillProperties::SolidFill(Box::new(dml::CT_SolidColorFillProperties {
        color_choice: Some(c.to_dml_alpha(alpha)),
        ..Default::default()
    }))
}

fn gradient(g: &Gradient) -> dml::CT_GradientFillProperties {
    dml::CT_GradientFillProperties {
        rot_with_shape: Some(true),
        gs_lst: Some(Box::new(dml::CT_GradientStopList {
            gs: g
                .stops
                .iter()
                .map(|(pos, c)| dml::CT_GradientStop {
                    pos: Some(fixed_percentage(*pos)),
                    color_choice: Some(c.to_dml()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })),
        shade_properties: Some(dml::EG_ShadeProperties::Lin(Box::new(
            dml::CT_LinearShadeProperties {
                ang: Some(angle(g.angle)),
                scaled: Some(false),
                ..Default::default()
            },
        ))),
        ..Default::default()
    }
}

/// Dash pattern of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineDash {
    /// Continuous.
    Solid,
    /// Round dots.
    Dot,
    /// Dashes.
    Dash,
    /// Long dashes.
    LongDash,
    /// Dash, dot.
    DashDot,
    /// Long dash, dot.
    LongDashDot,
    /// Long dash, dot, dot.
    LongDashDotDot,
    /// Short dashes (system style).
    SysDash,
    /// Short dots (system style).
    SysDot,
    /// Short dash, dot (system style).
    SysDashDot,
    /// Short dash, dot, dot (system style).
    SysDashDotDot,
}

impl LineDash {
    fn to_dml(self) -> dml::ST_PresetLineDashVal {
        use dml::ST_PresetLineDashVal as V;
        match self {
            LineDash::Solid => V::Solid,
            LineDash::Dot => V::Dot,
            LineDash::Dash => V::Dash,
            LineDash::LongDash => V::LgDash,
            LineDash::DashDot => V::DashDot,
            LineDash::LongDashDot => V::LgDashDot,
            LineDash::LongDashDotDot => V::LgDashDotDot,
            LineDash::SysDash => V::SysDash,
            LineDash::SysDot => V::SysDot,
            LineDash::SysDashDot => V::SysDashDot,
            LineDash::SysDashDotDot => V::SysDashDotDot,
        }
    }

    fn from_dml(v: dml::ST_PresetLineDashVal) -> LineDash {
        use dml::ST_PresetLineDashVal as V;
        match v {
            V::Solid => LineDash::Solid,
            V::Dot => LineDash::Dot,
            V::Dash => LineDash::Dash,
            V::LgDash => LineDash::LongDash,
            V::DashDot => LineDash::DashDot,
            V::LgDashDot => LineDash::LongDashDot,
            V::LgDashDotDot => LineDash::LongDashDotDot,
            V::SysDash => LineDash::SysDash,
            V::SysDot => LineDash::SysDot,
            V::SysDashDot => LineDash::SysDashDot,
            V::SysDashDotDot => LineDash::SysDashDotDot,
        }
    }
}

/// Shape of a line end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrowKind {
    /// Plain end.
    None,
    /// Filled triangle.
    Triangle,
    /// Stealth (notched) arrow.
    Stealth,
    /// Diamond.
    Diamond,
    /// Oval.
    Oval,
    /// Open arrow.
    Arrow,
}

/// Size of a line end relative to the line width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrowSize {
    /// Small.
    Small,
    /// Medium.
    Medium,
    /// Large.
    Large,
}

/// A decoration at the start (head) or end (tail) of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArrowHead {
    /// Shape.
    pub kind: ArrowKind,
    /// Width.
    pub width: ArrowSize,
    /// Length.
    pub length: ArrowSize,
}

impl ArrowHead {
    /// A medium arrow head of the given shape.
    pub fn new(kind: ArrowKind) -> Self {
        ArrowHead {
            kind,
            width: ArrowSize::Medium,
            length: ArrowSize::Medium,
        }
    }

    fn to_dml(self) -> dml::CT_LineEndProperties {
        let kind = match self.kind {
            ArrowKind::None => dml::ST_LineEndType::None,
            ArrowKind::Triangle => dml::ST_LineEndType::Triangle,
            ArrowKind::Stealth => dml::ST_LineEndType::Stealth,
            ArrowKind::Diamond => dml::ST_LineEndType::Diamond,
            ArrowKind::Oval => dml::ST_LineEndType::Oval,
            ArrowKind::Arrow => dml::ST_LineEndType::Arrow,
        };
        let size_w = |s| match s {
            ArrowSize::Small => dml::ST_LineEndWidth::Sm,
            ArrowSize::Medium => dml::ST_LineEndWidth::Med,
            ArrowSize::Large => dml::ST_LineEndWidth::Lg,
        };
        let size_l = |s| match s {
            ArrowSize::Small => dml::ST_LineEndLength::Sm,
            ArrowSize::Medium => dml::ST_LineEndLength::Med,
            ArrowSize::Large => dml::ST_LineEndLength::Lg,
        };
        dml::CT_LineEndProperties {
            type_: Some(kind),
            w: Some(size_w(self.width)),
            len: Some(size_l(self.length)),
            ..Default::default()
        }
    }

    fn from_dml(e: &dml::CT_LineEndProperties) -> ArrowHead {
        let kind = match e.type_ {
            None | Some(dml::ST_LineEndType::None) => ArrowKind::None,
            Some(dml::ST_LineEndType::Triangle) => ArrowKind::Triangle,
            Some(dml::ST_LineEndType::Stealth) => ArrowKind::Stealth,
            Some(dml::ST_LineEndType::Diamond) => ArrowKind::Diamond,
            Some(dml::ST_LineEndType::Oval) => ArrowKind::Oval,
            Some(dml::ST_LineEndType::Arrow) => ArrowKind::Arrow,
        };
        let w = match e.w {
            Some(dml::ST_LineEndWidth::Sm) => ArrowSize::Small,
            Some(dml::ST_LineEndWidth::Lg) => ArrowSize::Large,
            _ => ArrowSize::Medium,
        };
        let l = match e.len {
            Some(dml::ST_LineEndLength::Sm) => ArrowSize::Small,
            Some(dml::ST_LineEndLength::Lg) => ArrowSize::Large,
            _ => ArrowSize::Medium,
        };
        ArrowHead {
            kind,
            width: w,
            length: l,
        }
    }
}

/// An outline (shape border, connector or table cell border).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Colour, or `None` for no line.
    pub color: Option<Color>,
    /// Width.
    pub width: Length,
    /// Dash pattern (`None` = solid).
    pub dash: Option<LineDash>,
    /// Decoration at the start.
    pub head: Option<ArrowHead>,
    /// Decoration at the end.
    pub tail: Option<ArrowHead>,
}

impl Line {
    /// A solid line.
    pub fn solid(color: impl Into<Color>, width: Length) -> Self {
        Line {
            color: Some(color.into()),
            width,
            dash: None,
            head: None,
            tail: None,
        }
    }

    /// No line.
    pub fn none() -> Self {
        Line {
            color: None,
            width: Length::ZERO,
            dash: None,
            head: None,
            tail: None,
        }
    }

    /// Sets the dash pattern.
    pub fn dash(mut self, dash: LineDash) -> Self {
        self.dash = Some(dash);
        self
    }

    /// Sets the start decoration.
    pub fn head(mut self, head: ArrowHead) -> Self {
        self.head = Some(head);
        self
    }

    /// Sets the end decoration.
    pub fn tail(mut self, tail: ArrowHead) -> Self {
        self.tail = Some(tail);
        self
    }

    pub(crate) fn to_dml(&self) -> dml::CT_LineProperties {
        let fill = match self.color {
            Some(c) => dml::EG_LineFillProperties::SolidFill(Box::new(dml::CT_SolidColorFillProperties {
                color_choice: Some(c.to_dml()),
                ..Default::default()
            })),
            None => dml::EG_LineFillProperties::NoFill(Box::default()),
        };
        dml::CT_LineProperties {
            w: self
                .color
                .is_some()
                .then(|| self.width.as_emu().clamp(0, 20_116_800) as i32),
            line_fill_properties: Some(fill),
            line_dash_properties: self.dash.map(|d| {
                dml::EG_LineDashProperties::PrstDash(Box::new(dml::CT_PresetLineDashProperties {
                    val: Some(d.to_dml()),
                    ..Default::default()
                }))
            }),
            head_end: self.head.map(|h| Box::new(h.to_dml())),
            tail_end: self.tail.map(|t| Box::new(t.to_dml())),
            ..Default::default()
        }
    }

    pub(crate) fn from_dml(ln: &dml::CT_LineProperties) -> Line {
        let color = match &ln.line_fill_properties {
            Some(dml::EG_LineFillProperties::SolidFill(s)) => {
                s.color_choice.as_ref().and_then(Color::from_dml)
            }
            _ => None,
        };
        Line {
            color,
            width: Length::emu(i64::from(ln.w.unwrap_or(0))),
            dash: match &ln.line_dash_properties {
                Some(dml::EG_LineDashProperties::PrstDash(d)) => d.val.map(LineDash::from_dml),
                _ => None,
            },
            head: ln.head_end.as_deref().map(ArrowHead::from_dml),
            tail: ln.tail_end.as_deref().map(ArrowHead::from_dml),
        }
    }
}

/// An outer shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Shadow colour.
    pub color: Color,
    /// Opacity (`0.0` … `1.0`).
    pub alpha: f64,
    /// Blur radius.
    pub blur: Length,
    /// Distance from the shape.
    pub distance: Length,
    /// Direction in degrees (0 = right, 90 = down).
    pub direction: f64,
}

impl Default for Shadow {
    /// The "Offset: Bottom Right" preset of PowerPoint.
    fn default() -> Self {
        Shadow {
            color: Color::Rgb(Rgb::BLACK),
            alpha: 0.4,
            blur: Length::pt(4.0),
            distance: Length::pt(3.0),
            direction: 45.0,
        }
    }
}

impl Shadow {
    pub(crate) fn to_dml(self) -> dml::CT_OuterShadowEffect {
        dml::CT_OuterShadowEffect {
            blur_rad: Some(self.blur.as_emu().max(0)),
            dist: Some(self.distance.as_emu().max(0)),
            dir: Some(angle(self.direction)),
            algn: Some(dml::ST_RectAlignment::Tl),
            rot_with_shape: Some(false),
            color_choice: Some(self.color.to_dml_alpha(Some(self.alpha))),
            ..Default::default()
        }
    }

    /// An effect list holding only this shadow.
    pub(crate) fn effect_list(self) -> dml::EG_EffectProperties {
        dml::EG_EffectProperties::EffectLst(Box::new(dml::CT_EffectList {
            outer_shdw: Some(Box::new(self.to_dml())),
            ..Default::default()
        }))
    }

    pub(crate) fn from_dml(s: &dml::CT_OuterShadowEffect) -> Shadow {
        let alpha = s.color_choice.as_ref().and_then(color_alpha);
        Shadow {
            color: s
                .color_choice
                .as_ref()
                .and_then(Color::from_dml)
                .unwrap_or(Color::Rgb(Rgb::BLACK)),
            alpha: alpha.unwrap_or(1.0),
            blur: Length::emu(s.blur_rad.unwrap_or(0)),
            distance: Length::emu(s.dist.unwrap_or(0)),
            direction: s.dir.map(degrees).unwrap_or(0.0),
        }
    }
}

/// The alpha modifier of an sRGB or scheme colour.
pub(crate) fn color_alpha(c: &dml::EG_ColorChoice) -> Option<f64> {
    match c {
        dml::EG_ColorChoice::SrgbClr(c) => alpha_of(&c.color_transform),
        dml::EG_ColorChoice::SchemeClr(c) => alpha_of(&c.color_transform),
        _ => None,
    }
}

fn alpha_of(transforms: &[dml::EG_ColorTransform]) -> Option<f64> {
    transforms.iter().find_map(|t| match t {
        dml::EG_ColorTransform::Alpha(a) => match a.val.as_ref()? {
            dml::ST_PositiveFixedPercentage::PositiveFixedPercentageDecimal(v) => {
                Some(f64::from(*v) / 100_000.0)
            }
            dml::ST_PositiveFixedPercentage::PositiveFixedPercentage(s) => {
                s.trim_end_matches('%').parse::<f64>().ok().map(|v| v / 100.0)
            }
        },
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        for c in [
            Color::Rgb(Rgb(1, 2, 3)),
            Color::Scheme(SchemeColor::Accent2),
            Color::Scheme(SchemeColor::Background1),
        ] {
            assert_eq!(Color::from_dml(&c.to_dml()), Some(c));
        }
        let all = [
            SchemeColor::Dark1,
            SchemeColor::Light1,
            SchemeColor::Dark2,
            SchemeColor::Light2,
            SchemeColor::Accent1,
            SchemeColor::Accent2,
            SchemeColor::Accent3,
            SchemeColor::Accent4,
            SchemeColor::Accent5,
            SchemeColor::Accent6,
            SchemeColor::Hyperlink,
            SchemeColor::FollowedHyperlink,
            SchemeColor::Text1,
            SchemeColor::Background1,
            SchemeColor::Text2,
            SchemeColor::Background2,
        ];
        for s in all {
            assert_eq!(SchemeColor::from_dml(s.to_dml()), Some(s));
        }
        assert_eq!(SchemeColor::from_dml(dml::ST_SchemeColorVal::PhClr), None);
        let sys = dml::EG_ColorChoice::SysClr(Box::new(dml::CT_SystemColor {
            last_clr: Some(HexBinary(vec![9, 8, 7])),
            ..Default::default()
        }));
        assert_eq!(Color::from_dml(&sys), Some(Color::Rgb(Rgb(9, 8, 7))));
        assert_eq!(Color::from(Rgb::WHITE), Color::Rgb(Rgb::WHITE));
        assert_eq!(
            Color::from(SchemeColor::Accent1),
            Color::Scheme(SchemeColor::Accent1)
        );
    }

    #[test]
    fn fills_round_trip() {
        let fills = [
            Fill::None,
            Fill::solid(Rgb(10, 20, 30)),
            Fill::Gradient(Gradient::linear(Rgb::WHITE, SchemeColor::Accent1, 90.0)),
            Fill::Pattern {
                pattern: PatternType::DkDnDiag,
                foreground: Color::Rgb(Rgb::BLACK),
                background: Color::Rgb(Rgb::WHITE),
            },
        ];
        for f in fills {
            assert_eq!(Fill::from_dml(&f.to_dml()), Some(f.clone()), "{f:?}");
        }
        let alpha = Fill::SolidAlpha(Color::Rgb(Rgb(1, 1, 1)), 0.5).to_dml();
        assert_eq!(
            Fill::from_dml(&alpha),
            Some(Fill::SolidAlpha(Color::Rgb(Rgb(1, 1, 1)), 0.5))
        );
        assert_eq!(Fill::from(Rgb::BLACK), Fill::solid(Rgb::BLACK));
    }

    #[test]
    fn lines_round_trip() {
        let line = Line::solid(Rgb(255, 0, 0), Length::pt(2.0))
            .dash(LineDash::DashDot)
            .head(ArrowHead::new(ArrowKind::Oval))
            .tail(ArrowHead {
                kind: ArrowKind::Triangle,
                width: ArrowSize::Large,
                length: ArrowSize::Small,
            });
        assert_eq!(Line::from_dml(&line.to_dml()), line);
        let none = Line::none();
        let back = Line::from_dml(&none.to_dml());
        assert_eq!(back.color, None);
        for d in [
            LineDash::Solid,
            LineDash::Dot,
            LineDash::Dash,
            LineDash::LongDash,
            LineDash::DashDot,
            LineDash::LongDashDot,
            LineDash::LongDashDotDot,
            LineDash::SysDash,
            LineDash::SysDot,
            LineDash::SysDashDot,
            LineDash::SysDashDotDot,
        ] {
            assert_eq!(LineDash::from_dml(d.to_dml()), d);
        }
        for k in [
            ArrowKind::None,
            ArrowKind::Triangle,
            ArrowKind::Stealth,
            ArrowKind::Diamond,
            ArrowKind::Oval,
            ArrowKind::Arrow,
        ] {
            assert_eq!(ArrowHead::from_dml(&ArrowHead::new(k).to_dml()).kind, k);
        }
    }

    #[test]
    fn shadows_and_units() {
        let s = Shadow {
            color: Color::Rgb(Rgb(0, 0, 0)),
            alpha: 0.5,
            blur: Length::pt(5.0),
            distance: Length::pt(2.0),
            direction: 90.0,
        };
        assert_eq!(Shadow::from_dml(&s.to_dml()), s);
        assert!(matches!(s.effect_list(), dml::EG_EffectProperties::EffectLst(_)));
        assert_eq!(Shadow::default().direction, 45.0);
        assert_eq!(angle(90.0), 5_400_000);
        assert_eq!(angle(-90.0), 16_200_000);
        assert_eq!(degrees(5_400_000), 90.0);
        assert_eq!(percentage_value(&percentage(0.25)), Some(0.25));
        assert_eq!(
            percentage_value(&dml::ST_Percentage::Percentage("50%".into())),
            Some(0.5)
        );
        assert!(matches!(
            fixed_percentage(2.0),
            dml::ST_PositiveFixedPercentage::PositiveFixedPercentageDecimal(100_000)
        ));
    }
}
