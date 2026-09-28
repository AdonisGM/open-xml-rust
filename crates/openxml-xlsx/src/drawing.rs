//! Pictures and other objects placed on a worksheet: the drawing part
//! (`xl/drawings/drawingN.xml`, `xdr:wsDr`, ECMA-376 Part 1 §20.5).
//!
//! Objects are positioned by *anchors*: a two-cell anchor follows the cells
//! under the object's corners, a one-cell anchor fixes the top-left corner
//! to a cell and keeps the size, and an absolute anchor ignores cells.
//!
//! ```
//! use openxml_xlsx::{Workbook, Image, Length};
//!
//! let png = openxml_core::image::tiny_png(120, 60);
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.add_image("B2", &png)?;
//! sheet.add_image_with(&png, &Image::at("E2")?.width(Length::cm(3.0)).description("Company logo"))?;
//! let images = sheet.as_view().images()?;
//! assert_eq!(images.len(), 2);
//! assert_eq!(images[1].description.as_deref(), Some("Company logo"));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use std::borrow::Cow;

use openxml_core::{Error, Length, Result, sniff_image};
use openxml_opc::PartName;
use openxml_opc::known::rel_types;
use openxml_schema::{dml, dml_spreadsheet_drawing as xdr, sml};

use crate::cell_ref::{CellRange, CellRef, ToCellRange, ToCellRef};
use crate::parts::SideValue;
use crate::util::{cell_origin_emu, remove_relationship_and_orphans, walk_emu};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use xdr::ST_EditAs as EditAs;

/// A corner of an anchored object: a cell and an offset into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorPoint {
    /// The cell.
    pub cell: CellRef,
    /// Horizontal offset from the cell's left edge.
    pub dx: Length,
    /// Vertical offset from the cell's top edge.
    pub dy: Length,
}

impl AnchorPoint {
    /// The top-left corner of `cell`.
    pub fn at(cell: CellRef) -> Self {
        AnchorPoint {
            cell,
            dx: Length::ZERO,
            dy: Length::ZERO,
        }
    }
}

/// Where an object sits on the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Both corners are tied to cells (`xdr:twoCellAnchor`).
    TwoCell {
        /// Top-left corner.
        from: AnchorPoint,
        /// Bottom-right corner.
        to: AnchorPoint,
        /// How the object follows when cells are moved or resized.
        edit_as: EditAs,
    },
    /// The top-left corner is tied to a cell; the size is fixed (`xdr:oneCellAnchor`).
    OneCell {
        /// Top-left corner.
        from: AnchorPoint,
        /// Width.
        width: Length,
        /// Height.
        height: Length,
    },
    /// A fixed position on the sheet (`xdr:absoluteAnchor`).
    Absolute {
        /// Distance from the left edge of the sheet.
        x: Length,
        /// Distance from the top edge of the sheet.
        y: Length,
        /// Width.
        width: Length,
        /// Height.
        height: Length,
    },
}

/// How a new image is positioned, see [`Image`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// Top-left corner in a cell; the bottom-right corner is computed from
    /// the image size and the column widths/row heights (two-cell anchor).
    Cell {
        /// Top-left cell.
        at: CellRef,
        /// Offset into the cell.
        dx: Length,
        /// Offset into the cell.
        dy: Length,
        /// How the picture follows cell changes (Excel's default: `OneCell`,
        /// "move but don't size with cells").
        edit_as: EditAs,
    },
    /// Top-left corner in a cell, fixed size (one-cell anchor).
    OneCell {
        /// Top-left cell.
        at: CellRef,
        /// Offset into the cell.
        dx: Length,
        /// Offset into the cell.
        dy: Length,
    },
    /// A fixed position (absolute anchor).
    Absolute {
        /// Distance from the left edge of the sheet.
        x: Length,
        /// Distance from the top edge of the sheet.
        y: Length,
    },
    /// Stretched over a range of cells (two-cell anchor); the image size is ignored.
    Range {
        /// The cells covered.
        range: CellRange,
        /// How the picture follows cell changes.
        edit_as: EditAs,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Natural,
    Exact(Length, Length),
    Width(Length),
    Height(Length),
}

/// Options for a new picture: position, size and descriptive text.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    placement: Placement,
    size: Size,
    scale: (f64, f64),
    name: Option<String>,
    description: Option<String>,
    title: Option<String>,
    lock_aspect_ratio: bool,
}

impl Image {
    /// A picture whose top-left corner is in `cell`; it moves with the
    /// cells but keeps its size (Excel's default for inserted pictures).
    pub fn at(cell: impl ToCellRef) -> Result<Self> {
        Ok(Self::with_placement(Placement::Cell {
            at: cell.to_cell_ref()?,
            dx: Length::ZERO,
            dy: Length::ZERO,
            edit_as: EditAs::OneCell,
        }))
    }

    /// A picture tied to one cell with a fixed size (one-cell anchor).
    pub fn one_cell(cell: impl ToCellRef) -> Result<Self> {
        Ok(Self::with_placement(Placement::OneCell {
            at: cell.to_cell_ref()?,
            dx: Length::ZERO,
            dy: Length::ZERO,
        }))
    }

    /// A picture at a fixed position (absolute anchor).
    pub fn absolute(x: Length, y: Length) -> Self {
        Self::with_placement(Placement::Absolute { x, y })
    }

    /// A picture stretched over `range`; it moves and resizes with the cells.
    pub fn over(range: impl ToCellRange) -> Result<Self> {
        Ok(Self::with_placement(Placement::Range {
            range: range.to_cell_range()?,
            edit_as: EditAs::TwoCell,
        }))
    }

    /// A picture with an explicit placement.
    pub fn with_placement(placement: Placement) -> Self {
        Image {
            placement,
            size: Size::Natural,
            scale: (1.0, 1.0),
            name: None,
            description: None,
            title: None,
            lock_aspect_ratio: true,
        }
    }

    /// Offset of the top-left corner into its cell.
    pub fn offset(mut self, dx: Length, dy: Length) -> Self {
        match &mut self.placement {
            Placement::Cell { dx: x, dy: y, .. } | Placement::OneCell { dx: x, dy: y, .. } => {
                *x = dx;
                *y = dy;
            }
            Placement::Absolute { x, y } => {
                *x = *x + dx;
                *y = *y + dy;
            }
            Placement::Range { .. } => {}
        }
        self
    }

    /// How a cell-anchored picture follows cell changes.
    pub fn edit_as(mut self, mode: EditAs) -> Self {
        if let Placement::Cell { edit_as, .. } | Placement::Range { edit_as, .. } = &mut self.placement {
            *edit_as = mode;
        }
        self
    }

    /// Displayed size (the default is the image's natural size).
    pub fn size(mut self, width: Length, height: Length) -> Self {
        self.size = Size::Exact(width, height);
        self
    }

    /// Displayed width; the height keeps the aspect ratio.
    pub fn width(mut self, width: Length) -> Self {
        self.size = Size::Width(width);
        self
    }

    /// Displayed height; the width keeps the aspect ratio.
    pub fn height(mut self, height: Length) -> Self {
        self.size = Size::Height(height);
        self
    }

    /// Scales the size by `factor` (after `size`/`width`/`height`).
    pub fn scale(mut self, factor: f64) -> Self {
        self.scale = (factor, factor);
        self
    }

    /// Scales width and height separately.
    pub fn scale_xy(mut self, x: f64, y: f64) -> Self {
        self.scale = (x, y);
        self
    }

    /// Object name shown in Excel's selection pane (default `Picture N`).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Alternative text (description) for accessibility.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Alternative text title.
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// Whether Excel keeps the aspect ratio when the picture is resized (default true).
    pub fn lock_aspect_ratio(mut self, lock: bool) -> Self {
        self.lock_aspect_ratio = lock;
        self
    }
}

/// A picture found on a worksheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetImage {
    /// Drawing object id (unique within the sheet's drawing), used by
    /// [`WorksheetMut::remove_image`].
    pub id: u32,
    /// Object name.
    pub name: String,
    /// Alternative text.
    pub description: Option<String>,
    /// Alternative text title.
    pub title: Option<String>,
    /// Position.
    pub anchor: Anchor,
    /// The image part (absent for linked pictures).
    pub part: Option<PartName>,
    /// Content type of the image part.
    pub content_type: Option<String>,
}

fn coord(v: &Option<dml::ST_Coordinate>) -> Length {
    match v {
        Some(dml::ST_Coordinate::CoordinateUnqualified(e)) => Length::emu(*e),
        _ => Length::ZERO,
    }
}

fn to_coord(l: Length) -> Option<dml::ST_Coordinate> {
    Some(dml::ST_Coordinate::CoordinateUnqualified(l.as_emu()))
}

fn marker_point(m: &Option<Box<xdr::CT_Marker>>) -> Option<AnchorPoint> {
    let m = m.as_deref()?;
    let cell = CellRef::new(
        u32::try_from(m.row.unwrap_or(0)).ok()? + 1,
        u32::try_from(m.col.unwrap_or(0)).ok()? + 1,
    )
    .ok()?;
    Some(AnchorPoint {
        cell,
        dx: coord(&m.col_off),
        dy: coord(&m.row_off),
    })
}

fn marker(p: &AnchorPoint) -> Box<xdr::CT_Marker> {
    Box::new(xdr::CT_Marker {
        col: Some(p.cell.col() as i32 - 1),
        col_off: to_coord(p.dx),
        row: Some(p.cell.row() as i32 - 1),
        row_off: to_coord(p.dy),
        ..Default::default()
    })
}

fn ext_size(e: &Option<Box<dml::CT_PositiveSize2D>>) -> (Length, Length) {
    e.as_deref().map_or((Length::ZERO, Length::ZERO), |e| {
        (Length::emu(e.cx.unwrap_or(0)), Length::emu(e.cy.unwrap_or(0)))
    })
}

fn positive_size(width: Length, height: Length) -> Box<dml::CT_PositiveSize2D> {
    Box::new(dml::CT_PositiveSize2D {
        cx: Some(width.as_emu().max(0)),
        cy: Some(height.as_emu().max(0)),
        ..Default::default()
    })
}

/// The position of an anchor element.
pub(crate) fn anchor_of(a: &xdr::EG_Anchor) -> Option<Anchor> {
    match a {
        xdr::EG_Anchor::TwoCellAnchor(t) => Some(Anchor::TwoCell {
            from: marker_point(&t.from)?,
            to: marker_point(&t.to)?,
            edit_as: t.edit_as.unwrap_or(EditAs::TwoCell),
        }),
        xdr::EG_Anchor::OneCellAnchor(o) => {
            let (width, height) = ext_size(&o.ext);
            Some(Anchor::OneCell {
                from: marker_point(&o.from)?,
                width,
                height,
            })
        }
        xdr::EG_Anchor::AbsoluteAnchor(a) => {
            let (width, height) = ext_size(&a.ext);
            let pos = a.pos.as_deref();
            Some(Anchor::Absolute {
                x: pos.map_or(Length::ZERO, |p| coord(&p.x)),
                y: pos.map_or(Length::ZERO, |p| coord(&p.y)),
                width,
                height,
            })
        }
        xdr::EG_Anchor::Other(_) => None,
    }
}

/// The object held by an anchor.
pub(crate) enum Content {
    Pic(Box<xdr::CT_Picture>),
    Frame(Box<xdr::CT_GraphicalObjectFrame>),
}

macro_rules! content_choice {
    ($content:expr, $choice:ident) => {
        match $content {
            Content::Pic(p) => xdr::$choice::Pic(p),
            Content::Frame(f) => xdr::$choice::GraphicFrame(f),
        }
    };
}

/// Builds an anchor element holding `content`.
pub(crate) fn build_anchor(anchor: &Anchor, content: Content) -> xdr::EG_Anchor {
    let client_data = Some(Box::default());
    match anchor {
        Anchor::TwoCell { from, to, edit_as } => {
            xdr::EG_Anchor::TwoCellAnchor(Box::new(xdr::CT_TwoCellAnchor {
                edit_as: (*edit_as != EditAs::TwoCell).then_some(*edit_as),
                from: Some(marker(from)),
                to: Some(marker(to)),
                choice: Some(content_choice!(content, CT_TwoCellAnchor_Choice)),
                client_data,
                ..Default::default()
            }))
        }
        Anchor::OneCell { from, width, height } => {
            xdr::EG_Anchor::OneCellAnchor(Box::new(xdr::CT_OneCellAnchor {
                from: Some(marker(from)),
                ext: Some(positive_size(*width, *height)),
                choice: Some(content_choice!(content, CT_OneCellAnchor_Choice)),
                client_data,
                ..Default::default()
            }))
        }
        Anchor::Absolute { x, y, width, height } => {
            xdr::EG_Anchor::AbsoluteAnchor(Box::new(xdr::CT_AbsoluteAnchor {
                pos: Some(Box::new(dml::CT_Point2D {
                    x: to_coord(*x),
                    y: to_coord(*y),
                    ..Default::default()
                })),
                ext: Some(positive_size(*width, *height)),
                choice: Some(content_choice!(content, CT_AbsoluteAnchor_Choice)),
                client_data,
                ..Default::default()
            }))
        }
    }
}

macro_rules! nv_props {
    ($choice:expr, $Enum:ident) => {
        match $choice {
            Some(xdr::$Enum::Pic(p)) => p.nv_pic_pr.as_ref().and_then(|n| n.c_nv_pr.as_deref()),
            Some(xdr::$Enum::GraphicFrame(g)) => {
                g.nv_graphic_frame_pr.as_ref().and_then(|n| n.c_nv_pr.as_deref())
            }
            Some(xdr::$Enum::Sp(s)) => s.nv_sp_pr.as_ref().and_then(|n| n.c_nv_pr.as_deref()),
            Some(xdr::$Enum::CxnSp(c)) => c.nv_cxn_sp_pr.as_ref().and_then(|n| n.c_nv_pr.as_deref()),
            Some(xdr::$Enum::GrpSp(g)) => g.nv_grp_sp_pr.as_ref().and_then(|n| n.c_nv_pr.as_deref()),
            _ => None,
        }
    };
}

/// Non-visual properties (id, name, alt text) of the object of an anchor.
fn anchor_props(a: &xdr::EG_Anchor) -> Option<&dml::CT_NonVisualDrawingProps> {
    match a {
        xdr::EG_Anchor::TwoCellAnchor(t) => nv_props!(&t.choice, CT_TwoCellAnchor_Choice),
        xdr::EG_Anchor::OneCellAnchor(o) => nv_props!(&o.choice, CT_OneCellAnchor_Choice),
        xdr::EG_Anchor::AbsoluteAnchor(a) => nv_props!(&a.choice, CT_AbsoluteAnchor_Choice),
        xdr::EG_Anchor::Other(_) => None,
    }
}

/// The picture of an anchor, if it holds one.
fn anchor_picture(a: &xdr::EG_Anchor) -> Option<&xdr::CT_Picture> {
    match a {
        xdr::EG_Anchor::TwoCellAnchor(t) => match &t.choice {
            Some(xdr::CT_TwoCellAnchor_Choice::Pic(p)) => Some(p),
            _ => None,
        },
        xdr::EG_Anchor::OneCellAnchor(o) => match &o.choice {
            Some(xdr::CT_OneCellAnchor_Choice::Pic(p)) => Some(p),
            _ => None,
        },
        xdr::EG_Anchor::AbsoluteAnchor(a) => match &a.choice {
            Some(xdr::CT_AbsoluteAnchor_Choice::Pic(p)) => Some(p),
            _ => None,
        },
        xdr::EG_Anchor::Other(_) => None,
    }
}

fn picture_embed(p: &xdr::CT_Picture) -> Option<&str> {
    p.blip_fill.as_ref()?.blip.as_ref()?.r_embed.as_deref()
}

/// The next free object id of a drawing.
fn next_object_id(d: &xdr::CT_Drawing) -> u32 {
    d.anchor
        .iter()
        .filter_map(anchor_props)
        .filter_map(|p| p.id)
        .max()
        .unwrap_or(1)
        + 1
}

/// Moves the cell markers of anchors (used when rows or columns are
/// inserted or deleted): `map` receives a 1-based row or column number and
/// returns the new one, `None` when the row or column was deleted.
pub(crate) fn remap_anchor_markers(d: &mut xdr::CT_Drawing, rows: bool, map: &dyn Fn(u32) -> Option<u32>) {
    let limit = if rows {
        crate::cell_ref::MAX_ROW
    } else {
        crate::cell_ref::MAX_COL
    };
    let fix = |m: &mut Option<Box<xdr::CT_Marker>>| {
        let Some(m) = m.as_deref_mut() else { return };
        let (index, offset) = if rows {
            (&mut m.row, &mut m.row_off)
        } else {
            (&mut m.col, &mut m.col_off)
        };
        let Some(i) = index.and_then(|i| u32::try_from(i).ok()) else {
            return;
        };
        let mut n = i + 1;
        loop {
            match map(n) {
                Some(new) => {
                    if n != i + 1 {
                        // The original line was deleted: snap to the next surviving one.
                        *offset = to_coord(Length::ZERO);
                    }
                    *index = Some(new as i32 - 1);
                    break;
                }
                None if n < limit => n += 1,
                None => break,
            }
        }
    };
    for a in &mut d.anchor {
        match a {
            xdr::EG_Anchor::TwoCellAnchor(t) => {
                fix(&mut t.from);
                fix(&mut t.to);
            }
            xdr::EG_Anchor::OneCellAnchor(o) => fix(&mut o.from),
            _ => {}
        }
    }
}

fn drawing_part_of(ws: &sml::CT_Worksheet, pkg: &openxml_opc::Package, sheet: &PartName) -> Option<PartName> {
    let rid = ws.drawing.as_ref()?.r_id.as_deref()?;
    pkg.relationship_target(Some(sheet), rid)
        .filter(|p| pkg.contains(p))
}

impl Worksheet<'_> {
    /// The drawing part of the sheet, if it has one.
    pub fn drawing_part(&self) -> Option<PartName> {
        drawing_part_of(self.data, self.env.package, self.part)
    }

    /// The typed drawing part, if the sheet has one.
    pub fn drawing(&self) -> Result<Option<Cow<'_, xdr::CT_Drawing>>> {
        match self.drawing_part() {
            Some(p) => Ok(Some(self.env.side.peek_drawing(self.env.package, &p)?)),
            None => Ok(None),
        }
    }

    /// Pictures on the sheet, in drawing order.
    pub fn images(&self) -> Result<Vec<SheetImage>> {
        let Some(part) = self.drawing_part() else {
            return Ok(Vec::new());
        };
        let drawing = self.env.side.peek_drawing(self.env.package, &part)?;
        let mut out = Vec::new();
        for a in &drawing.anchor {
            let (Some(pic), Some(anchor)) = (anchor_picture(a), anchor_of(a)) else {
                continue;
            };
            let props = anchor_props(a);
            let image_part = picture_embed(pic)
                .and_then(|rid| self.env.package.relationship_target(Some(&part), rid))
                .filter(|p| self.env.package.contains(p));
            out.push(SheetImage {
                id: props.and_then(|p| p.id).unwrap_or(0),
                name: props.and_then(|p| p.name.clone()).unwrap_or_default(),
                description: props.and_then(|p| p.descr.clone()),
                title: props.and_then(|p| p.title.clone()),
                anchor,
                content_type: image_part
                    .as_ref()
                    .and_then(|p| self.env.package.part(p))
                    .map(|p| p.content_type().to_owned()),
                part: image_part,
            });
        }
        Ok(out)
    }

    /// The bytes of a picture's image part.
    pub fn image_data(&self, image: &SheetImage) -> Option<&[u8]> {
        let part = image.part.as_ref()?;
        Some(self.env.package.part(part)?.data())
    }
}

impl WorksheetMut<'_> {
    /// The drawing part of the sheet, created (empty) if the sheet has none.
    pub fn drawing_part(&mut self) -> Result<PartName> {
        if let Some(p) = drawing_part_of(self.data, self.package, self.part) {
            return Ok(p);
        }
        let name = self.package.next_part_name("/xl/drawings/drawing{}.xml")?;
        self.side
            .create(self.package, &name, SideValue::Drawing(Box::default()))?;
        let rid = self
            .package
            .add_relationship(Some(self.part), rel_types::DRAWING, &name)?;
        self.data.drawing = Some(Box::new(sml::CT_Drawing {
            r_id: Some(rid),
            ..Default::default()
        }));
        Ok(name)
    }

    /// The typed drawing part (created if needed), for objects the API does
    /// not cover. It is rewritten on save.
    pub fn drawing_mut(&mut self) -> Result<&mut xdr::CT_Drawing> {
        let part = self.drawing_part()?;
        self.side.drawing(self.package, &part)
    }

    /// Adds a relationship from the sheet's drawing part to `target` (for
    /// example a chart part) and returns its id, for use in objects added
    /// with [`WorksheetMut::add_graphic_frame`]. An existing relationship of
    /// the same type and target is reused.
    pub fn relate_from_drawing(&mut self, rel_type: &str, target: &PartName) -> Result<String> {
        let drawing = self.drawing_part()?;
        if let Some(rels) = self.package.relationships(Some(&drawing))
            && let Some(r) = rels.by_type(rel_type).find(|r| {
                !r.is_external() && PartName::resolve(Some(&drawing), &r.target).is_ok_and(|t| &t == target)
            })
        {
            return Ok(r.id.clone());
        }
        Ok(self.package.add_relationship(Some(&drawing), rel_type, target)?)
    }

    /// Adds a graphic frame — the container of charts, diagrams and other
    /// graphic objects — holding `graphic` (whose content refers to parts
    /// related with [`WorksheetMut::relate_from_drawing`]). Returns the
    /// object id.
    pub fn add_graphic_frame(
        &mut self,
        anchor: &Anchor,
        name: &str,
        graphic: dml::CT_GraphicalObjectData,
    ) -> Result<u32> {
        let (x, y, width, height) = self.anchor_box(anchor);
        let drawing = self.drawing_mut()?;
        let id = next_object_id(drawing);
        let frame = xdr::CT_GraphicalObjectFrame {
            macro_: Some(String::new()),
            nv_graphic_frame_pr: Some(Box::new(xdr::CT_GraphicalObjectFrameNonVisual {
                c_nv_pr: Some(Box::new(dml::CT_NonVisualDrawingProps {
                    id: Some(id),
                    name: Some(if name.is_empty() {
                        format!("Object {}", id - 1)
                    } else {
                        name.to_owned()
                    }),
                    ..Default::default()
                })),
                c_nv_graphic_frame_pr: Some(Box::default()),
                ..Default::default()
            })),
            xfrm: Some(Box::new(transform(x, y, width, height))),
            graphic: Some(Box::new(dml::CT_GraphicalObject {
                graphic_data: Some(Box::new(graphic)),
                ..Default::default()
            })),
            ..Default::default()
        };
        drawing
            .anchor
            .push(build_anchor(anchor, Content::Frame(Box::new(frame))));
        Ok(id)
    }

    /// Position and size of an anchor in EMU.
    fn anchor_box(&self, anchor: &Anchor) -> (Length, Length, Length, Length) {
        match *anchor {
            Anchor::TwoCell { from, to, .. } => {
                let (x0, y0) = cell_origin_emu(self.data, from.cell);
                let (x1, y1) = cell_origin_emu(self.data, to.cell);
                let x = x0 + from.dx.as_emu();
                let y = y0 + from.dy.as_emu();
                (
                    Length::emu(x),
                    Length::emu(y),
                    Length::emu((x1 + to.dx.as_emu() - x).max(0)),
                    Length::emu((y1 + to.dy.as_emu() - y).max(0)),
                )
            }
            Anchor::OneCell { from, width, height } => {
                let (x0, y0) = cell_origin_emu(self.data, from.cell);
                (
                    Length::emu(x0 + from.dx.as_emu()),
                    Length::emu(y0 + from.dy.as_emu()),
                    width,
                    height,
                )
            }
            Anchor::Absolute { x, y, width, height } => (x, y, width, height),
        }
    }

    /// The anchor of a new picture of the given size.
    fn resolve_placement(&self, placement: &Placement, width: Length, height: Length) -> Anchor {
        match *placement {
            Placement::Cell { at, dx, dy, edit_as } => {
                let (col, cx, row, cy) = walk_emu(
                    self.data,
                    at,
                    dx.as_emu() + width.as_emu(),
                    dy.as_emu() + height.as_emu(),
                );
                let to_cell = CellRef::new(row, col).unwrap_or(at);
                Anchor::TwoCell {
                    from: AnchorPoint { cell: at, dx, dy },
                    to: AnchorPoint {
                        cell: to_cell,
                        dx: Length::emu(cx),
                        dy: Length::emu(cy),
                    },
                    edit_as,
                }
            }
            Placement::OneCell { at, dx, dy } => Anchor::OneCell {
                from: AnchorPoint { cell: at, dx, dy },
                width,
                height,
            },
            Placement::Absolute { x, y } => Anchor::Absolute { x, y, width, height },
            Placement::Range { range, edit_as } => {
                let end = range.end();
                let beyond = CellRef::new(
                    (end.row() + 1).min(crate::cell_ref::MAX_ROW),
                    (end.col() + 1).min(crate::cell_ref::MAX_COL),
                )
                .unwrap_or(end);
                Anchor::TwoCell {
                    from: AnchorPoint::at(range.start()),
                    to: AnchorPoint::at(beyond),
                    edit_as,
                }
            }
        }
    }

    /// Stores image bytes (reusing an identical image part of the package)
    /// and relates the drawing to it. Returns the relationship id.
    fn embed_image(&mut self, data: &[u8], extension: &str, content_type: &str) -> Result<String> {
        let existing = self
            .package
            .parts()
            .find(|(n, p)| n.as_str().starts_with("/xl/media/") && p.data() == data)
            .map(|(n, _)| n.clone());
        let part = match existing {
            Some(p) => p,
            None => {
                let p = self
                    .package
                    .next_part_name(&format!("/xl/media/image{{}}.{extension}"))?;
                self.package.add_part(p.clone(), content_type, data.to_vec())?;
                p
            }
        };
        self.relate_from_drawing(rel_types::IMAGE, &part)
    }

    /// Adds a picture with its top-left corner in cell `at`, at its natural
    /// size (pixels at the image's resolution). Returns the object id.
    pub fn add_image(&mut self, at: impl ToCellRef, data: &[u8]) -> Result<u32> {
        self.add_image_with(data, &Image::at(at)?)
    }

    /// Adds a picture (PNG, JPEG, GIF, BMP, TIFF, EMF or WMF). Identical
    /// image bytes are stored once per package. Returns the object id.
    pub fn add_image_with(&mut self, data: &[u8], image: &Image) -> Result<u32> {
        let info =
            sniff_image(data).ok_or_else(|| Error::InvalidArgument("unrecognised image format".into()))?;
        let (nw, nh) = info.natural_size();
        let ratio = if nw.as_emu() > 0 {
            nh.as_emu() as f64 / nw.as_emu() as f64
        } else {
            1.0
        };
        let (w, h) = match image.size {
            Size::Natural => (nw, nh),
            Size::Exact(w, h) => (w, h),
            Size::Width(w) => (w, Length::emu((w.as_emu() as f64 * ratio).round() as i64)),
            Size::Height(h) => (Length::emu((h.as_emu() as f64 / ratio).round() as i64), h),
        };
        let (w, h) = (
            Length::emu((w.as_emu() as f64 * image.scale.0).round() as i64),
            Length::emu((h.as_emu() as f64 * image.scale.1).round() as i64),
        );
        if w.as_emu() <= 0 || h.as_emu() <= 0 {
            return Err(Error::InvalidArgument("the picture has no size".into()));
        }
        let anchor = self.resolve_placement(&image.placement, w, h);
        let (x, y, bw, bh) = self.anchor_box(&anchor);
        let drawing = self.drawing_part()?;
        let format = info.format;
        let rid = self.embed_image(data, format.extension(), format.content_type())?;
        let d = self.side.drawing(self.package, &drawing)?;
        let id = next_object_id(d);
        let pic = xdr::CT_Picture {
            nv_pic_pr: Some(Box::new(xdr::CT_PictureNonVisual {
                c_nv_pr: Some(Box::new(dml::CT_NonVisualDrawingProps {
                    id: Some(id),
                    name: Some(
                        image
                            .name
                            .clone()
                            .unwrap_or_else(|| format!("Picture {}", id - 1)),
                    ),
                    descr: image.description.clone(),
                    title: image.title.clone(),
                    ..Default::default()
                })),
                c_nv_pic_pr: Some(Box::new(dml::CT_NonVisualPictureProperties {
                    pic_locks: Some(Box::new(dml::CT_PictureLocking {
                        no_change_aspect: image.lock_aspect_ratio.then_some(true),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            blip_fill: Some(Box::new(dml::CT_BlipFillProperties {
                blip: Some(Box::new(dml::CT_Blip {
                    r_embed: Some(rid),
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
                xfrm: Some(Box::new(transform(x, y, bw, bh))),
                geometry: Some(dml::EG_Geometry::PrstGeom(Box::new(dml::CT_PresetGeometry2D {
                    prst: Some(dml::ST_ShapeType::Rect),
                    av_lst: Some(Box::default()),
                    ..Default::default()
                }))),
                ..Default::default()
            })),
            ..Default::default()
        };
        d.anchor.push(build_anchor(&anchor, Content::Pic(Box::new(pic))));
        Ok(id)
    }

    /// Removes the picture with object id `id`. Its image part is removed
    /// when nothing else uses it, and the drawing part when it becomes
    /// empty. Returns whether the picture existed.
    pub fn remove_image(&mut self, id: u32) -> Result<bool> {
        let Some(drawing_name) = drawing_part_of(self.data, self.package, self.part) else {
            return Ok(false);
        };
        let d = self.side.drawing(self.package, &drawing_name)?;
        let Some(pos) = d
            .anchor
            .iter()
            .position(|a| anchor_picture(a).is_some() && anchor_props(a).and_then(|p| p.id) == Some(id))
        else {
            return Ok(false);
        };
        let removed = d.anchor.remove(pos);
        let rid = anchor_picture(&removed)
            .and_then(picture_embed)
            .map(str::to_owned);
        let still_used = rid.as_deref().is_some_and(|rid| {
            d.anchor
                .iter()
                .filter_map(anchor_picture)
                .any(|p| picture_embed(p) == Some(rid))
        });
        let now_empty = d.anchor.is_empty() && d.extra_children.is_empty();
        if let Some(rid) = rid.filter(|_| !still_used) {
            for gone in remove_relationship_and_orphans(self.package, &drawing_name, &rid) {
                self.side.forget(&gone);
            }
        }
        if now_empty {
            let sheet_rid = self.data.drawing.take().and_then(|d| d.r_id).unwrap_or_default();
            for gone in remove_relationship_and_orphans(self.package, self.part, &sheet_rid) {
                self.side.forget(&gone);
            }
        }
        Ok(true)
    }
}

fn transform(x: Length, y: Length, width: Length, height: Length) -> dml::CT_Transform2D {
    dml::CT_Transform2D {
        off: Some(Box::new(dml::CT_Point2D {
            x: to_coord(x),
            y: to_coord(y),
            ..Default::default()
        })),
        ext: Some(positive_size(width, height)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_round_trip_through_elements() {
        let from = AnchorPoint {
            cell: CellRef::parse("B3").unwrap(),
            dx: Length::emu(100),
            dy: Length::emu(200),
        };
        let to = AnchorPoint::at(CellRef::parse("D9").unwrap());
        let anchors = [
            Anchor::TwoCell {
                from,
                to,
                edit_as: EditAs::OneCell,
            },
            Anchor::TwoCell {
                from,
                to,
                edit_as: EditAs::TwoCell,
            },
            Anchor::OneCell {
                from,
                width: Length::emu(5000),
                height: Length::emu(3000),
            },
            Anchor::Absolute {
                x: Length::emu(1),
                y: Length::emu(2),
                width: Length::emu(3),
                height: Length::emu(4),
            },
        ];
        for a in anchors {
            let el = build_anchor(&a, Content::Pic(Box::default()));
            assert_eq!(anchor_of(&el), Some(a));
        }
        let el = build_anchor(&anchors[1], Content::Pic(Box::default()));
        let xdr::EG_Anchor::TwoCellAnchor(t) = &el else {
            panic!("two-cell anchor")
        };
        assert_eq!(t.edit_as, None, "twoCell is the default and is not written");
        assert_eq!(t.from.as_ref().unwrap().col, Some(1), "markers are zero-based");
    }

    #[test]
    fn markers_follow_row_changes() {
        let mut d = xdr::CT_Drawing::default();
        d.anchor.push(build_anchor(
            &Anchor::TwoCell {
                from: AnchorPoint {
                    cell: CellRef::parse("A5").unwrap(),
                    dx: Length::ZERO,
                    dy: Length::emu(10),
                },
                to: AnchorPoint::at(CellRef::parse("C8").unwrap()),
                edit_as: EditAs::OneCell,
            },
            Content::Pic(Box::default()),
        ));
        // Delete rows 4..=5: row 5 is gone, rows after move up by two.
        remap_anchor_markers(&mut d, true, &|r| match r {
            4 | 5 => None,
            r if r > 5 => Some(r - 2),
            r => Some(r),
        });
        let Some(Anchor::TwoCell { from, to, .. }) = anchor_of(&d.anchor[0]) else {
            panic!()
        };
        assert_eq!(from.cell.to_string(), "A4", "snapped to the first surviving row");
        assert_eq!(from.dy, Length::ZERO);
        assert_eq!(to.cell.to_string(), "C6");
    }
}
