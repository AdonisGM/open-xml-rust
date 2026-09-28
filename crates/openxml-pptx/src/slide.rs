//! Slides: read-only views and editing.

use std::ops::Deref;

use openxml_core::{Error, Length, Result, sniff_image};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::{dml, pml};

use crate::presentation::Presentation;
use crate::shape::{self, PlaceholderKind, ShapeInfo, ShapeMut};
use crate::table::{self, TableMut};
use crate::template;
use crate::text::{self, Rgb};

/// A notes page attached to a slide.
#[derive(Clone, Debug)]
pub(crate) struct Notes {
    pub(crate) part: PartName,
    pub(crate) data: pml::CT_NotesSlide,
    pub(crate) dirty: bool,
}

/// A slide of a presentation.
#[derive(Clone, Debug)]
pub struct Slide {
    pub(crate) part: PartName,
    pub(crate) id: u32,
    pub(crate) data: pml::CT_Slide,
    pub(crate) layout: Option<PartName>,
    pub(crate) layout_name: Option<String>,
    pub(crate) notes: Option<Notes>,
    pub(crate) dirty: bool,
}

fn tree_of(c_sld: Option<&pml::CT_CommonSlideData>) -> Option<&pml::CT_GroupShape> {
    c_sld?.sp_tree.as_deref()
}

impl Slide {
    /// Name of the slide part, e.g. `/ppt/slides/slide1.xml`.
    pub fn part_name(&self) -> &PartName {
        &self.part
    }

    /// Slide identifier (`p:sldId/@id`), stable when slides are reordered.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &pml::CT_Slide {
        &self.data
    }

    /// Name of the layout the slide is based on.
    pub fn layout_name(&self) -> Option<&str> {
        self.layout_name.as_deref()
    }

    /// Name of the layout part the slide is based on.
    pub fn layout_part_name(&self) -> Option<&PartName> {
        self.layout.as_ref()
    }

    fn tree(&self) -> Option<&pml::CT_GroupShape> {
        tree_of(self.data.c_sld.as_deref())
    }

    /// The top-level shapes of the slide in document (z-) order; groups list their members.
    pub fn shapes(&self) -> Vec<ShapeInfo> {
        self.tree().map(shape::describe_tree).unwrap_or_default()
    }

    /// Text of the title placeholder, if the slide has one.
    pub fn title(&self) -> Option<String> {
        self.shapes()
            .into_iter()
            .flat_map(|s| s.walk().into_iter().cloned().collect::<Vec<_>>())
            .find(|s| s.placeholder.is_some_and(|p| p.kind.is_title()))
            .map(|s| s.text)
    }

    /// Text of all shapes, one shape per line (table cells separated by tabs).
    pub fn text(&self) -> String {
        self.tree().map(shape::tree_text).unwrap_or_default()
    }

    /// Text of the notes page, if the slide has one.
    pub fn notes_text(&self) -> Option<String> {
        let notes = self.notes.as_ref()?;
        let tree = tree_of(notes.data.c_sld.as_deref())?;
        let body = tree.choice.iter().find_map(|c| match c {
            pml::CT_GroupShape_Choice::Sp(sp)
                if shape::shape_placeholder(sp).is_some_and(|p| p.kind == PlaceholderKind::Body) =>
            {
                Some(sp.tx_body.as_deref().map(text::body_text).unwrap_or_default())
            }
            _ => None,
        });
        Some(body.unwrap_or_default())
    }

    /// Name of the notes part, if the slide has notes.
    pub fn notes_part_name(&self) -> Option<&PartName> {
        self.notes.as_ref().map(|n| &n.part)
    }

    /// Whether the slide is hidden in slide shows.
    pub fn is_hidden(&self) -> bool {
        self.data.show == Some(false)
    }

    /// The solid background colour set on the slide itself, if any.
    pub fn background_color(&self) -> Option<Rgb> {
        let bg = self.data.c_sld.as_ref()?.bg.as_ref()?;
        match bg.background.as_ref()? {
            pml::EG_Background::BgPr(pr) => Rgb::from_fill(pr.fill_properties.as_ref()?),
            _ => None,
        }
    }
}

/// Mutable access to a slide. Obtained from [`Presentation::slide_mut`] or
/// [`Presentation::add_slide`]; dereferences to [`Slide`] for reading.
pub struct SlideMut<'a> {
    pub(crate) pres: &'a mut Presentation,
    pub(crate) index: usize,
}

impl Deref for SlideMut<'_> {
    type Target = Slide;
    fn deref(&self) -> &Slide {
        &self.pres.slides[self.index]
    }
}

fn is_body_like(kind: PlaceholderKind) -> bool {
    matches!(kind, PlaceholderKind::Body | PlaceholderKind::Object)
}

impl SlideMut<'_> {
    fn slide(&mut self) -> &mut Slide {
        let s = &mut self.pres.slides[self.index];
        s.dirty = true;
        s
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut pml::CT_Slide {
        &mut self.slide().data
    }

    fn tree_mut(&mut self) -> &mut pml::CT_GroupShape {
        let c_sld = self.slide().data.c_sld.get_or_insert_with(Box::default);
        c_sld.sp_tree.get_or_insert_with(|| {
            Box::new(pml::CT_GroupShape {
                nv_grp_sp_pr: Some(Box::new(pml::CT_GroupShapeNonVisual {
                    c_nv_pr: Some(Box::new(shape::nv_props(1, ""))),
                    c_nv_grp_sp_pr: Some(Box::default()),
                    nv_pr: Some(Box::default()),
                    ..Default::default()
                })),
                grp_sp_pr: Some(Box::default()),
                ..Default::default()
            })
        })
    }

    fn next_id(&mut self) -> u32 {
        shape::max_shape_id(self.tree_mut()) + 1
    }

    /// Index (in the shape tree) of the first placeholder accepted by `pred`,
    /// preferring the lowest placeholder index.
    fn find_placeholder(&mut self, pred: impl Fn(PlaceholderKind) -> bool) -> Option<usize> {
        let tree = self.tree_mut();
        tree.choice
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                pml::CT_GroupShape_Choice::Sp(sp) => shape::shape_placeholder(sp)
                    .filter(|p| pred(p.kind))
                    .map(|p| (p.index.unwrap_or(0), i)),
                _ => None,
            })
            .min()
            .map(|(_, i)| i)
    }

    /// Returns the placeholder accepted by `pred`, copying it from the layout
    /// if the slide does not have it yet.
    fn placeholder(&mut self, what: &str, pred: impl Fn(PlaceholderKind) -> bool) -> Result<ShapeMut<'_>> {
        let index = match self.find_placeholder(&pred) {
            Some(i) => i,
            None => {
                let layout_part = self.layout.clone();
                let template = layout_part
                    .and_then(|lp| self.pres.layouts.iter().find(|l| l.part == lp))
                    .and_then(|l| {
                        let mut candidates: Vec<&pml::CT_Shape> = l
                            .placeholder_shapes()
                            .filter(|sp| shape::shape_placeholder(sp).is_some_and(|p| pred(p.kind)))
                            .collect();
                        candidates.sort_by_key(|sp| {
                            shape::shape_placeholder(sp).and_then(|p| p.index).unwrap_or(0)
                        });
                        candidates.first().map(|sp| (*sp).clone())
                    })
                    .ok_or_else(|| {
                        Error::NotFound(format!("{what} placeholder on slide {}", self.index + 1))
                    })?;
                let id = self.next_id();
                let sp = shape::placeholder_from_layout(&template, id)
                    .ok_or_else(|| Error::NotFound(format!("{what} placeholder")))?;
                let tree = self.tree_mut();
                tree.choice.push(pml::CT_GroupShape_Choice::Sp(Box::new(sp)));
                tree.choice.len() - 1
            }
        };
        match &mut self.tree_mut().choice[index] {
            pml::CT_GroupShape_Choice::Sp(sp) => Ok(ShapeMut::new(sp)),
            _ => unreachable!("placeholders are p:sp shapes"),
        }
    }

    /// The first placeholder of the given kind (copied from the layout when missing).
    pub fn placeholder_mut(&mut self, kind: PlaceholderKind) -> Result<ShapeMut<'_>> {
        self.placeholder(&format!("{kind:?}"), move |k| k == kind)
    }

    /// Sets the text of the title placeholder.
    pub fn set_title(&mut self, title: &str) -> Result<()> {
        self.placeholder("title", PlaceholderKind::is_title)?
            .set_text(title);
        Ok(())
    }

    /// Sets the text of the subtitle placeholder (title slides).
    pub fn set_subtitle(&mut self, subtitle: &str) -> Result<()> {
        self.placeholder("subtitle", |k| k == PlaceholderKind::Subtitle)?
            .set_text(subtitle);
        Ok(())
    }

    /// Replaces the text of the main body/content placeholder with one
    /// paragraph (bullet) per line.
    pub fn set_body_text<S: AsRef<str>>(&mut self, lines: &[S]) -> Result<()> {
        let items: Vec<(u8, &str)> = lines.iter().map(|l| (0, l.as_ref())).collect();
        self.set_body_levels(&items)
    }

    /// Replaces the body text with paragraphs at explicit outline levels
    /// (0 = top level, up to 8).
    pub fn set_body_levels<S: AsRef<str>>(&mut self, items: &[(u8, S)]) -> Result<()> {
        let paragraphs = items
            .iter()
            .map(|(lvl, t)| text::paragraph(t.as_ref(), *lvl))
            .collect();
        let mut body = self.placeholder("body", is_body_like)?;
        let tx = body
            .raw_mut()
            .tx_body
            .get_or_insert_with(|| Box::new(text::text_body(Vec::new())));
        text::set_paragraphs(tx, paragraphs);
        Ok(())
    }

    /// Adds a text box and returns it for formatting.
    ///
    /// ```
    /// use openxml_core::{FontSize, Length};
    /// use openxml_pptx::{Alignment, LayoutKind, Presentation, Rgb};
    ///
    /// let mut deck = Presentation::new();
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// slide
    ///     .add_text_box(Length::cm(2.0), Length::cm(2.0), Length::cm(10.0), Length::cm(2.0), "Hello")
    ///     .font_size(FontSize(32.0))
    ///     .bold(true)
    ///     .color(Rgb(0x1F, 0x4E, 0x79))
    ///     .align(Alignment::Center);
    /// assert_eq!(slide.text(), "Hello");
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_text_box(&mut self, x: Length, y: Length, w: Length, h: Length, text: &str) -> ShapeMut<'_> {
        let id = self.next_id();
        let tree = self.tree_mut();
        tree.choice
            .push(pml::CT_GroupShape_Choice::Sp(Box::new(shape::new_text_box(
                id, x, y, w, h, text,
            ))));
        match tree.choice.last_mut() {
            Some(pml::CT_GroupShape_Choice::Sp(sp)) => ShapeMut::new(sp),
            _ => unreachable!("just pushed a shape"),
        }
    }

    /// The first `p:sp` shape with the given name.
    pub fn shape_mut(&mut self, name: &str) -> Option<ShapeMut<'_>> {
        let tree = self.tree_mut();
        tree.choice.iter_mut().find_map(|c| match c {
            pml::CT_GroupShape_Choice::Sp(sp) if shape::shape_name(sp) == Some(name) => {
                Some(ShapeMut::new(sp))
            }
            _ => None,
        })
    }

    /// Adds a picture. When `height` is `None` it follows from the image's aspect ratio.
    ///
    /// Identical images are stored once in the package. Returns the shape identifier.
    pub fn add_picture(
        &mut self,
        image: &[u8],
        x: Length,
        y: Length,
        width: Length,
        height: Option<Length>,
    ) -> Result<u32> {
        let info = sniff_image(image).ok_or(Error::UnsupportedImage)?;
        let (w, h) = match height {
            Some(h) => (width, h),
            None => info.size_for_width(width),
        };
        let slide_part = self.part.clone();
        let pkg = &mut self.pres.package;
        let existing = pkg
            .parts()
            .find(|(name, part)| name.as_str().starts_with("/ppt/media/") && part.data() == image)
            .map(|(name, _)| name.clone());
        let media = match existing {
            Some(m) => m,
            None => {
                let name =
                    pkg.next_part_name(&format!("/ppt/media/image{{}}.{}", info.format.extension()))?;
                pkg.add_part(name.clone(), info.format.content_type(), image.to_vec())?;
                name
            }
        };
        let rid = match pkg.relationships(Some(&slide_part)).and_then(|rels| {
            rels.by_type(rel_types::IMAGE)
                .find(|r| PartName::resolve(Some(&slide_part), &r.target).ok().as_ref() == Some(&media))
                .map(|r| r.id.clone())
        }) {
            Some(id) => id,
            None => pkg.add_relationship(Some(&slide_part), rel_types::IMAGE, &media)?,
        };
        let id = self.next_id();
        let pic = pml::CT_Picture {
            nv_pic_pr: Some(Box::new(pml::CT_PictureNonVisual {
                c_nv_pr: Some(Box::new(shape::nv_props(id, &format!("Picture {}", id - 1)))),
                c_nv_pic_pr: Some(Box::new(dml::CT_NonVisualPictureProperties {
                    pic_locks: Some(Box::new(dml::CT_PictureLocking {
                        no_change_aspect: Some(true),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                nv_pr: Some(Box::default()),
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
                xfrm: Some(Box::new(shape::transform(x, y, w, h))),
                geometry: Some(shape::rect_geometry()),
                ..Default::default()
            })),
            ..Default::default()
        };
        self.tree_mut()
            .choice
            .push(pml::CT_GroupShape_Choice::Pic(Box::new(pic)));
        Ok(id)
    }

    /// Adds an empty table with the default table style and returns it for editing.
    pub fn add_table(
        &mut self,
        rows: usize,
        cols: usize,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> Result<TableMut<'_>> {
        let id = self.next_id();
        let frame = table::new_table_frame(id, rows, cols, x, y, w, h)?;
        let tree = self.tree_mut();
        tree.choice
            .push(pml::CT_GroupShape_Choice::GraphicFrame(Box::new(frame)));
        match tree.choice.last_mut() {
            Some(pml::CT_GroupShape_Choice::GraphicFrame(f)) => Ok(TableMut::new(f)),
            _ => unreachable!("just pushed a graphic frame"),
        }
    }

    /// The table in the graphic frame with the given shape identifier.
    pub fn table_mut(&mut self, shape_id: u32) -> Option<TableMut<'_>> {
        self.tree_mut().choice.iter_mut().find_map(|c| match c {
            pml::CT_GroupShape_Choice::GraphicFrame(f)
                if table::frame_table(f).is_some()
                    && f.nv_graphic_frame_pr
                        .as_ref()
                        .and_then(|n| n.c_nv_pr.as_ref())
                        .and_then(|c| c.id)
                        == Some(shape_id) =>
            {
                Some(TableMut::new(f))
            }
            _ => None,
        })
    }

    /// Removes the top-level shape with the given identifier. Returns whether one was removed.
    pub fn remove_shape(&mut self, shape_id: u32) -> bool {
        let tree = self.tree_mut();
        let before = tree.choice.len();
        tree.choice.retain(|c| shape::describe(c).id != shape_id);
        before != tree.choice.len()
    }

    /// Replaces `from` with `to` in the text runs of every shape and table on
    /// the slide. Text split across runs is not matched. Returns the number of replacements.
    pub fn replace_text(&mut self, from: &str, to: &str) -> usize {
        fn walk(tree: &mut pml::CT_GroupShape, from: &str, to: &str) -> usize {
            let mut n = 0;
            for c in &mut tree.choice {
                match c {
                    pml::CT_GroupShape_Choice::Sp(sp) => {
                        if let Some(body) = sp.tx_body.as_deref_mut() {
                            n += text::replace_in_body(body, from, to);
                        }
                    }
                    pml::CT_GroupShape_Choice::GrpSp(g) => n += walk(g, from, to),
                    pml::CT_GroupShape_Choice::GraphicFrame(f) => {
                        if let Some(mut t) = table::frame_table(f) {
                            let mut k = 0;
                            for cell in t.tr.iter_mut().flat_map(|r| r.tc.iter_mut()) {
                                if let Some(body) = cell.tx_body.as_deref_mut() {
                                    k += text::replace_in_body(body, from, to);
                                }
                            }
                            if k > 0 {
                                table::store_table(f, &t);
                                n += k;
                            }
                        }
                    }
                    _ => {}
                }
            }
            n
        }
        walk(self.tree_mut(), from, to)
    }

    /// Fills the slide background with a solid colour.
    pub fn set_background_color(&mut self, color: Rgb) {
        let c_sld = self.slide().data.c_sld.get_or_insert_with(Box::default);
        c_sld.bg = Some(Box::new(pml::CT_Background {
            background: Some(pml::EG_Background::BgPr(Box::new(pml::CT_BackgroundProperties {
                fill_properties: Some(color.solid_fill()),
                effect_properties: Some(dml::EG_EffectProperties::EffectLst(Box::default())),
                ..Default::default()
            }))),
            ..Default::default()
        }));
    }

    /// Hides or shows the slide in slide shows.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.slide().data.show = if hidden { Some(false) } else { None };
    }

    /// Sets the speaker notes (one paragraph per line), creating the notes
    /// page — and the presentation's notes master — when needed.
    pub fn set_notes(&mut self, notes: &str) -> Result<()> {
        let paragraphs = text::paragraphs_from_text(notes);
        if self.pres.slides[self.index].notes.is_none() {
            self.create_notes()?;
        }
        let slide = self.slide();
        let n = slide.notes.as_mut().expect("notes exist");
        n.dirty = true;
        let c_sld = n.data.c_sld.get_or_insert_with(Box::default);
        let tree = c_sld.sp_tree.get_or_insert_with(Box::default);
        let body_index = tree.choice.iter().position(|c| {
            matches!(c, pml::CT_GroupShape_Choice::Sp(sp)
                if shape::shape_placeholder(sp).is_some_and(|p| p.kind == PlaceholderKind::Body))
        });
        let index = match body_index {
            Some(i) => i,
            None => {
                let id = shape::max_shape_id(tree) + 1;
                let layout_like = pml::CT_Shape {
                    nv_sp_pr: Some(Box::new(pml::CT_ShapeNonVisual {
                        c_nv_pr: Some(Box::new(shape::nv_props(id, "Notes Placeholder"))),
                        nv_pr: Some(Box::new(pml::CT_ApplicationNonVisualDrawingProps {
                            ph: Some(Box::new(pml::CT_Placeholder {
                                type_: Some(pml::ST_PlaceholderType::Body),
                                idx: Some(1),
                                ..Default::default()
                            })),
                            ..Default::default()
                        })),
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                let sp = shape::placeholder_from_layout(&layout_like, id).expect("placeholder");
                tree.choice.push(pml::CT_GroupShape_Choice::Sp(Box::new(sp)));
                tree.choice.len() - 1
            }
        };
        if let pml::CT_GroupShape_Choice::Sp(sp) = &mut tree.choice[index] {
            let body = sp
                .tx_body
                .get_or_insert_with(|| Box::new(text::text_body(Vec::new())));
            text::set_paragraphs(body, paragraphs);
        }
        Ok(())
    }

    fn create_notes(&mut self) -> Result<()> {
        let master = self.pres.ensure_notes_master()?;
        let slide_part = self.part.clone();
        let pkg = &mut self.pres.package;
        let part = pkg.next_part_name("/ppt/notesSlides/notesSlide{}.xml")?;
        let xml = template::notes_slide_xml(r#"<a:p><a:endParaRPr lang="en-US"/></a:p>"#);
        let data = pml::elements::NOTES.parse(&xml).map_err(|source| Error::Xml {
            part: part.to_string(),
            source,
        })?;
        pkg.add_part(part.clone(), ct::PML_NOTES_SLIDE, xml.into_bytes())?;
        pkg.add_relationship(Some(&part), rel_types::NOTES_MASTER, &master)?;
        pkg.add_relationship(Some(&part), rel_types::SLIDE, &slide_part)?;
        pkg.add_relationship(Some(&slide_part), rel_types::NOTES_SLIDE, &part)?;
        self.slide().notes = Some(Notes {
            part,
            data,
            dirty: true,
        });
        self.pres.dirty = true;
        Ok(())
    }
}
