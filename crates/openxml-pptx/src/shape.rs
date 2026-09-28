//! Shapes on a slide: inspection, construction and editing.

use openxml_core::{FontSize, Length};
use openxml_schema::{dml, pml};
use openxml_xml::{Ns, RawElement, XmlReader};

use crate::table;
use crate::text::{self, Alignment, Rgb};

/// `graphicData/@uri` of DrawingML tables.
pub const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
/// `graphicData/@uri` of charts.
pub const CHART_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// `graphicData/@uri` of SmartArt diagrams.
pub const DIAGRAM_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
/// `graphicData/@uri` of embedded OLE objects.
pub const OLE_URI: &str = "http://schemas.openxmlformats.org/presentationml/2006/ole";

/// What kind of object a shape is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    /// A preset or custom geometry shape (`p:sp`), including placeholders.
    AutoShape,
    /// A text box (`p:sp` with `txBox="1"`).
    TextBox,
    /// A picture (`p:pic`).
    Picture,
    /// A table (`p:graphicFrame` holding `a:tbl`).
    Table,
    /// A chart (`p:graphicFrame` holding `c:chart`).
    Chart,
    /// A SmartArt diagram.
    Diagram,
    /// An embedded OLE object.
    OleObject,
    /// Another kind of graphic frame.
    GraphicFrame,
    /// A group of shapes (`p:grpSp`).
    Group,
    /// A connector (`p:cxnSp`).
    Connector,
    /// Ink or other content part (`p:contentPart`).
    ContentPart,
    /// Content the schema does not describe.
    Other,
}

/// The role of a placeholder (ECMA-376 Part 1 §19.7.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceholderKind {
    /// Slide title.
    Title,
    /// Centred title (title slides).
    CenteredTitle,
    /// Subtitle.
    Subtitle,
    /// Body text.
    Body,
    /// Generic content (the default placeholder type).
    Object,
    /// Chart.
    Chart,
    /// Table.
    Table,
    /// Clip art.
    ClipArt,
    /// SmartArt diagram.
    Diagram,
    /// Media clip.
    Media,
    /// Slide image (notes pages).
    SlideImage,
    /// Picture.
    Picture,
    /// Date and time.
    Date,
    /// Footer.
    Footer,
    /// Header.
    Header,
    /// Slide number.
    SlideNumber,
}

impl PlaceholderKind {
    pub(crate) fn from_pml(t: Option<pml::ST_PlaceholderType>) -> PlaceholderKind {
        use pml::ST_PlaceholderType as T;
        match t {
            None | Some(T::Obj) => PlaceholderKind::Object,
            Some(T::Title) => PlaceholderKind::Title,
            Some(T::CtrTitle) => PlaceholderKind::CenteredTitle,
            Some(T::SubTitle) => PlaceholderKind::Subtitle,
            Some(T::Body) => PlaceholderKind::Body,
            Some(T::Chart) => PlaceholderKind::Chart,
            Some(T::Tbl) => PlaceholderKind::Table,
            Some(T::ClipArt) => PlaceholderKind::ClipArt,
            Some(T::Dgm) => PlaceholderKind::Diagram,
            Some(T::Media) => PlaceholderKind::Media,
            Some(T::SldImg) => PlaceholderKind::SlideImage,
            Some(T::Pic) => PlaceholderKind::Picture,
            Some(T::Dt) => PlaceholderKind::Date,
            Some(T::Ftr) => PlaceholderKind::Footer,
            Some(T::Hdr) => PlaceholderKind::Header,
            Some(T::SldNum) => PlaceholderKind::SlideNumber,
        }
    }

    /// Whether the placeholder is a title.
    pub fn is_title(self) -> bool {
        matches!(self, PlaceholderKind::Title | PlaceholderKind::CenteredTitle)
    }

    /// Whether the placeholder is one of the footer-area placeholders
    /// (date, footer, header, slide number), which new slides do not copy.
    pub fn is_footer_area(self) -> bool {
        matches!(
            self,
            PlaceholderKind::Date
                | PlaceholderKind::Footer
                | PlaceholderKind::Header
                | PlaceholderKind::SlideNumber
        )
    }

    /// Whether the placeholder holds text.
    pub fn holds_text(self) -> bool {
        !matches!(
            self,
            PlaceholderKind::Chart
                | PlaceholderKind::Table
                | PlaceholderKind::ClipArt
                | PlaceholderKind::Diagram
                | PlaceholderKind::Media
                | PlaceholderKind::SlideImage
                | PlaceholderKind::Picture
        )
    }
}

/// Placeholder information of a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Placeholder {
    /// Role of the placeholder.
    pub kind: PlaceholderKind,
    /// Index linking the placeholder to its layout counterpart.
    pub index: Option<u32>,
}

/// A read-only description of a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeInfo {
    /// Shape identifier (unique within a slide).
    pub id: u32,
    /// Shape name.
    pub name: String,
    /// Kind of object.
    pub kind: ShapeKind,
    /// Placeholder role, for placeholders.
    pub placeholder: Option<Placeholder>,
    /// Position of the top-left corner, when the shape specifies one.
    pub offset: Option<(Length, Length)>,
    /// Width and height, when the shape specifies them.
    pub size: Option<(Length, Length)>,
    /// Text of the shape (paragraphs separated by `\n`, table cells by `\t`).
    pub text: String,
    /// Members of a group.
    pub children: Vec<ShapeInfo>,
}

impl ShapeInfo {
    fn new(kind: ShapeKind) -> Self {
        ShapeInfo {
            id: 0,
            name: String::new(),
            kind,
            placeholder: None,
            offset: None,
            size: None,
            text: String::new(),
            children: Vec::new(),
        }
    }

    /// Visits this shape and all descendants in document order.
    pub fn walk(&self) -> Vec<&ShapeInfo> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.walk());
        }
        out
    }
}

/// Parses a universal measure such as `2.5cm`, `1in` or `12pt`.
pub(crate) fn universal_measure(s: &str) -> Option<Length> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split);
    let v: f64 = num.parse().ok()?;
    Some(match unit {
        "mm" => Length::mm(v),
        "cm" => Length::cm(v),
        "in" => Length::inches(v),
        "pt" => Length::pt(v),
        "pc" | "pi" => Length::pt(v * 12.0),
        _ => return None,
    })
}

pub(crate) fn coordinate(c: &dml::ST_Coordinate) -> Option<Length> {
    match c {
        dml::ST_Coordinate::CoordinateUnqualified(v) => Some(Length::emu(*v)),
        dml::ST_Coordinate::UniversalMeasure(s) => universal_measure(s),
    }
}

pub(crate) fn coord(v: Length) -> dml::ST_Coordinate {
    dml::ST_Coordinate::CoordinateUnqualified(v.as_emu())
}

fn offset_of(off: Option<&dml::CT_Point2D>) -> Option<(Length, Length)> {
    let off = off?;
    Some((coordinate(off.x.as_ref()?)?, coordinate(off.y.as_ref()?)?))
}

fn size_of(ext: Option<&dml::CT_PositiveSize2D>) -> Option<(Length, Length)> {
    let ext = ext?;
    Some((Length::emu(ext.cx?), Length::emu(ext.cy?)))
}

/// A pair of lengths: an offset `(x, y)` or a size `(width, height)`.
type Pair = (Length, Length);

fn xfrm_geometry(x: Option<&dml::CT_Transform2D>) -> (Option<Pair>, Option<Pair>) {
    match x {
        Some(x) => (offset_of(x.off.as_deref()), size_of(x.ext.as_deref())),
        None => (None, None),
    }
}

fn apply_nv(info: &mut ShapeInfo, nv: Option<&dml::CT_NonVisualDrawingProps>) {
    if let Some(nv) = nv {
        info.id = nv.id.unwrap_or(0);
        info.name = nv.name.clone().unwrap_or_default();
    }
}

fn apply_ph(info: &mut ShapeInfo, nv_pr: Option<&pml::CT_ApplicationNonVisualDrawingProps>) {
    if let Some(ph) = nv_pr.and_then(|n| n.ph.as_deref()) {
        info.placeholder = Some(Placeholder {
            kind: PlaceholderKind::from_pml(ph.type_),
            index: ph.idx,
        });
    }
}

/// Placeholder information of a `p:sp`.
pub(crate) fn shape_placeholder(sp: &pml::CT_Shape) -> Option<Placeholder> {
    let ph = sp.nv_sp_pr.as_ref()?.nv_pr.as_ref()?.ph.as_deref()?;
    Some(Placeholder {
        kind: PlaceholderKind::from_pml(ph.type_),
        index: ph.idx,
    })
}

/// Name of a `p:sp`.
pub(crate) fn shape_name(sp: &pml::CT_Shape) -> Option<&str> {
    sp.nv_sp_pr.as_ref()?.c_nv_pr.as_ref()?.name.as_deref()
}

/// Identifier of a `p:sp`.
pub(crate) fn shape_id(sp: &pml::CT_Shape) -> Option<u32> {
    sp.nv_sp_pr.as_ref()?.c_nv_pr.as_ref()?.id
}

/// Reads one shape-tree child from raw XML (used for `mc:AlternateContent`).
fn reparse_choice(raw: &RawElement) -> Option<pml::CT_GroupShape_Choice> {
    let xml = raw.to_xml();
    let mut r = XmlReader::new(&xml);
    let tag = r.root().ok()?;
    pml::CT_GroupShape_Choice::read_choice(&mut r, &tag).ok()
}

/// The shapes offered by an `mc:AlternateContent` element: the fallback when
/// present, otherwise the first choice.
pub(crate) fn alternate_content_shapes(raw: &RawElement) -> Vec<pml::CT_GroupShape_Choice> {
    if !raw.name.is(Ns::MC, "AlternateContent") {
        return Vec::new();
    }
    let branch = raw
        .child(Ns::MC, "Fallback")
        .or_else(|| raw.child(Ns::MC, "Choice"));
    branch
        .map(|b| {
            b.elements()
                .filter(|e| e.name.ns == Ns::P)
                .filter_map(reparse_choice)
                .filter(|c| !matches!(c, pml::CT_GroupShape_Choice::Other(_)))
                .collect()
        })
        .unwrap_or_default()
}

/// Describes one member of a shape tree.
pub(crate) fn describe(choice: &pml::CT_GroupShape_Choice) -> ShapeInfo {
    use pml::CT_GroupShape_Choice as C;
    match choice {
        C::Sp(sp) => {
            let text_box = sp
                .nv_sp_pr
                .as_ref()
                .and_then(|n| n.c_nv_sp_pr.as_ref())
                .and_then(|c| c.tx_box)
                .unwrap_or(false);
            let mut info = ShapeInfo::new(if text_box {
                ShapeKind::TextBox
            } else {
                ShapeKind::AutoShape
            });
            if let Some(nv) = &sp.nv_sp_pr {
                apply_nv(&mut info, nv.c_nv_pr.as_deref());
                apply_ph(&mut info, nv.nv_pr.as_deref());
            }
            (info.offset, info.size) = xfrm_geometry(sp.sp_pr.as_ref().and_then(|p| p.xfrm.as_deref()));
            info.text = sp.tx_body.as_deref().map(text::body_text).unwrap_or_default();
            info
        }
        C::Pic(pic) => {
            let mut info = ShapeInfo::new(ShapeKind::Picture);
            if let Some(nv) = &pic.nv_pic_pr {
                apply_nv(&mut info, nv.c_nv_pr.as_deref());
                apply_ph(&mut info, nv.nv_pr.as_deref());
            }
            (info.offset, info.size) = xfrm_geometry(pic.sp_pr.as_ref().and_then(|p| p.xfrm.as_deref()));
            info
        }
        C::GraphicFrame(frame) => {
            let uri = frame
                .graphic
                .as_ref()
                .and_then(|g| g.graphic_data.as_ref())
                .and_then(|d| d.uri.as_deref());
            let kind = match uri {
                Some(TABLE_URI) => ShapeKind::Table,
                Some(CHART_URI) => ShapeKind::Chart,
                Some(DIAGRAM_URI) => ShapeKind::Diagram,
                Some(OLE_URI) => ShapeKind::OleObject,
                _ => ShapeKind::GraphicFrame,
            };
            let mut info = ShapeInfo::new(kind);
            if let Some(nv) = &frame.nv_graphic_frame_pr {
                apply_nv(&mut info, nv.c_nv_pr.as_deref());
                apply_ph(&mut info, nv.nv_pr.as_deref());
            }
            (info.offset, info.size) = xfrm_geometry(frame.xfrm.as_deref());
            if kind == ShapeKind::Table
                && let Some(t) = table::frame_table(frame)
            {
                info.text = table::table_text(&t);
            }
            info
        }
        C::GrpSp(group) => {
            let mut info = ShapeInfo::new(ShapeKind::Group);
            if let Some(nv) = &group.nv_grp_sp_pr {
                apply_nv(&mut info, nv.c_nv_pr.as_deref());
            }
            if let Some(x) = group.grp_sp_pr.as_ref().and_then(|p| p.xfrm.as_deref()) {
                info.offset = offset_of(x.off.as_deref());
                info.size = size_of(x.ext.as_deref());
            }
            info.children = describe_tree(group);
            info.text = join_texts(&info.children);
            info
        }
        C::CxnSp(cxn) => {
            let mut info = ShapeInfo::new(ShapeKind::Connector);
            if let Some(nv) = &cxn.nv_cxn_sp_pr {
                apply_nv(&mut info, nv.c_nv_pr.as_deref());
            }
            (info.offset, info.size) = xfrm_geometry(cxn.sp_pr.as_ref().and_then(|p| p.xfrm.as_deref()));
            info
        }
        C::ContentPart(_) => ShapeInfo::new(ShapeKind::ContentPart),
        C::Other(raw) => {
            let alternatives = alternate_content_shapes(raw);
            match alternatives.as_slice() {
                [single] => describe(single),
                [] => ShapeInfo::new(ShapeKind::Other),
                many => {
                    let mut info = ShapeInfo::new(ShapeKind::Other);
                    info.children = many.iter().map(describe).collect();
                    info.text = join_texts(&info.children);
                    info
                }
            }
        }
    }
}

fn join_texts(shapes: &[ShapeInfo]) -> String {
    shapes
        .iter()
        .map(|s| s.text.as_str())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Describes the members of a shape tree (or group) in document order.
pub(crate) fn describe_tree(tree: &pml::CT_GroupShape) -> Vec<ShapeInfo> {
    tree.choice.iter().map(describe).collect()
}

/// Text of all shapes of a tree, one shape per line.
pub(crate) fn tree_text(tree: &pml::CT_GroupShape) -> String {
    join_texts(&describe_tree(tree))
}

fn raw_max_id(raw: &RawElement) -> u32 {
    raw.descendants()
        .into_iter()
        .filter(|e| &*e.name.local == "cNvPr")
        .filter_map(|e| e.attr(Ns::NONE, "id")?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// The largest shape identifier used in a shape tree.
pub(crate) fn max_shape_id(tree: &pml::CT_GroupShape) -> u32 {
    use pml::CT_GroupShape_Choice as C;
    let own = tree
        .nv_grp_sp_pr
        .as_ref()
        .and_then(|n| n.c_nv_pr.as_ref())
        .and_then(|c| c.id)
        .unwrap_or(0);
    tree.choice
        .iter()
        .map(|c| match c {
            C::Sp(sp) => shape_id(sp).unwrap_or(0),
            C::Pic(p) => p
                .nv_pic_pr
                .as_ref()
                .and_then(|n| n.c_nv_pr.as_ref())
                .and_then(|c| c.id)
                .unwrap_or(0),
            C::GraphicFrame(f) => f
                .nv_graphic_frame_pr
                .as_ref()
                .and_then(|n| n.c_nv_pr.as_ref())
                .and_then(|c| c.id)
                .unwrap_or(0),
            C::CxnSp(x) => x
                .nv_cxn_sp_pr
                .as_ref()
                .and_then(|n| n.c_nv_pr.as_ref())
                .and_then(|c| c.id)
                .unwrap_or(0),
            C::GrpSp(g) => max_shape_id(g),
            C::ContentPart(_) => 0,
            C::Other(raw) => raw_max_id(raw),
        })
        .max()
        .unwrap_or(0)
        .max(own)
}

/// Non-visual drawing properties with an identifier and a name.
pub(crate) fn nv_props(id: u32, name: &str) -> dml::CT_NonVisualDrawingProps {
    dml::CT_NonVisualDrawingProps {
        id: Some(id),
        name: Some(name.to_owned()),
        ..Default::default()
    }
}

/// A 2-D transform.
pub(crate) fn transform(x: Length, y: Length, w: Length, h: Length) -> dml::CT_Transform2D {
    dml::CT_Transform2D {
        off: Some(Box::new(dml::CT_Point2D {
            x: Some(coord(x)),
            y: Some(coord(y)),
            ..Default::default()
        })),
        ext: Some(Box::new(dml::CT_PositiveSize2D {
            cx: Some(w.as_emu().max(0)),
            cy: Some(h.as_emu().max(0)),
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Preset rectangle geometry.
pub(crate) fn rect_geometry() -> dml::EG_Geometry {
    dml::EG_Geometry::PrstGeom(Box::new(dml::CT_PresetGeometry2D {
        prst: Some(dml::ST_ShapeType::Rect),
        av_lst: Some(Box::default()),
        ..Default::default()
    }))
}

/// A text box shape.
pub(crate) fn new_text_box(id: u32, x: Length, y: Length, w: Length, h: Length, text: &str) -> pml::CT_Shape {
    let mut body = text::text_body(text::paragraphs_from_text(text));
    body.body_pr = Some(Box::new(dml::CT_TextBodyProperties {
        wrap: Some(dml::ST_TextWrappingType::Square),
        rtl_col: Some(false),
        text_autofit: Some(dml::EG_TextAutofit::SpAutoFit(Box::default())),
        ..Default::default()
    }));
    pml::CT_Shape {
        nv_sp_pr: Some(Box::new(pml::CT_ShapeNonVisual {
            c_nv_pr: Some(Box::new(nv_props(
                id,
                &format!("TextBox {}", id.saturating_sub(1)),
            ))),
            c_nv_sp_pr: Some(Box::new(dml::CT_NonVisualDrawingShapeProps {
                tx_box: Some(true),
                ..Default::default()
            })),
            nv_pr: Some(Box::default()),
            ..Default::default()
        })),
        sp_pr: Some(Box::new(dml::CT_ShapeProperties {
            xfrm: Some(Box::new(transform(x, y, w, h))),
            geometry: Some(rect_geometry()),
            fill_properties: Some(dml::EG_FillProperties::NoFill(Box::default())),
            ..Default::default()
        })),
        tx_body: Some(Box::new(body)),
        ..Default::default()
    }
}

/// An empty slide placeholder that inherits everything from `layout_sp`.
pub(crate) fn placeholder_from_layout(layout_sp: &pml::CT_Shape, id: u32) -> Option<pml::CT_Shape> {
    let ph = layout_sp.nv_sp_pr.as_ref()?.nv_pr.as_ref()?.ph.as_deref()?;
    let kind = PlaceholderKind::from_pml(ph.type_);
    let name = shape_name(layout_sp)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Placeholder {}", id - 1));
    Some(pml::CT_Shape {
        nv_sp_pr: Some(Box::new(pml::CT_ShapeNonVisual {
            c_nv_pr: Some(Box::new(nv_props(id, &name))),
            c_nv_sp_pr: Some(Box::new(dml::CT_NonVisualDrawingShapeProps {
                sp_locks: Some(Box::new(dml::CT_ShapeLocking {
                    no_grp: Some(true),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            nv_pr: Some(Box::new(pml::CT_ApplicationNonVisualDrawingProps {
                ph: Some(Box::new(pml::CT_Placeholder {
                    type_: ph.type_,
                    orient: ph.orient,
                    sz: ph.sz,
                    idx: ph.idx,
                    has_custom_prompt: None,
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        })),
        sp_pr: Some(Box::default()),
        tx_body: kind.holds_text().then(|| Box::new(text::text_body(Vec::new()))),
        ..Default::default()
    })
}

/// Mutable access to a `p:sp` shape (a text box or placeholder).
///
/// Formatting methods apply to the text the shape currently holds; call
/// [`ShapeMut::set_text`] first when replacing text.
pub struct ShapeMut<'a> {
    shape: &'a mut pml::CT_Shape,
}

impl<'a> ShapeMut<'a> {
    pub(crate) fn new(shape: &'a mut pml::CT_Shape) -> Self {
        ShapeMut { shape }
    }

    /// Shape identifier.
    pub fn id(&self) -> u32 {
        shape_id(self.shape).unwrap_or(0)
    }

    /// Shape name.
    pub fn name(&self) -> &str {
        shape_name(self.shape).unwrap_or("")
    }

    /// Renames the shape.
    pub fn set_name(&mut self, name: &str) -> &mut Self {
        if let Some(nv) = self.shape.nv_sp_pr.as_mut().and_then(|n| n.c_nv_pr.as_mut()) {
            nv.name = Some(name.to_owned());
        }
        self
    }

    /// Text of the shape.
    pub fn text(&self) -> String {
        self.shape
            .tx_body
            .as_deref()
            .map(text::body_text)
            .unwrap_or_default()
    }

    fn body(&mut self) -> &mut dml::CT_TextBody {
        self.shape
            .tx_body
            .get_or_insert_with(|| Box::new(text::text_body(Vec::new())))
    }

    /// Replaces the text (one paragraph per line), keeping the formatting of the first run.
    pub fn set_text(&mut self, text: &str) -> &mut Self {
        let body = self.body();
        let template = body.p.iter().find_map(|p| {
            p.text_run.iter().find_map(|r| match r {
                dml::EG_TextRun::R(run) => run.r_pr.clone(),
                _ => None,
            })
        });
        text::set_paragraphs(body, text::paragraphs_from_text(text));
        if let Some(props) = template {
            text::for_each_run_props(body, |p| *p = (*props).clone());
        }
        self
    }

    /// Sets the font size of all text.
    pub fn font_size(&mut self, size: FontSize) -> &mut Self {
        text::for_each_run_props(self.body(), |p| p.sz = Some(size.hundredths()));
        self
    }

    /// Sets or clears bold on all text.
    pub fn bold(&mut self, on: bool) -> &mut Self {
        text::for_each_run_props(self.body(), |p| p.b = Some(on));
        self
    }

    /// Sets or clears italic on all text.
    pub fn italic(&mut self, on: bool) -> &mut Self {
        text::for_each_run_props(self.body(), |p| p.i = Some(on));
        self
    }

    /// Sets or clears single underline on all text.
    pub fn underline(&mut self, on: bool) -> &mut Self {
        let u = if on {
            dml::ST_TextUnderlineType::Sng
        } else {
            dml::ST_TextUnderlineType::None
        };
        text::for_each_run_props(self.body(), |p| p.u = Some(u));
        self
    }

    /// Sets the text colour.
    pub fn color(&mut self, color: Rgb) -> &mut Self {
        text::for_each_run_props(self.body(), |p| p.fill_properties = Some(color.solid_fill()));
        self
    }

    /// Sets the Latin typeface of all text.
    pub fn font(&mut self, typeface: &str) -> &mut Self {
        text::for_each_run_props(self.body(), |p| p.latin = Some(Box::new(text::font(typeface))));
        self
    }

    /// Alignment of the first paragraph, when set explicitly.
    pub fn alignment(&self) -> Option<Alignment> {
        let p = self.shape.tx_body.as_ref()?.p.first()?;
        Alignment::from_dml(p.p_pr.as_ref()?.algn?)
    }

    /// Sets the alignment of all paragraphs.
    pub fn align(&mut self, alignment: Alignment) -> &mut Self {
        text::for_each_paragraph_props(self.body(), |p| p.algn = Some(alignment.to_dml()));
        self
    }

    fn sp_pr(&mut self) -> &mut dml::CT_ShapeProperties {
        self.shape.sp_pr.get_or_insert_with(Box::default)
    }

    /// Fills the shape with a solid colour.
    pub fn fill(&mut self, color: Rgb) -> &mut Self {
        self.sp_pr().fill_properties = Some(color.solid_fill());
        self
    }

    /// Draws the shape outline with a solid colour and width.
    pub fn outline(&mut self, color: Rgb, width: Length) -> &mut Self {
        let ln = dml::CT_LineProperties {
            w: Some(width.as_emu().clamp(0, i64::from(i32::MAX)) as i32),
            line_fill_properties: Some(dml::EG_LineFillProperties::SolidFill(Box::new(
                dml::CT_SolidColorFillProperties {
                    color_choice: Some(color.srgb()),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        };
        self.sp_pr().ln = Some(Box::new(ln));
        self
    }

    /// Moves the shape.
    pub fn set_position(&mut self, x: Length, y: Length) -> &mut Self {
        let xfrm = self.sp_pr().xfrm.get_or_insert_with(Box::default);
        xfrm.off = Some(Box::new(dml::CT_Point2D {
            x: Some(coord(x)),
            y: Some(coord(y)),
            ..Default::default()
        }));
        self
    }

    /// Resizes the shape.
    pub fn set_size(&mut self, w: Length, h: Length) -> &mut Self {
        let xfrm = self.sp_pr().xfrm.get_or_insert_with(Box::default);
        xfrm.ext = Some(Box::new(dml::CT_PositiveSize2D {
            cx: Some(w.as_emu().max(0)),
            cy: Some(h.as_emu().max(0)),
            ..Default::default()
        }));
        self
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &pml::CT_Shape {
        self.shape
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut pml::CT_Shape {
        self.shape
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_measures() {
        assert_eq!(universal_measure("1in"), Some(Length::inches(1.0)));
        assert_eq!(universal_measure("2.5cm"), Some(Length::cm(2.5)));
        assert_eq!(universal_measure("10mm"), Some(Length::cm(1.0)));
        assert_eq!(universal_measure("12pt"), Some(Length::pt(12.0)));
        assert_eq!(universal_measure("1pc"), Some(Length::pt(12.0)));
        assert_eq!(universal_measure("1pi"), Some(Length::pt(12.0)));
        assert_eq!(universal_measure("3km"), None);
        assert_eq!(universal_measure("cm"), None);
        assert_eq!(universal_measure("12"), None);
        assert_eq!(
            coordinate(&dml::ST_Coordinate::UniversalMeasure("1in".into())),
            Some(Length::inches(1.0))
        );
        assert_eq!(coordinate(&coord(Length::emu(5))), Some(Length::emu(5)));
    }

    #[test]
    fn placeholder_kinds() {
        assert_eq!(PlaceholderKind::from_pml(None), PlaceholderKind::Object);
        assert!(PlaceholderKind::from_pml(Some(pml::ST_PlaceholderType::CtrTitle)).is_title());
        assert!(PlaceholderKind::SlideNumber.is_footer_area());
        assert!(!PlaceholderKind::Body.is_footer_area());
        assert!(PlaceholderKind::Subtitle.holds_text());
        assert!(!PlaceholderKind::Picture.holds_text());
        for t in pml::ST_PlaceholderType::ALL {
            let _ = PlaceholderKind::from_pml(Some(*t));
        }
    }

    #[test]
    fn text_box_description_and_editing() {
        let mut sp = new_text_box(
            5,
            Length::cm(1.0),
            Length::cm(2.0),
            Length::cm(3.0),
            Length::cm(4.0),
            "a\nb",
        );
        let info = describe(&pml::CT_GroupShape_Choice::Sp(Box::new(sp.clone())));
        assert_eq!(info.kind, ShapeKind::TextBox);
        assert_eq!(info.id, 5);
        assert_eq!(info.name, "TextBox 4");
        assert_eq!(info.offset, Some((Length::cm(1.0), Length::cm(2.0))));
        assert_eq!(info.size, Some((Length::cm(3.0), Length::cm(4.0))));
        assert_eq!(info.text, "a\nb");
        assert_eq!(info.walk().len(), 1);

        let mut m = ShapeMut::new(&mut sp);
        m.font_size(FontSize(20.0))
            .bold(true)
            .italic(true)
            .underline(true)
            .color(Rgb(1, 2, 3));
        assert_eq!(m.alignment(), None);
        m.font("Arial")
            .align(Alignment::Center)
            .fill(Rgb::WHITE)
            .outline(Rgb::BLACK, Length::pt(1.0));
        assert_eq!(m.alignment(), Some(Alignment::Center));
        m.set_position(Length::ZERO, Length::ZERO)
            .set_size(Length::cm(1.0), Length::cm(1.0))
            .set_name("Box");
        assert_eq!(m.name(), "Box");
        assert_eq!(m.id(), 5);
        m.set_text("new text");
        assert_eq!(m.text(), "new text");
        let body = m.raw().tx_body.as_ref().unwrap();
        let dml::EG_TextRun::R(run) = &body.p[0].text_run[0] else {
            panic!()
        };
        let props = run.r_pr.as_ref().unwrap();
        assert_eq!(props.sz, Some(2000), "formatting survives set_text");
        assert_eq!(props.b, Some(true));
        assert!(m.raw_mut().sp_pr.is_some());
    }

    #[test]
    fn placeholders_are_copied_from_layouts() {
        let layout_sp = pml::CT_Shape {
            nv_sp_pr: Some(Box::new(pml::CT_ShapeNonVisual {
                c_nv_pr: Some(Box::new(nv_props(2, "Title 1"))),
                nv_pr: Some(Box::new(pml::CT_ApplicationNonVisualDrawingProps {
                    ph: Some(Box::new(pml::CT_Placeholder {
                        type_: Some(pml::ST_PlaceholderType::Pic),
                        idx: Some(3),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        };
        let copy = placeholder_from_layout(&layout_sp, 7).unwrap();
        assert_eq!(shape_id(&copy), Some(7));
        assert_eq!(shape_name(&copy), Some("Title 1"));
        assert_eq!(
            shape_placeholder(&copy),
            Some(Placeholder {
                kind: PlaceholderKind::Picture,
                index: Some(3)
            })
        );
        assert!(copy.tx_body.is_none(), "picture placeholders hold no text");
        assert!(placeholder_from_layout(&pml::CT_Shape::default(), 2).is_none());
    }

    #[test]
    fn ids_and_alternate_content() {
        let mut tree = pml::CT_GroupShape {
            nv_grp_sp_pr: Some(Box::new(pml::CT_GroupShapeNonVisual {
                c_nv_pr: Some(Box::new(nv_props(1, ""))),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(max_shape_id(&tree), 1);
        tree.choice
            .push(pml::CT_GroupShape_Choice::Sp(Box::new(new_text_box(
                4,
                Length::ZERO,
                Length::ZERO,
                Length::ZERO,
                Length::ZERO,
                "x",
            ))));
        let ac = RawElement::parse(concat!(
            r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
            r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" "#,
            r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
            r#"<mc:Choice Requires="p14"><p:sp><p:nvSpPr><p:cNvPr id="9" name="new"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>choice</a:t></a:r></a:p></p:txBody></p:sp></mc:Choice>"#,
            r#"<mc:Fallback><p:sp><p:nvSpPr><p:cNvPr id="9" name="old"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>"#,
            r#"<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>fallback</a:t></a:r></a:p></p:txBody></p:sp></mc:Fallback>"#,
            r#"</mc:AlternateContent>"#
        ))
        .unwrap();
        tree.choice.push(pml::CT_GroupShape_Choice::Other(Box::new(ac)));
        assert_eq!(max_shape_id(&tree), 9);
        let infos = describe_tree(&tree);
        assert_eq!(infos[1].name, "old");
        assert_eq!(infos[1].text, "fallback");
        assert_eq!(tree_text(&tree), "x\nfallback");
        let other = RawElement::new(Ns::NONE, "unknown");
        assert_eq!(
            describe(&pml::CT_GroupShape_Choice::Other(Box::new(other))).kind,
            ShapeKind::Other
        );
    }
}
