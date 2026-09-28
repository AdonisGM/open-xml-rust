//! Slide layouts.

use std::ops::Deref;

use openxml_core::part::{read_part, write_part};
use openxml_core::{Error, Length, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{PartName, Relationship};
use openxml_schema::{dml, pml};

use crate::drawing::Fill;
use crate::presentation::Presentation;
use crate::shape::{self, Placeholder, PlaceholderKind};
use crate::text;

/// The common slide layouts (ECMA-376 Part 1 §19.7.15, `ST_SlideLayoutType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutKind {
    /// Title slide (`title`).
    Title,
    /// Title and content (`obj`).
    TitleAndContent,
    /// Section header (`secHead`).
    SectionHeader,
    /// Two content columns (`twoObj`).
    TwoContent,
    /// Comparison: two titled content columns (`twoTxTwoObj`).
    Comparison,
    /// Title only (`titleOnly`).
    TitleOnly,
    /// Blank (`blank`).
    Blank,
    /// Content with caption (`objTx`).
    ContentWithCaption,
    /// Picture with caption (`picTx`).
    PictureWithCaption,
}

impl LayoutKind {
    /// The `type` attribute value of layouts of this kind.
    pub(crate) fn layout_type(self) -> pml::ST_SlideLayoutType {
        use pml::ST_SlideLayoutType as T;
        match self {
            LayoutKind::Title => T::Title,
            LayoutKind::TitleAndContent => T::Obj,
            LayoutKind::SectionHeader => T::SecHead,
            LayoutKind::TwoContent => T::TwoObj,
            LayoutKind::Comparison => T::TwoTxTwoObj,
            LayoutKind::TitleOnly => T::TitleOnly,
            LayoutKind::Blank => T::Blank,
            LayoutKind::ContentWithCaption => T::ObjTx,
            LayoutKind::PictureWithCaption => T::PicTx,
        }
    }

    pub(crate) fn from_layout_type(t: pml::ST_SlideLayoutType) -> Option<LayoutKind> {
        use pml::ST_SlideLayoutType as T;
        Some(match t {
            T::Title => LayoutKind::Title,
            T::Obj | T::Tx => LayoutKind::TitleAndContent,
            T::SecHead => LayoutKind::SectionHeader,
            T::TwoObj | T::TwoColTx => LayoutKind::TwoContent,
            T::TwoTxTwoObj => LayoutKind::Comparison,
            T::TitleOnly => LayoutKind::TitleOnly,
            T::Blank => LayoutKind::Blank,
            T::ObjTx => LayoutKind::ContentWithCaption,
            T::PicTx => LayoutKind::PictureWithCaption,
            _ => return None,
        })
    }

    /// The name PowerPoint gives layouts of this kind.
    pub fn default_name(self) -> &'static str {
        match self {
            LayoutKind::Title => "Title Slide",
            LayoutKind::TitleAndContent => "Title and Content",
            LayoutKind::SectionHeader => "Section Header",
            LayoutKind::TwoContent => "Two Content",
            LayoutKind::Comparison => "Comparison",
            LayoutKind::TitleOnly => "Title Only",
            LayoutKind::Blank => "Blank",
            LayoutKind::ContentWithCaption => "Content with Caption",
            LayoutKind::PictureWithCaption => "Picture with Caption",
        }
    }
}

/// Selects a layout: by kind, by name (case-insensitive) or by position in
/// [`crate::Presentation::layouts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutRef<'a> {
    /// First layout of this kind.
    Kind(LayoutKind),
    /// Layout with this name.
    Name(&'a str),
    /// Layout at this position.
    Index(usize),
}

impl From<LayoutKind> for LayoutRef<'_> {
    fn from(k: LayoutKind) -> Self {
        LayoutRef::Kind(k)
    }
}

impl<'a> From<&'a str> for LayoutRef<'a> {
    fn from(name: &'a str) -> Self {
        LayoutRef::Name(name)
    }
}

impl From<usize> for LayoutRef<'_> {
    fn from(i: usize) -> Self {
        LayoutRef::Index(i)
    }
}

/// A slide layout of the presentation.
#[derive(Clone, Debug)]
pub struct Layout {
    pub(crate) part: PartName,
    pub(crate) master: PartName,
    pub(crate) data: pml::CT_SlideLayout,
    pub(crate) dirty: bool,
}

impl Layout {
    /// Name of the layout (`p:cSld/@name`).
    pub fn name(&self) -> &str {
        self.data
            .c_sld
            .as_ref()
            .and_then(|c| c.name.as_deref())
            .unwrap_or("")
    }

    /// Kind of the layout, when it is one of the common kinds.
    pub fn kind(&self) -> Option<LayoutKind> {
        self.data.type_.and_then(LayoutKind::from_layout_type)
    }

    /// Name of the layout part.
    pub fn part_name(&self) -> &PartName {
        &self.part
    }

    /// Name of the slide master the layout belongs to.
    pub fn master_part_name(&self) -> &PartName {
        &self.master
    }

    /// Placeholders defined by the layout.
    pub fn placeholders(&self) -> Vec<Placeholder> {
        self.placeholder_shapes()
            .filter_map(shape::shape_placeholder)
            .collect()
    }

    pub(crate) fn placeholder_shapes(&self) -> impl Iterator<Item = &pml::CT_Shape> {
        self.data
            .c_sld
            .as_ref()
            .and_then(|c| c.sp_tree.as_deref())
            .into_iter()
            .flat_map(|t| t.choice.iter())
            .filter_map(|c| match c {
                pml::CT_GroupShape_Choice::Sp(sp) if shape::shape_placeholder(sp).is_some() => Some(&**sp),
                _ => None,
            })
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &pml::CT_SlideLayout {
        &self.data
    }

    pub(crate) fn matches(&self, position: usize, selector: &LayoutRef<'_>) -> bool {
        match selector {
            LayoutRef::Kind(k) => self.data.type_ == Some(k.layout_type()),
            LayoutRef::Name(n) => self.name().eq_ignore_ascii_case(n),
            LayoutRef::Index(i) => *i == position,
        }
    }
}

/// Mutable access to a slide layout; changes are written when the presentation is saved.
pub struct LayoutMut<'a> {
    pres: &'a mut Presentation,
    index: usize,
}

impl Deref for LayoutMut<'_> {
    type Target = Layout;
    fn deref(&self) -> &Layout {
        &self.pres.layouts[self.index]
    }
}

impl LayoutMut<'_> {
    fn layout(&mut self) -> &mut Layout {
        let l = &mut self.pres.layouts[self.index];
        l.dirty = true;
        l
    }

    fn tree(&mut self) -> &mut pml::CT_GroupShape {
        self.layout()
            .data
            .c_sld
            .get_or_insert_with(Box::default)
            .sp_tree
            .get_or_insert_with(Box::default)
    }

    /// Position of the layout in [`Presentation::layouts`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// Renames the layout.
    pub fn set_name(&mut self, name: &str) -> &mut Self {
        self.layout().data.c_sld.get_or_insert_with(Box::default).name = Some(name.to_owned());
        self
    }

    /// Adds a placeholder of the given kind; slides based on the layout can
    /// then fill it (e.g. [`crate::SlideMut::fill_picture_placeholder`]).
    /// Returns its shape identifier.
    pub fn add_placeholder(
        &mut self,
        kind: PlaceholderKind,
        x: Length,
        y: Length,
        w: Length,
        h: Length,
    ) -> u32 {
        let tree = self.tree();
        let id = shape::max_shape_id(tree) + 1;
        let idx = tree
            .choice
            .iter()
            .filter_map(|c| match c {
                pml::CT_GroupShape_Choice::Sp(sp) => shape::shape_placeholder(sp).and_then(|p| p.index),
                _ => None,
            })
            .max()
            .map_or(1, |i| i + 1);
        let label = match kind {
            PlaceholderKind::Title | PlaceholderKind::CenteredTitle => "Title",
            PlaceholderKind::Subtitle => "Subtitle",
            PlaceholderKind::Picture => "Picture Placeholder",
            PlaceholderKind::Chart => "Chart Placeholder",
            PlaceholderKind::Table => "Table Placeholder",
            PlaceholderKind::Media => "Media Placeholder",
            PlaceholderKind::Date => "Date Placeholder",
            PlaceholderKind::Footer => "Footer Placeholder",
            PlaceholderKind::SlideNumber => "Slide Number Placeholder",
            _ => "Content Placeholder",
        };
        let sp = pml::CT_Shape {
            nv_sp_pr: Some(Box::new(pml::CT_ShapeNonVisual {
                c_nv_pr: Some(Box::new(shape::nv_props(id, &format!("{label} {}", id - 1)))),
                c_nv_sp_pr: Some(Box::new(dml::CT_NonVisualDrawingShapeProps {
                    sp_locks: Some(Box::new(dml::CT_ShapeLocking {
                        no_grp: Some(true),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                nv_pr: Some(Box::new(pml::CT_ApplicationNonVisualDrawingProps {
                    ph: Some(Box::new(pml::CT_Placeholder {
                        type_: kind.to_pml(),
                        idx: (!kind.is_title()).then_some(idx),
                        sz: kind.is_footer_area().then_some(pml::ST_PlaceholderSize::Quarter),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            sp_pr: Some(Box::new(dml::CT_ShapeProperties {
                xfrm: Some(Box::new(shape::transform(x, y, w, h))),
                ..Default::default()
            })),
            tx_body: Some(Box::new(text::text_body(Vec::new()))),
            ..Default::default()
        };
        tree.choice.push(pml::CT_GroupShape_Choice::Sp(Box::new(sp)));
        id
    }

    /// Removes a shape (placeholder or decoration) of the layout.
    pub fn remove_shape(&mut self, shape_id: u32) -> bool {
        let tree = self.tree();
        let before = tree.choice.len();
        tree.choice
            .retain(|c| crate::util::choice_id(c) != Some(shape_id));
        before != tree.choice.len()
    }

    /// Fills the layout background (overriding the master).
    pub fn set_background(&mut self, fill: Fill) -> &mut Self {
        self.layout().data.c_sld.get_or_insert_with(Box::default).bg =
            Some(Box::new(crate::theme::background(&fill)));
        self
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut pml::CT_SlideLayout {
        &mut self.layout().data
    }
}

impl Presentation {
    /// Mutable access to the layout at `index` of [`Presentation::layouts`].
    pub fn layout_mut(&mut self, index: usize) -> Option<LayoutMut<'_>> {
        if index < self.layouts.len() {
            Some(LayoutMut { pres: self, index })
        } else {
            None
        }
    }

    /// Adds a custom layout to the slide master of `based_on`, starting as a
    /// copy of it, and returns it for editing.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, PlaceholderKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// let mut layout = deck.add_layout("Photo", LayoutKind::TitleOnly)?;
    /// layout.add_placeholder(PlaceholderKind::Picture, Length::cm(2.0), Length::cm(4.0), Length::cm(20.0), Length::cm(12.0));
    /// let mut slide = deck.add_slide("Photo")?;
    /// assert!(slide.placeholder_mut(PlaceholderKind::Picture).is_ok());
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn add_layout<'a>(
        &mut self,
        name: &str,
        based_on: impl Into<LayoutRef<'a>>,
    ) -> Result<LayoutMut<'_>> {
        let selector = based_on.into();
        let base = self
            .layouts
            .iter()
            .enumerate()
            .position(|(i, l)| l.matches(i, &selector))
            .ok_or_else(|| Error::NotFound(format!("layout {selector:?}")))?;
        let base_part = self.layouts[base].part.clone();
        let master = self.layouts[base].master.clone();
        let mut data = self.layouts[base].data.clone();
        data.c_sld.get_or_insert_with(Box::default).name = Some(name.to_owned());
        data.type_ = None;
        data.matching_name = None;
        data.preserve = Some(true);
        data.user_drawn = Some(true);

        // A layout id unique among all masters and layouts.
        let mut max_id = crate::template::FIRST_MASTER_ID - 1;
        for m in self
            .presentation
            .sld_master_id_lst
            .iter()
            .flat_map(|l| &l.sld_master_id)
        {
            max_id = max_id.max(m.id.unwrap_or(0));
        }
        for mp in self.master_parts() {
            let md = read_part(&self.package, &mp, &pml::elements::SLD_MASTER)?;
            for l in md.sld_layout_id_lst.iter().flat_map(|l| &l.sld_layout_id) {
                max_id = max_id.max(l.id.unwrap_or(0));
            }
        }
        let layout_id = max_id
            .checked_add(1)
            .ok_or_else(|| Error::InvalidDocument("no layout identifier left".into()))?;

        let part = self
            .package
            .next_part_name("/ppt/slideLayouts/slideLayout{}.xml")?;
        self.package.add_part(
            part.clone(),
            ct::PML_SLIDE_LAYOUT,
            pml::elements::SLD_LAYOUT.to_bytes(&data),
        )?;
        // Same folder as the base layout: relative targets stay valid.
        let rels: Vec<Relationship> = self
            .package
            .relationships(Some(&base_part))
            .map(|r| r.iter().cloned().collect())
            .unwrap_or_default();
        let target_rels = self
            .package
            .relationships_mut(Some(&part))
            .ok_or_else(|| Error::MissingPart(part.to_string()))?;
        for r in rels {
            target_rels.insert(r)?;
        }
        let rid = self
            .package
            .add_relationship(Some(&master), rel_types::SLIDE_LAYOUT, &part)?;
        let mut master_data = read_part(&self.package, &master, &pml::elements::SLD_MASTER)?;
        master_data
            .sld_layout_id_lst
            .get_or_insert_with(Box::default)
            .sld_layout_id
            .push(pml::CT_SlideLayoutIdListEntry {
                id: Some(layout_id),
                r_id: Some(rid),
                ..Default::default()
            });
        write_part(
            &mut self.package,
            &master,
            ct::PML_SLIDE_MASTER,
            &pml::elements::SLD_MASTER,
            &master_data,
        )?;
        self.layouts.push(Layout {
            part,
            master,
            data,
            dirty: false,
        });
        let index = self.layouts.len() - 1;
        Ok(LayoutMut { pres: self, index })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_map_to_layout_types() {
        for k in [
            LayoutKind::Title,
            LayoutKind::TitleAndContent,
            LayoutKind::SectionHeader,
            LayoutKind::TwoContent,
            LayoutKind::Comparison,
            LayoutKind::TitleOnly,
            LayoutKind::Blank,
            LayoutKind::ContentWithCaption,
            LayoutKind::PictureWithCaption,
        ] {
            assert_eq!(LayoutKind::from_layout_type(k.layout_type()), Some(k));
            assert!(!k.default_name().is_empty());
        }
        assert_eq!(LayoutKind::from_layout_type(pml::ST_SlideLayoutType::Cust), None);
    }

    #[test]
    fn selectors() {
        assert_eq!(
            LayoutRef::from(LayoutKind::Blank),
            LayoutRef::Kind(LayoutKind::Blank)
        );
        assert_eq!(LayoutRef::from("Blank"), LayoutRef::Name("Blank"));
        assert_eq!(LayoutRef::from(3usize), LayoutRef::Index(3));
        let layout = Layout {
            part: PartName::new("/ppt/slideLayouts/slideLayout1.xml").unwrap(),
            master: PartName::new("/ppt/slideMasters/slideMaster1.xml").unwrap(),
            data: pml::CT_SlideLayout {
                type_: Some(pml::ST_SlideLayoutType::Blank),
                c_sld: Some(Box::new(pml::CT_CommonSlideData {
                    name: Some("Blank".into()),
                    ..Default::default()
                })),
                ..Default::default()
            },
            dirty: false,
        };
        assert_eq!(layout.name(), "Blank");
        assert_eq!(layout.kind(), Some(LayoutKind::Blank));
        assert!(layout.matches(0, &LayoutRef::Kind(LayoutKind::Blank)));
        assert!(!layout.matches(0, &LayoutRef::Kind(LayoutKind::Title)));
        assert!(layout.matches(0, &LayoutRef::Name("blank")));
        assert!(layout.matches(2, &LayoutRef::Index(2)));
        assert!(!layout.matches(1, &LayoutRef::Index(2)));
        assert!(layout.placeholders().is_empty());
        assert_eq!(layout.part_name().as_str(), "/ppt/slideLayouts/slideLayout1.xml");
        assert_eq!(
            layout.master_part_name().as_str(),
            "/ppt/slideMasters/slideMaster1.xml"
        );
        assert!(layout.raw().c_sld.is_some());
    }
}
