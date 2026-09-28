//! Pictures: cropping, transparency, effects, replacement and placeholders.

use openxml_core::{Error, ImageInfo, Length, Result, sniff_image};
use openxml_opc::known::rel_types;
use openxml_opc::{Package, PartName};
use openxml_schema::{dml, pml};

use crate::drawing::{self, Line, Shadow};
use crate::shape::{self, PlaceholderKind};
use crate::slide::SlideMut;
use crate::util;

/// Adds `image` to the package (once: identical media parts are reused) and
/// relates it to `source`. Returns the relationship id and the image description.
pub(crate) fn relate_image(
    pkg: &mut Package,
    source: &PartName,
    image: &[u8],
) -> Result<(String, ImageInfo)> {
    let info = sniff_image(image).ok_or(Error::UnsupportedImage)?;
    let media = relate_media_part(
        pkg,
        image,
        &format!("/ppt/media/image{{}}.{}", info.format.extension()),
        info.format.content_type(),
    )?;
    let rid = relate(pkg, source, rel_types::IMAGE, &media)?;
    Ok((rid, info))
}

/// The existing `/ppt/media/` part holding exactly `bytes`, or a new one.
pub(crate) fn relate_media_part(
    pkg: &mut Package,
    bytes: &[u8],
    pattern: &str,
    content_type: &str,
) -> Result<PartName> {
    let existing = pkg
        .parts()
        .find(|(name, part)| name.as_str().starts_with("/ppt/media/") && part.data() == bytes)
        .map(|(name, _)| name.clone());
    match existing {
        Some(m) => Ok(m),
        None => {
            let name = pkg.next_part_name(pattern)?;
            pkg.add_part(name.clone(), content_type, bytes.to_vec())?;
            Ok(name)
        }
    }
}

/// The id of a relationship of `rel_type` from `source` to `target`, added when missing.
pub(crate) fn relate(
    pkg: &mut Package,
    source: &PartName,
    rel_type: &str,
    target: &PartName,
) -> Result<String> {
    let existing = pkg.relationships(Some(source)).and_then(|rels| {
        rels.by_type(rel_type)
            .find(|r| {
                !r.is_external() && PartName::resolve(Some(source), &r.target).ok().as_ref() == Some(target)
            })
            .map(|r| r.id.clone())
    });
    match existing {
        Some(id) => Ok(id),
        None => Ok(pkg.add_relationship(Some(source), rel_type, target)?),
    }
}

/// A blip fill stretched over the whole shape.
pub(crate) fn stretched_blip(r_id: String) -> dml::CT_BlipFillProperties {
    dml::CT_BlipFillProperties {
        rot_with_shape: Some(true),
        blip: Some(Box::new(dml::CT_Blip {
            r_embed: Some(r_id),
            ..Default::default()
        })),
        fill_mode_properties: Some(dml::EG_FillModeProperties::Stretch(Box::new(
            dml::CT_StretchInfoProperties {
                fill_rect: Some(Box::default()),
                ..Default::default()
            },
        ))),
        ..Default::default()
    }
}

/// A `p:pic` element.
pub(crate) fn new_picture(
    id: u32,
    name: &str,
    r_id: String,
    xfrm: Option<dml::CT_Transform2D>,
    ph: Option<Box<pml::CT_Placeholder>>,
) -> pml::CT_Picture {
    let mut blip = stretched_blip(r_id);
    blip.rot_with_shape = None;
    pml::CT_Picture {
        nv_pic_pr: Some(Box::new(pml::CT_PictureNonVisual {
            c_nv_pr: Some(Box::new(shape::nv_props(id, name))),
            c_nv_pic_pr: Some(Box::new(dml::CT_NonVisualPictureProperties {
                pic_locks: Some(Box::new(dml::CT_PictureLocking {
                    no_grp: ph.is_some().then_some(true),
                    no_change_aspect: Some(true),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            nv_pr: Some(Box::new(pml::CT_ApplicationNonVisualDrawingProps {
                ph,
                ..Default::default()
            })),
            ..Default::default()
        })),
        blip_fill: Some(Box::new(blip)),
        sp_pr: Some(Box::new(dml::CT_ShapeProperties {
            geometry: xfrm.is_some().then(shape::rect_geometry),
            xfrm: xfrm.map(Box::new),
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Cropping of a picture: the fraction removed from each side (negative values add space).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Crop {
    /// Fraction removed from the left edge.
    pub left: f64,
    /// Fraction removed from the top edge.
    pub top: f64,
    /// Fraction removed from the right edge.
    pub right: f64,
    /// Fraction removed from the bottom edge.
    pub bottom: f64,
}

impl Crop {
    /// The crop that makes an image of `image_aspect` (width / height) fill a
    /// box of `box_aspect` without distortion, removing equal amounts on both sides.
    pub fn to_fill(image_aspect: f64, box_aspect: f64) -> Crop {
        if !(image_aspect > 0.0 && box_aspect > 0.0) {
            return Crop::default();
        }
        if image_aspect > box_aspect {
            let side = (1.0 - box_aspect / image_aspect) / 2.0;
            Crop {
                left: side,
                right: side,
                ..Crop::default()
            }
        } else {
            let side = (1.0 - image_aspect / box_aspect) / 2.0;
            Crop {
                top: side,
                bottom: side,
                ..Crop::default()
            }
        }
    }

    fn to_dml(self) -> Option<dml::CT_RelativeRect> {
        let v = |f: f64| (f.abs() > 1e-9).then(|| drawing::percentage(f));
        let rect = dml::CT_RelativeRect {
            l: v(self.left),
            t: v(self.top),
            r: v(self.right),
            b: v(self.bottom),
            ..Default::default()
        };
        (rect.l.is_some() || rect.t.is_some() || rect.r.is_some() || rect.b.is_some()).then_some(rect)
    }

    fn from_dml(r: &dml::CT_RelativeRect) -> Crop {
        let v =
            |p: &Option<dml::ST_Percentage>| p.as_ref().and_then(drawing::percentage_value).unwrap_or(0.0);
        Crop {
            left: v(&r.l),
            top: v(&r.t),
            right: v(&r.r),
            bottom: v(&r.b),
        }
    }
}

/// Mutable access to a picture (`p:pic`).
pub struct PictureMut<'a> {
    pic: &'a mut pml::CT_Picture,
}

impl<'a> PictureMut<'a> {
    pub(crate) fn new(pic: &'a mut pml::CT_Picture) -> Self {
        PictureMut { pic }
    }

    fn nv(&self) -> Option<&dml::CT_NonVisualDrawingProps> {
        self.pic.nv_pic_pr.as_ref()?.c_nv_pr.as_deref()
    }

    fn nv_mut(&mut self) -> &mut dml::CT_NonVisualDrawingProps {
        self.pic
            .nv_pic_pr
            .get_or_insert_with(Box::default)
            .c_nv_pr
            .get_or_insert_with(Box::default)
    }

    fn blip_fill(&mut self) -> &mut dml::CT_BlipFillProperties {
        self.pic.blip_fill.get_or_insert_with(Box::default)
    }

    fn sp_pr(&mut self) -> &mut dml::CT_ShapeProperties {
        self.pic.sp_pr.get_or_insert_with(Box::default)
    }

    /// Shape identifier.
    pub fn id(&self) -> u32 {
        self.nv().and_then(|n| n.id).unwrap_or(0)
    }

    /// Shape name.
    pub fn name(&self) -> &str {
        self.nv().and_then(|n| n.name.as_deref()).unwrap_or("")
    }

    /// Renames the picture.
    pub fn set_name(&mut self, name: &str) -> &mut Self {
        self.nv_mut().name = Some(name.to_owned());
        self
    }

    /// Relationship id of the embedded image.
    pub fn image_rel_id(&self) -> Option<&str> {
        self.pic.blip_fill.as_ref()?.blip.as_ref()?.r_embed.as_deref()
    }

    /// Crops the picture.
    pub fn set_crop(&mut self, crop: Crop) -> &mut Self {
        self.blip_fill().src_rect = crop.to_dml().map(Box::new);
        self
    }

    /// Current cropping.
    pub fn crop(&self) -> Crop {
        self.pic
            .blip_fill
            .as_ref()
            .and_then(|b| b.src_rect.as_deref())
            .map(Crop::from_dml)
            .unwrap_or_default()
    }

    /// Makes the picture partly transparent (`0.0` opaque … `1.0` invisible).
    pub fn set_transparency(&mut self, transparency: f64) -> &mut Self {
        let blip = self.blip_fill().blip.get_or_insert_with(Box::default);
        blip.choice
            .retain(|c| !matches!(c, dml::CT_Blip_Choice::AlphaModFix(_)));
        let opacity = (1.0 - transparency).clamp(0.0, 1.0);
        if opacity < 1.0 {
            blip.choice.push(dml::CT_Blip_Choice::AlphaModFix(Box::new(
                dml::CT_AlphaModulateFixedEffect {
                    amt: Some(dml::ST_PositivePercentage::PositivePercentageDecimal(
                        (opacity * 100_000.0).round() as i32,
                    )),
                    ..Default::default()
                },
            )));
        }
        self
    }

    /// Transparency (`0.0` opaque … `1.0` invisible).
    pub fn transparency(&self) -> f64 {
        let amt = self
            .pic
            .blip_fill
            .as_ref()
            .and_then(|b| b.blip.as_ref())
            .and_then(|b| {
                b.choice.iter().find_map(|c| match c {
                    dml::CT_Blip_Choice::AlphaModFix(a) => a.amt.clone(),
                    _ => None,
                })
            });
        match amt {
            Some(dml::ST_PositivePercentage::PositivePercentageDecimal(v)) => 1.0 - f64::from(v) / 100_000.0,
            Some(dml::ST_PositivePercentage::PositivePercentage(s)) => {
                1.0 - crate::format::percent_string(&s).unwrap_or(1.0)
            }
            None => 0.0,
        }
    }

    /// Sets the picture border.
    pub fn set_line(&mut self, line: Line) -> &mut Self {
        self.sp_pr().ln = Some(Box::new(line.to_dml()));
        self
    }

    /// The picture border, when set.
    pub fn line_format(&self) -> Option<Line> {
        Some(Line::from_dml(self.pic.sp_pr.as_ref()?.ln.as_deref()?))
    }

    fn effects(&mut self) -> &mut dml::CT_EffectList {
        let sp_pr = self.sp_pr();
        if !matches!(
            sp_pr.effect_properties,
            Some(dml::EG_EffectProperties::EffectLst(_))
        ) {
            sp_pr.effect_properties = Some(dml::EG_EffectProperties::EffectLst(Box::default()));
        }
        match sp_pr.effect_properties.as_mut() {
            Some(dml::EG_EffectProperties::EffectLst(l)) => l,
            _ => unreachable!("just set an effect list"),
        }
    }

    /// Adds or removes an outer shadow.
    pub fn set_shadow(&mut self, shadow: Option<Shadow>) -> &mut Self {
        self.effects().outer_shdw = shadow.map(|s| Box::new(s.to_dml()));
        self
    }

    /// The outer shadow, when set.
    pub fn shadow(&self) -> Option<Shadow> {
        match self.pic.sp_pr.as_ref()?.effect_properties.as_ref()? {
            dml::EG_EffectProperties::EffectLst(l) => l.outer_shdw.as_deref().map(Shadow::from_dml),
            _ => None,
        }
    }

    /// Softens the edges over `radius` (`None` removes the effect).
    pub fn set_soft_edges(&mut self, radius: Option<Length>) -> &mut Self {
        self.effects().soft_edge = radius.map(|r| {
            Box::new(dml::CT_SoftEdgesEffect {
                rad: Some(r.as_emu().max(0)),
                ..Default::default()
            })
        });
        self
    }

    /// Soft edge radius, when set.
    pub fn soft_edges(&self) -> Option<Length> {
        match self.pic.sp_pr.as_ref()?.effect_properties.as_ref()? {
            dml::EG_EffectProperties::EffectLst(l) => l.soft_edge.as_ref()?.rad.map(Length::emu),
            _ => None,
        }
    }

    /// Rotates the picture clockwise, in degrees.
    pub fn set_rotation(&mut self, degrees: f64) -> &mut Self {
        let a = drawing::angle(degrees);
        self.sp_pr().xfrm.get_or_insert_with(Box::default).rot = (a != 0).then_some(a);
        self
    }

    /// Moves and resizes the picture.
    pub fn set_bounds(&mut self, x: Length, y: Length, w: Length, h: Length) -> &mut Self {
        let rot = self
            .pic
            .sp_pr
            .as_ref()
            .and_then(|p| p.xfrm.as_ref())
            .and_then(|x| x.rot);
        let mut xfrm = shape::transform(x, y, w, h);
        xfrm.rot = rot;
        let sp_pr = self.sp_pr();
        sp_pr.xfrm = Some(Box::new(xfrm));
        if sp_pr.geometry.is_none() {
            sp_pr.geometry = Some(shape::rect_geometry());
        }
        self
    }

    /// Sets the alternative text: a short title and a description.
    pub fn set_alt_text(&mut self, title: Option<&str>, description: &str) -> &mut Self {
        let nv = self.nv_mut();
        nv.title = title.map(str::to_owned);
        nv.descr = Some(description.to_owned());
        self
    }

    /// The alternative text description.
    pub fn alt_text(&self) -> Option<&str> {
        self.nv()?.descr.as_deref()
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &pml::CT_Picture {
        self.pic
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut pml::CT_Picture {
        self.pic
    }
}

/// Whether the serialized slide still mentions relationship `rid`.
pub(crate) fn slide_uses_rel(data: &pml::CT_Slide, rid: &str) -> bool {
    pml::elements::SLD.to_xml(data).contains(&format!("\"{rid}\""))
}

impl SlideMut<'_> {
    /// The picture with the given shape identifier (searching groups).
    pub fn picture_mut(&mut self, shape_id: u32) -> Option<PictureMut<'_>> {
        match util::find_mut(self.tree_mut(), shape_id)? {
            pml::CT_GroupShape_Choice::Pic(p) => Some(PictureMut::new(p)),
            _ => None,
        }
    }

    /// Sets the alternative text of any graphic (shape, picture, table, chart,
    /// group, connector) on the slide. Returns whether the shape was found.
    pub fn set_alt_text(&mut self, shape_id: u32, title: Option<&str>, description: &str) -> bool {
        let Some(nv) = util::find_mut(self.tree_mut(), shape_id).and_then(util::c_nv_pr_mut) else {
            return false;
        };
        nv.title = title.map(str::to_owned);
        nv.descr = Some(description.to_owned());
        true
    }

    /// Replaces the image of a picture, keeping its position, size, cropping and effects.
    ///
    /// The previous image part is removed when nothing else uses it.
    pub fn replace_picture(&mut self, shape_id: u32, image: &[u8]) -> Result<()> {
        let old = self
            .picture_mut(shape_id)
            .ok_or_else(|| Error::NotFound(format!("picture {shape_id}")))?
            .image_rel_id()
            .map(str::to_owned);
        let slide_part = self.part.clone();
        let (rid, _) = relate_image(&mut self.pres.package, &slide_part, image)?;
        let mut pic = self.picture_mut(shape_id).expect("checked above");
        pic.blip_fill().blip.get_or_insert_with(Box::default).r_embed = Some(rid.clone());
        if let Some(old) = old.filter(|o| *o != rid) {
            self.drop_relationship_if_unused(&old);
        }
        Ok(())
    }

    /// Removes relationship `rid` of the slide when the slide no longer
    /// refers to it, then removes parts nothing refers to.
    pub(crate) fn drop_relationship_if_unused(&mut self, rid: &str) {
        if slide_uses_rel(&self.pres.slides[self.index].data, rid) {
            return;
        }
        let slide_part = self.part.clone();
        let target = self.pres.package.relationship_target(Some(&slide_part), rid);
        if let Some(rels) = self.pres.package.relationships_mut(Some(&slide_part)) {
            rels.remove(rid);
        }
        if let Some(t) = target
            && !self.pres.is_referenced(&t)
        {
            self.pres.remove_parts_and_orphans(vec![t]);
        }
    }

    /// Removes relationships of the kinds this crate creates (images, media,
    /// hyperlinks, slide jumps, charts) that the slide no longer refers to,
    /// then the parts nothing refers to any more.
    pub(crate) fn prune_relationships(&mut self) {
        const PRUNABLE: &[&str] = &[
            rel_types::IMAGE,
            rel_types::VIDEO,
            rel_types::AUDIO,
            crate::media::MEDIA_REL_TYPE,
            rel_types::HYPERLINK,
            rel_types::SLIDE,
            rel_types::CHART,
        ];
        let slide_part = self.part.clone();
        let xml = pml::elements::SLD.to_xml(&self.pres.slides[self.index].data);
        let unused: Vec<String> = self
            .pres
            .package
            .relationships(Some(&slide_part))
            .map(|rels| {
                rels.iter()
                    .filter(|r| PRUNABLE.contains(&r.rel_type.as_str()))
                    .filter(|r| !xml.contains(&format!("\"{}\"", r.id)))
                    .map(|r| r.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for rid in unused {
            self.drop_relationship_if_unused(&rid);
        }
    }

    /// Fills the shape with a picture (stretched over the shape).
    pub fn set_shape_picture_fill(&mut self, shape_id: u32, image: &[u8]) -> Result<()> {
        if self.shape_by_id(shape_id).is_none() {
            return Err(Error::NotFound(format!("shape {shape_id}")));
        }
        let slide_part = self.part.clone();
        let (rid, _) = relate_image(&mut self.pres.package, &slide_part, image)?;
        self.shape_by_id(shape_id)
            .expect("checked above")
            .set_blip_fill(rid);
        Ok(())
    }

    /// Puts an image into the slide's first picture placeholder (copied from
    /// the layout when the slide does not have it yet). The image is cropped
    /// to fill the placeholder like PowerPoint does. Returns the shape identifier.
    pub fn fill_picture_placeholder(&mut self, image: &[u8]) -> Result<u32> {
        // Make sure the placeholder is on the slide.
        let (id, name, ph, own_xfrm) = {
            let sp = self.placeholder_mut(PlaceholderKind::Picture)?;
            let nv = sp.raw().nv_sp_pr.as_ref();
            let ph = nv
                .and_then(|n| n.nv_pr.as_ref())
                .and_then(|n| n.ph.clone())
                .ok_or_else(|| Error::NotFound("picture placeholder".into()))?;
            (
                sp.id(),
                sp.name().to_owned(),
                ph,
                sp.raw().sp_pr.as_ref().and_then(|p| p.xfrm.as_deref()).cloned(),
            )
        };
        let size = own_xfrm
            .as_ref()
            .and_then(|x| x.ext.as_deref())
            .and_then(|e| Some((e.cx?, e.cy?)))
            .or_else(|| self.layout_placeholder_size(&ph));
        let slide_part = self.part.clone();
        let (rid, info) = relate_image(&mut self.pres.package, &slide_part, image)?;
        let mut pic = new_picture(id, &name, rid, own_xfrm, Some(ph));
        if let Some((cx, cy)) = size.filter(|&(cx, cy)| cx > 0 && cy > 0) {
            let crop = Crop::to_fill(
                f64::from(info.width.max(1)) / f64::from(info.height.max(1)),
                cx as f64 / cy as f64,
            );
            PictureMut::new(&mut pic).set_crop(crop);
        }
        let tree = self.tree_mut();
        let index = tree
            .choice
            .iter()
            .position(|c| util::choice_id(c) == Some(id))
            .expect("placeholder was found");
        tree.choice[index] = pml::CT_GroupShape_Choice::Pic(Box::new(pic));
        Ok(id)
    }

    /// Size of the layout placeholder matching `ph`.
    fn layout_placeholder_size(&self, ph: &pml::CT_Placeholder) -> Option<(i64, i64)> {
        let layout = self
            .pres
            .layouts
            .iter()
            .find(|l| Some(&l.part) == self.layout.as_ref())?;
        layout.placeholder_shapes().find_map(|sp| {
            let lph = sp.nv_sp_pr.as_ref()?.nv_pr.as_ref()?.ph.as_deref()?;
            if lph.idx != ph.idx || lph.type_ != ph.type_ {
                return None;
            }
            let ext = sp.sp_pr.as_ref()?.xfrm.as_ref()?.ext.as_deref()?;
            Some((ext.cx?, ext.cy?))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::Color;
    use crate::text::Rgb;

    #[test]
    fn crops() {
        let c = Crop::to_fill(2.0, 1.0);
        assert!((c.left - 0.25).abs() < 1e-9 && (c.right - 0.25).abs() < 1e-9);
        assert_eq!(c.top, 0.0);
        let c = Crop::to_fill(1.0, 2.0);
        assert!((c.top - 0.25).abs() < 1e-9);
        assert_eq!(Crop::to_fill(0.0, 1.0), Crop::default());
        assert_eq!(Crop::default().to_dml(), None);
        let r = Crop {
            left: 0.1,
            top: -0.05,
            right: 0.0,
            bottom: 0.2,
        };
        assert_eq!(Crop::from_dml(&r.to_dml().unwrap()), r);
    }

    #[test]
    fn picture_editing() {
        let mut pic = new_picture(
            4,
            "Picture 3",
            "rId2".into(),
            Some(shape::transform(
                Length::ZERO,
                Length::ZERO,
                Length::cm(1.0),
                Length::cm(1.0),
            )),
            None,
        );
        let mut m = PictureMut::new(&mut pic);
        assert_eq!((m.id(), m.name()), (4, "Picture 3"));
        assert_eq!(m.image_rel_id(), Some("rId2"));
        m.set_name("Logo");
        assert_eq!(m.name(), "Logo");
        assert_eq!(m.transparency(), 0.0);
        m.set_transparency(0.25);
        assert!((m.transparency() - 0.25).abs() < 1e-9);
        m.set_transparency(0.0);
        assert!(
            m.raw()
                .blip_fill
                .as_ref()
                .unwrap()
                .blip
                .as_ref()
                .unwrap()
                .choice
                .is_empty()
        );
        m.set_crop(Crop {
            left: 0.1,
            ..Crop::default()
        });
        assert!((m.crop().left - 0.1).abs() < 1e-9);
        assert_eq!(m.line_format(), None);
        m.set_line(Line::solid(Rgb::BLACK, Length::pt(1.0)));
        assert_eq!(m.line_format().unwrap().color, Some(Color::Rgb(Rgb::BLACK)));
        m.set_shadow(Some(Shadow::default()))
            .set_soft_edges(Some(Length::pt(5.0)));
        assert!(m.shadow().is_some());
        assert_eq!(m.soft_edges(), Some(Length::pt(5.0)));
        m.set_soft_edges(None).set_rotation(30.0);
        assert_eq!(m.soft_edges(), None);
        assert!(m.shadow().is_some(), "other effects stay");
        m.set_bounds(Length::cm(1.0), Length::cm(2.0), Length::cm(3.0), Length::cm(4.0));
        assert_eq!(
            m.raw().sp_pr.as_ref().unwrap().xfrm.as_ref().unwrap().rot,
            Some(1_800_000)
        );
        m.set_alt_text(Some("t"), "d");
        assert_eq!(m.alt_text(), Some("d"));
        assert!(m.raw_mut().nv_pic_pr.is_some());
    }
}
