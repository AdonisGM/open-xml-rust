//! DrawingML objects in the text: inline and floating (anchored) frames
//! around any `a:graphicData` (pictures today; charts or diagrams can reuse
//! [`inline_graphic`]/[`anchor_graphic`]), alternative text, image
//! hyperlinks and picture replacement.

use openxml_core::{Error, Length, Result, sniff_image};
use openxml_opc::PartName;
use openxml_opc::known::rel_types;
use openxml_schema::{dml, dml_picture, dml_wordprocessing_drawing as wp, wml};
use openxml_xml::{Ns, RawElement, RawNode};

use crate::document::{Document, Shared};
use crate::paragraph::ParagraphMut;
use crate::picture::PICTURE_URI;
use crate::{text, walk};

/// Reference point for a horizontal position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAnchor {
    /// The page edge.
    Page,
    /// The page margins.
    Margin,
    /// The column containing the anchor paragraph.
    Column,
    /// The character where the object is anchored.
    Character,
    /// The left margin area.
    LeftMargin,
    /// The right margin area.
    RightMargin,
    /// The inside margin (odd/even pages).
    InsideMargin,
    /// The outside margin (odd/even pages).
    OutsideMargin,
}

/// Reference point for a vertical position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// The page edge.
    Page,
    /// The page margins.
    Margin,
    /// The anchor paragraph.
    Paragraph,
    /// The line containing the anchor.
    Line,
    /// The top margin area.
    TopMargin,
    /// The bottom margin area.
    BottomMargin,
    /// The inside margin.
    InsideMargin,
    /// The outside margin.
    OutsideMargin,
}

/// Horizontal alignment relative to a [`HorizontalAnchor`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlignment {
    /// Left.
    Left,
    /// Centered.
    Center,
    /// Right.
    Right,
    /// Inside (odd/even pages).
    Inside,
    /// Outside (odd/even pages).
    Outside,
}

/// Vertical alignment relative to a [`VerticalAnchor`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    /// Top.
    Top,
    /// Centered.
    Center,
    /// Bottom.
    Bottom,
    /// Inside.
    Inside,
    /// Outside.
    Outside,
}

/// Horizontal position of a floating object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalPosition {
    /// Offset of the left edge from the anchor.
    Offset(HorizontalAnchor, Length),
    /// Aligned relative to the anchor.
    Align(HorizontalAnchor, HorizontalAlignment),
}

/// Vertical position of a floating object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalPosition {
    /// Offset of the top edge from the anchor.
    Offset(VerticalAnchor, Length),
    /// Aligned relative to the anchor.
    Align(VerticalAnchor, VerticalAlignment),
}

/// How text flows around a floating object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// Around the bounding box.
    Square,
    /// Tightly around the object's outline.
    Tight,
    /// Above and below only.
    TopAndBottom,
    /// No wrapping, the object is behind the text.
    BehindText,
    /// No wrapping, the object is in front of the text.
    InFrontOfText,
}

/// Placement of a floating (anchored) object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Floating {
    /// Horizontal position.
    pub horizontal: HorizontalPosition,
    /// Vertical position.
    pub vertical: VerticalPosition,
    /// Text wrapping.
    pub wrap: Wrap,
    /// Minimum distance between the object and the surrounding text.
    pub distance: Length,
    /// Whether the object may overlap other floating objects.
    pub allow_overlap: bool,
}

impl Floating {
    /// A floating placement with a 1/8 inch distance from the text.
    pub fn new(horizontal: HorizontalPosition, vertical: VerticalPosition, wrap: Wrap) -> Self {
        Floating {
            horizontal,
            vertical,
            wrap,
            distance: Length::inches(0.125),
            allow_overlap: true,
        }
    }
}

/// Options of a picture.
///
/// ```
/// use openxml_docx::*;
/// # let png = openxml_core::image::tiny_png(40, 20);
///
/// let mut doc = Document::new();
/// let mut options = PictureOptions::new(Length::cm(4.0));
/// options.description = Some("Company logo".into());
/// options.floating = Some(Floating::new(
///     HorizontalPosition::Align(HorizontalAnchor::Margin, HorizontalAlignment::Right),
///     VerticalPosition::Offset(VerticalAnchor::Paragraph, Length::ZERO),
///     Wrap::Square,
/// ));
/// doc.add_paragraph("Text flows around the logo.").add_picture_with(&png, &options)?;
/// let pictures = doc.pictures();
/// assert!(pictures[0].floating);
/// assert_eq!(pictures[0].description.as_deref(), Some("Company logo"));
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureOptions {
    /// Displayed width.
    pub width: Length,
    /// Displayed height; derived from the aspect ratio when `None`.
    pub height: Option<Length>,
    /// Alternative text (`descr`); the file name when `None`.
    pub description: Option<String>,
    /// Title (`title`).
    pub title: Option<String>,
    /// URL opened when the picture is clicked.
    pub hyperlink: Option<String>,
    /// Floating placement; inline when `None`.
    pub floating: Option<Floating>,
}

impl PictureOptions {
    /// An inline picture `width` wide keeping its aspect ratio.
    pub fn new(width: Length) -> Self {
        PictureOptions {
            width,
            height: None,
            description: None,
            title: None,
            hyperlink: None,
            floating: None,
        }
    }
}

/// A picture found in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureInfo {
    /// Drawing object id (`wp:docPr/@id`).
    pub id: u32,
    /// Object name.
    pub name: Option<String>,
    /// Alternative text.
    pub description: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Displayed width.
    pub width: Length,
    /// Displayed height.
    pub height: Length,
    /// Whether the picture floats (`wp:anchor`) rather than being inline.
    pub floating: bool,
    /// Relationship id of the embedded image.
    pub relationship_id: Option<String>,
    /// The image part.
    pub image_part: Option<PartName>,
}

/// Non-visual properties of a drawing frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct FrameInfo {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub title: Option<String>,
    pub hyperlink_rel: Option<String>,
}

fn doc_pr(info: &FrameInfo) -> Box<dml::CT_NonVisualDrawingProps> {
    Box::new(dml::CT_NonVisualDrawingProps {
        id: Some(info.id),
        name: Some(info.name.clone()),
        descr: info.description.clone(),
        title: info.title.clone(),
        hlink_click: info.hyperlink_rel.as_ref().map(|id| {
            Box::new(dml::CT_Hyperlink {
                r_id: Some(id.clone()),
                ..Default::default()
            })
        }),
        ..Default::default()
    })
}

fn size(width: Length, height: Length) -> Box<dml::CT_PositiveSize2D> {
    Box::new(dml::CT_PositiveSize2D {
        cx: Some(width.as_emu().max(0)),
        cy: Some(height.as_emu().max(0)),
        ..Default::default()
    })
}

fn zero_effect_extent() -> Box<wp::CT_EffectExtent> {
    let zero = || Some(dml::ST_Coordinate::CoordinateUnqualified(0));
    Box::new(wp::CT_EffectExtent {
        l: zero(),
        t: zero(),
        r: zero(),
        b: zero(),
        ..Default::default()
    })
}

fn graphic(data: dml::CT_GraphicalObjectData) -> Box<dml::CT_GraphicalObject> {
    Box::new(dml::CT_GraphicalObject {
        graphic_data: Some(Box::new(data)),
        ..Default::default()
    })
}

fn frame_locks(is_picture: bool) -> Box<dml::CT_NonVisualGraphicFrameProperties> {
    Box::new(dml::CT_NonVisualGraphicFrameProperties {
        graphic_frame_locks: is_picture.then(|| {
            Box::new(dml::CT_GraphicalObjectFrameLocking {
                no_change_aspect: Some(true),
                ..Default::default()
            })
        }),
        ..Default::default()
    })
}

/// Wraps any graphic object data in an inline frame (`wp:inline`).
pub(crate) fn inline_graphic(
    data: dml::CT_GraphicalObjectData,
    width: Length,
    height: Length,
    info: &FrameInfo,
) -> wp::CT_Inline {
    let is_picture = data.uri.as_deref() == Some(PICTURE_URI);
    wp::CT_Inline {
        dist_t: Some(0),
        dist_b: Some(0),
        dist_l: Some(0),
        dist_r: Some(0),
        extent: Some(size(width, height)),
        effect_extent: Some(zero_effect_extent()),
        doc_pr: Some(doc_pr(info)),
        c_nv_graphic_frame_pr: Some(frame_locks(is_picture)),
        graphic: Some(graphic(data)),
        ..Default::default()
    }
}

fn emu(len: Length) -> i32 {
    len.as_emu().clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn wrap_distance(len: Length) -> u32 {
    len.as_emu().clamp(0, i64::from(u32::MAX)) as u32
}

/// Wraps any graphic object data in a floating frame (`wp:anchor`).
pub(crate) fn anchor_graphic(
    data: dml::CT_GraphicalObjectData,
    width: Length,
    height: Length,
    info: &FrameInfo,
    floating: &Floating,
) -> wp::CT_Anchor {
    let is_picture = data.uri.as_deref() == Some(PICTURE_URI);
    let d = wrap_distance(floating.distance);
    let position_h = match floating.horizontal {
        HorizontalPosition::Offset(a, off) => (a, wp::CT_PosH_Choice::PosOffset(emu(off))),
        HorizontalPosition::Align(a, al) => (
            a,
            wp::CT_PosH_Choice::Align(match al {
                HorizontalAlignment::Left => wp::ST_AlignH::Left,
                HorizontalAlignment::Center => wp::ST_AlignH::Center,
                HorizontalAlignment::Right => wp::ST_AlignH::Right,
                HorizontalAlignment::Inside => wp::ST_AlignH::Inside,
                HorizontalAlignment::Outside => wp::ST_AlignH::Outside,
            }),
        ),
    };
    let position_v = match floating.vertical {
        VerticalPosition::Offset(a, off) => (a, wp::CT_PosV_Choice::PosOffset(emu(off))),
        VerticalPosition::Align(a, al) => (
            a,
            wp::CT_PosV_Choice::Align(match al {
                VerticalAlignment::Top => wp::ST_AlignV::Top,
                VerticalAlignment::Center => wp::ST_AlignV::Center,
                VerticalAlignment::Bottom => wp::ST_AlignV::Bottom,
                VerticalAlignment::Inside => wp::ST_AlignV::Inside,
                VerticalAlignment::Outside => wp::ST_AlignV::Outside,
            }),
        ),
    };
    let wrap = match floating.wrap {
        Wrap::Square => wp::CT_Anchor_Choice::WrapSquare(Box::new(wp::CT_WrapSquare {
            wrap_text: Some(wp::ST_WrapText::BothSides),
            ..Default::default()
        })),
        Wrap::Tight => {
            // The wrap polygon of the bounding box, in the 21600-unit space Word uses.
            let pt = |x: i64, y: i64| dml::CT_Point2D {
                x: Some(dml::ST_Coordinate::CoordinateUnqualified(x)),
                y: Some(dml::ST_Coordinate::CoordinateUnqualified(y)),
                ..Default::default()
            };
            wp::CT_Anchor_Choice::WrapTight(Box::new(wp::CT_WrapTight {
                wrap_text: Some(wp::ST_WrapText::BothSides),
                wrap_polygon: Some(Box::new(wp::CT_WrapPath {
                    edited: Some(false),
                    start: Some(Box::new(pt(0, 0))),
                    line_to: vec![pt(0, 21600), pt(21600, 21600), pt(21600, 0), pt(0, 0)],
                    ..Default::default()
                })),
                ..Default::default()
            }))
        }
        Wrap::TopAndBottom => wp::CT_Anchor_Choice::WrapTopAndBottom(Box::default()),
        Wrap::BehindText | Wrap::InFrontOfText => wp::CT_Anchor_Choice::WrapNone(Box::default()),
    };
    wp::CT_Anchor {
        dist_t: Some(d),
        dist_b: Some(d),
        dist_l: Some(d),
        dist_r: Some(d),
        simple_pos_attr: Some(false),
        relative_height: Some(251_658_240 + info.id),
        behind_doc: Some(floating.wrap == Wrap::BehindText),
        locked: Some(false),
        layout_in_cell: Some(true),
        allow_overlap: Some(floating.allow_overlap),
        simple_pos: Some(Box::new(dml::CT_Point2D {
            x: Some(dml::ST_Coordinate::CoordinateUnqualified(0)),
            y: Some(dml::ST_Coordinate::CoordinateUnqualified(0)),
            ..Default::default()
        })),
        position_h: Some(Box::new(wp::CT_PosH {
            relative_from: Some(match position_h.0 {
                HorizontalAnchor::Page => wp::ST_RelFromH::Page,
                HorizontalAnchor::Margin => wp::ST_RelFromH::Margin,
                HorizontalAnchor::Column => wp::ST_RelFromH::Column,
                HorizontalAnchor::Character => wp::ST_RelFromH::Character,
                HorizontalAnchor::LeftMargin => wp::ST_RelFromH::LeftMargin,
                HorizontalAnchor::RightMargin => wp::ST_RelFromH::RightMargin,
                HorizontalAnchor::InsideMargin => wp::ST_RelFromH::InsideMargin,
                HorizontalAnchor::OutsideMargin => wp::ST_RelFromH::OutsideMargin,
            }),
            choice: Some(position_h.1),
            ..Default::default()
        })),
        position_v: Some(Box::new(wp::CT_PosV {
            relative_from: Some(match position_v.0 {
                VerticalAnchor::Page => wp::ST_RelFromV::Page,
                VerticalAnchor::Margin => wp::ST_RelFromV::Margin,
                VerticalAnchor::Paragraph => wp::ST_RelFromV::Paragraph,
                VerticalAnchor::Line => wp::ST_RelFromV::Line,
                VerticalAnchor::TopMargin => wp::ST_RelFromV::TopMargin,
                VerticalAnchor::BottomMargin => wp::ST_RelFromV::BottomMargin,
                VerticalAnchor::InsideMargin => wp::ST_RelFromV::InsideMargin,
                VerticalAnchor::OutsideMargin => wp::ST_RelFromV::OutsideMargin,
            }),
            choice: Some(position_v.1),
            ..Default::default()
        })),
        extent: Some(size(width, height)),
        effect_extent: Some(zero_effect_extent()),
        choice: Some(wrap),
        doc_pr: Some(doc_pr(info)),
        c_nv_graphic_frame_pr: Some(frame_locks(is_picture)),
        graphic: Some(graphic(data)),
        ..Default::default()
    }
}

/// The `a:graphicData` of a picture (`pic:pic`) showing image `rel_id`.
pub(crate) fn picture_data(
    rel_id: &str,
    name: &str,
    info: &FrameInfo,
    width: Length,
    height: Length,
) -> dml::CT_GraphicalObjectData {
    let pic = dml_picture::CT_Picture {
        nv_pic_pr: Some(Box::new(dml_picture::CT_PictureNonVisual {
            c_nv_pr: Some(Box::new(dml::CT_NonVisualDrawingProps {
                id: Some(0),
                name: Some(name.to_owned()),
                descr: info.description.clone(),
                hlink_click: info.hyperlink_rel.as_ref().map(|id| {
                    Box::new(dml::CT_Hyperlink {
                        r_id: Some(id.clone()),
                        ..Default::default()
                    })
                }),
                ..Default::default()
            })),
            c_nv_pic_pr: Some(Box::default()),
            ..Default::default()
        })),
        blip_fill: Some(Box::new(dml::CT_BlipFillProperties {
            blip: Some(Box::new(dml::CT_Blip {
                r_embed: Some(rel_id.to_owned()),
                ..Default::default()
            })),
            fill_mode_properties: Some(dml::EG_FillModeProperties::Stretch(Box::new(
                dml::CT_StretchInfoProperties {
                    fill_rect: Some(Box::default()),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        })),
        sp_pr: Some(Box::new(dml::CT_ShapeProperties {
            xfrm: Some(Box::new(dml::CT_Transform2D {
                off: Some(Box::new(dml::CT_Point2D {
                    x: Some(dml::ST_Coordinate::CoordinateUnqualified(0)),
                    y: Some(dml::ST_Coordinate::CoordinateUnqualified(0)),
                    ..Default::default()
                })),
                ext: Some(size(width, height)),
                ..Default::default()
            })),
            geometry: Some(dml::EG_Geometry::PrstGeom(Box::new(dml::CT_PresetGeometry2D {
                prst: Some(dml::ST_ShapeType::Rect),
                av_lst: Some(Box::default()),
                ..Default::default()
            }))),
            ..Default::default()
        })),
        ..Default::default()
    };
    // a:graphicData holds xsd:any content, so the typed picture is converted to raw XML.
    dml::CT_GraphicalObjectData {
        uri: Some(PICTURE_URI.to_owned()),
        any: vec![RawElement::from_typed(&pic, Ns::PIC, "pic")],
        ..Default::default()
    }
}

/// Builds a `w:drawing` from graphic data, inline or floating.
pub(crate) fn drawing(
    data: dml::CT_GraphicalObjectData,
    width: Length,
    height: Length,
    info: &FrameInfo,
    floating: Option<&Floating>,
) -> wml::CT_Drawing {
    let choice = match floating {
        Some(f) => wml::CT_Drawing_Choice::Anchor(Box::new(anchor_graphic(data, width, height, info, f))),
        None => wml::CT_Drawing_Choice::Inline(Box::new(inline_graphic(data, width, height, info))),
    };
    wml::CT_Drawing {
        choice: vec![choice],
        ..Default::default()
    }
}

impl Shared {
    /// Adds an image part related from `part` and builds its drawing.
    pub(crate) fn add_picture_with(
        &mut self,
        part: &PartName,
        image: &[u8],
        options: &PictureOptions,
    ) -> Result<wml::CT_Drawing> {
        let info = sniff_image(image).ok_or(Error::UnsupportedImage)?;
        if options.width.as_emu() <= 0 || options.height.is_some_and(|h| h.as_emu() <= 0) {
            return Err(Error::InvalidArgument("picture size must be positive".into()));
        }
        let (width, natural_height) = info.size_for_width(options.width);
        let height = options.height.unwrap_or(natural_height);
        let pattern = format!("/word/media/image{{}}.{}", info.format.extension());
        let name = self.package.next_part_name(&pattern)?;
        self.package
            .add_part(name.clone(), info.format.content_type(), image.to_vec())?;
        let rel_id = self
            .package
            .add_relationship(Some(part), rel_types::IMAGE, &name)?;
        let hyperlink_rel = options
            .hyperlink
            .as_deref()
            .map(|url| self.add_hyperlink_relationship(part, url))
            .transpose()?;
        self.next_drawing_id += 1;
        let id = self.next_drawing_id;
        let frame = FrameInfo {
            id,
            name: format!("Picture {id}"),
            description: Some(
                options
                    .description
                    .clone()
                    .unwrap_or_else(|| name.file_name().to_owned()),
            ),
            title: options.title.clone(),
            hyperlink_rel,
        };
        let data = picture_data(&rel_id, name.file_name(), &frame, width, height);
        Ok(drawing(data, width, height, &frame, options.floating.as_ref()))
    }
}

impl ParagraphMut<'_> {
    /// Appends a picture with the given size, alternative text, hyperlink
    /// and placement (inline or floating).
    pub fn add_picture_with(&mut self, image: &[u8], options: &PictureOptions) -> Result<&mut Self> {
        let drawing = self.shared.add_picture_with(&self.part, image, options)?;
        self.p.p_content.push(wml::EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![wml::EG_RunInnerContent::Drawing(Box::new(drawing))],
            ..Default::default()
        })));
        Ok(self)
    }

    /// Appends any DrawingML graphic (for example a chart part's
    /// `c:chart` reference) in an inline or floating frame. Relationships
    /// referenced by `data` must already exist from this part. Returns the
    /// drawing object id.
    pub fn add_graphic(
        &mut self,
        data: dml::CT_GraphicalObjectData,
        width: Length,
        height: Length,
        name: &str,
        floating: Option<&Floating>,
    ) -> u32 {
        self.shared.next_drawing_id += 1;
        let id = self.shared.next_drawing_id;
        let info = FrameInfo {
            id,
            name: name.to_owned(),
            ..Default::default()
        };
        let drawing = drawing(data, width, height, &info, floating);
        self.p.p_content.push(wml::EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![wml::EG_RunInnerContent::Drawing(Box::new(drawing))],
            ..Default::default()
        })));
        id
    }
}

/// The picture element of graphic data, if it holds one.
fn picture_element(data: &dml::CT_GraphicalObjectData) -> Option<&RawElement> {
    (data.uri.as_deref() == Some(PICTURE_URI))
        .then(|| data.any.iter().find(|e| e.name.is(Ns::PIC, "pic")))
        .flatten()
}

fn blip_embed(pic: &RawElement) -> Option<String> {
    pic.descendants()
        .into_iter()
        .find(|e| e.name.is(Ns::A, "blip"))
        .and_then(|b| b.attr(Ns::R, "embed"))
        .map(str::to_owned)
}

fn set_blip_embed(e: &mut RawElement, rel_id: &str) -> bool {
    if e.name.is(Ns::A, "blip") {
        e.set_attr(Ns::R, "embed", rel_id);
        return true;
    }
    let mut found = false;
    for c in &mut e.children {
        if let RawNode::Element(child) = c {
            found |= set_blip_embed(child, rel_id);
        }
    }
    found
}

/// Frame properties shared by inline and anchored drawings.
struct Frame<'a> {
    doc_pr: Option<&'a dml::CT_NonVisualDrawingProps>,
    extent: Option<&'a dml::CT_PositiveSize2D>,
    data: Option<&'a dml::CT_GraphicalObjectData>,
    floating: bool,
}

fn frames(d: &wml::CT_Drawing) -> Vec<Frame<'_>> {
    d.choice
        .iter()
        .filter_map(|c| match c {
            wml::CT_Drawing_Choice::Inline(i) => Some(Frame {
                doc_pr: i.doc_pr.as_deref(),
                extent: i.extent.as_deref(),
                data: i.graphic.as_deref().and_then(|g| g.graphic_data.as_deref()),
                floating: false,
            }),
            wml::CT_Drawing_Choice::Anchor(a) => Some(Frame {
                doc_pr: a.doc_pr.as_deref(),
                extent: a.extent.as_deref(),
                data: a.graphic.as_deref().and_then(|g| g.graphic_data.as_deref()),
                floating: true,
            }),
            _ => None,
        })
        .collect()
}

/// Non-visual properties and graphic of a frame.
type FrameSlots<'a> = (
    &'a mut Option<Box<dml::CT_NonVisualDrawingProps>>,
    &'a mut Option<Box<dml::CT_GraphicalObject>>,
);

/// Mutable non-visual properties and graphic data of a drawing's frames.
fn frames_mut(d: &mut wml::CT_Drawing) -> Vec<FrameSlots<'_>> {
    d.choice
        .iter_mut()
        .filter_map(|c| match c {
            wml::CT_Drawing_Choice::Inline(i) => Some((&mut i.doc_pr, &mut i.graphic)),
            wml::CT_Drawing_Choice::Anchor(a) => Some((&mut a.doc_pr, &mut a.graphic)),
            _ => None,
        })
        .collect()
}

fn is_picture_frame(graphic: &Option<Box<dml::CT_GraphicalObject>>) -> bool {
    graphic
        .as_deref()
        .and_then(|g| g.graphic_data.as_deref())
        .and_then(picture_element)
        .is_some()
}

impl Document {
    /// Visits the drawings of the body (in paragraphs and tables).
    fn body_drawings_mut(&mut self, f: &mut dyn FnMut(&mut wml::CT_Drawing)) {
        let Some(body) = self.main.body.as_mut() else {
            return;
        };
        self.main_dirty = true;
        walk::walk_blocks(&mut body.block_level_elts, &mut |p| {
            for r in text::runs_mut(&mut p.p_content) {
                for c in &mut r.run_inner_content {
                    if let wml::EG_RunInnerContent::Drawing(d) = c {
                        f(d);
                    }
                }
            }
        });
    }

    /// Appends a paragraph holding a picture with the given options
    /// (see [`ParagraphMut::add_picture_with`]).
    pub fn add_picture_with(&mut self, image: &[u8], options: &PictureOptions) -> Result<ParagraphMut<'_>> {
        let part = self.shared.main_part.clone();
        let drawing = self.shared.add_picture_with(&part, image, options)?;
        let mut p = self.add_paragraph("");
        p.raw().p_content.push(wml::EG_PContent::R(Box::new(wml::CT_R {
            run_inner_content: vec![wml::EG_RunInnerContent::Drawing(Box::new(drawing))],
            ..Default::default()
        })));
        Ok(p)
    }

    /// Pictures of the body in document order.
    pub fn pictures(&self) -> Vec<PictureInfo> {
        let mut out = Vec::new();
        walk::walk_blocks_ref(&self.body().block_level_elts, &mut |p| {
            for r in text::runs(&p.p_content) {
                for c in &r.run_inner_content {
                    let wml::EG_RunInnerContent::Drawing(d) = c else {
                        continue;
                    };
                    for frame in frames(d) {
                        let Some(pic) = frame.data.and_then(picture_element) else {
                            continue;
                        };
                        let rel = blip_embed(pic);
                        let part = rel.as_deref().and_then(|id| {
                            self.shared
                                .package
                                .relationship_target(Some(&self.shared.main_part), id)
                        });
                        let doc_pr = frame.doc_pr;
                        out.push(PictureInfo {
                            id: doc_pr.and_then(|d| d.id).unwrap_or_default(),
                            name: doc_pr.and_then(|d| d.name.clone()),
                            description: doc_pr.and_then(|d| d.descr.clone()),
                            title: doc_pr.and_then(|d| d.title.clone()),
                            width: Length::emu(frame.extent.and_then(|e| e.cx).unwrap_or(0)),
                            height: Length::emu(frame.extent.and_then(|e| e.cy).unwrap_or(0)),
                            floating: frame.floating,
                            relationship_id: rel,
                            image_part: part,
                        });
                    }
                }
            }
        });
        out
    }

    /// Sets the alternative text and title of the `index`-th picture
    /// (in [`Document::pictures`] order).
    pub fn set_picture_alt_text(
        &mut self,
        index: usize,
        description: Option<&str>,
        title: Option<&str>,
    ) -> Result<()> {
        let mut n = 0usize;
        let mut done = false;
        self.body_drawings_mut(&mut |d| {
            for (doc_pr, graphic) in frames_mut(d) {
                if done || !is_picture_frame(graphic) {
                    continue;
                }
                if n == index {
                    let pr = doc_pr.get_or_insert_with(Default::default);
                    pr.descr = description.map(str::to_owned);
                    pr.title = title.map(str::to_owned);
                    done = true;
                }
                n += 1;
            }
        });
        if done {
            Ok(())
        } else {
            Err(Error::NotFound(format!("picture {index} (the body has {n})")))
        }
    }

    /// Replaces the image shown by the `index`-th picture. The frame keeps
    /// its size; the old image part is removed when nothing else uses it.
    pub fn replace_picture(&mut self, index: usize, image: &[u8]) -> Result<()> {
        let pictures = self.pictures();
        let count = pictures.len();
        let old = pictures
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("picture {index} (the body has {count})")))?;
        let info = sniff_image(image).ok_or(Error::UnsupportedImage)?;
        let pattern = format!("/word/media/image{{}}.{}", info.format.extension());
        let pkg = &mut self.shared.package;
        let name = pkg.next_part_name(&pattern)?;
        pkg.add_part(name.clone(), info.format.content_type(), image.to_vec())?;
        let rel_id = pkg.add_relationship(Some(&self.shared.main_part), rel_types::IMAGE, &name)?;
        let mut n = 0usize;
        self.body_drawings_mut(&mut |d| {
            for (_, graphic) in frames_mut(d) {
                if !is_picture_frame(graphic) {
                    continue;
                }
                if n == index
                    && let Some(data) = graphic.as_mut().and_then(|g| g.graphic_data.as_mut())
                {
                    for e in &mut data.any {
                        set_blip_embed(e, &rel_id);
                    }
                }
                n += 1;
            }
        });
        // Drop the previous image when no other picture uses it.
        if let Some(old_rel) = &old.relationship_id {
            let still_used = self
                .pictures()
                .iter()
                .any(|p| p.relationship_id.as_deref() == Some(old_rel.as_str()));
            if !still_used {
                self.remove_unused_relationship(old_rel, old.image_part.as_ref());
            }
        }
        Ok(())
    }

    /// Removes a relationship of the main part and its target part when no
    /// other relationship in the package points to it.
    fn remove_unused_relationship(&mut self, rel_id: &str, target: Option<&PartName>) {
        let main = self.shared.main_part.clone();
        if let Some(rels) = self.shared.package.relationships_mut(Some(&main)) {
            rels.remove(rel_id);
        }
        let Some(target) = target else { return };
        let pkg = &self.shared.package;
        let referenced = pkg.parts().any(|(name, part)| {
            part.relationships().iter().any(|r| {
                !r.is_external() && pkg.relationship_target(Some(name), &r.id).as_ref() == Some(target)
            })
        });
        if !referenced {
            self.shared.package.remove_part(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> FrameInfo {
        FrameInfo {
            id: 7,
            name: "Chart 7".into(),
            description: Some("alt".into()),
            title: Some("title".into()),
            hyperlink_rel: Some("rId9".into()),
        }
    }

    #[test]
    fn anchors_carry_position_and_wrapping() {
        let data = dml::CT_GraphicalObjectData {
            uri: Some("http://schemas.openxmlformats.org/drawingml/2006/chart".into()),
            ..Default::default()
        };
        let floating = Floating::new(
            HorizontalPosition::Offset(HorizontalAnchor::Page, Length::inches(1.0)),
            VerticalPosition::Align(VerticalAnchor::Margin, VerticalAlignment::Bottom),
            Wrap::BehindText,
        );
        let a = anchor_graphic(
            data.clone(),
            Length::inches(2.0),
            Length::inches(1.0),
            &info(),
            &floating,
        );
        assert_eq!(a.behind_doc, Some(true));
        assert!(matches!(a.choice, Some(wp::CT_Anchor_Choice::WrapNone(_))));
        let h = a.position_h.unwrap();
        assert_eq!(h.relative_from, Some(wp::ST_RelFromH::Page));
        assert_eq!(h.choice, Some(wp::CT_PosH_Choice::PosOffset(914_400)));
        assert_eq!(
            a.position_v.unwrap().choice,
            Some(wp::CT_PosV_Choice::Align(wp::ST_AlignV::Bottom))
        );
        // Not a picture: no aspect lock.
        assert!(a.c_nv_graphic_frame_pr.unwrap().graphic_frame_locks.is_none());
        let pr = a.doc_pr.unwrap();
        assert_eq!(
            (pr.descr.as_deref(), pr.title.as_deref()),
            (Some("alt"), Some("title"))
        );
        assert_eq!(pr.hlink_click.unwrap().r_id.as_deref(), Some("rId9"));

        for (wrap, check) in [
            (Wrap::Square, "wrapSquare"),
            (Wrap::Tight, "wrapTight"),
            (Wrap::TopAndBottom, "wrapTopAndBottom"),
            (Wrap::InFrontOfText, "wrapNone"),
        ] {
            let f = Floating { wrap, ..floating };
            let a = anchor_graphic(
                data.clone(),
                Length::inches(1.0),
                Length::inches(1.0),
                &info(),
                &f,
            );
            assert_eq!(a.choice.as_ref().unwrap().element_name().1, check);
            assert_eq!(a.behind_doc, Some(false));
        }
        let inline = inline_graphic(data, Length::inches(1.0), Length::inches(1.0), &info());
        assert_eq!(inline.doc_pr.unwrap().id, Some(7));
    }

    #[test]
    fn blip_embeds_are_found_and_replaced() {
        let data = picture_data("rId3", "a.png", &info(), Length::inches(1.0), Length::inches(1.0));
        let mut pic = picture_element(&data).unwrap().clone();
        assert_eq!(blip_embed(&pic).as_deref(), Some("rId3"));
        assert!(set_blip_embed(&mut pic, "rId8"));
        assert_eq!(blip_embed(&pic).as_deref(), Some("rId8"));
        let parsed: dml_picture::CT_Picture = pic.to_typed().unwrap();
        let c_nv_pr = parsed.nv_pic_pr.unwrap().c_nv_pr.unwrap();
        assert_eq!(c_nv_pr.hlink_click.unwrap().r_id.as_deref(), Some("rId9"));
    }
}
