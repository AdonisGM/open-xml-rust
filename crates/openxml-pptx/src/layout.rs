//! Slide layouts.

use openxml_opc::PartName;
use openxml_schema::pml;

use crate::shape::{self, Placeholder};

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
