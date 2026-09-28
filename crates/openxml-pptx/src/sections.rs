//! Sections (`p14:sectionLst` in the presentation's extension list).
//!
//! Sections are read and kept consistent when slides are added, copied or
//! removed; they are not created or edited.

use openxml_xml::{RawElement, RawNode};

use crate::presentation::Presentation;

/// URI of the `p:ext` holding the section list.
const SECTIONS_URI: &str = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}";
const P14_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";

/// A section of the presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Name.
    pub name: String,
    /// Slides of the section, as positions in the presentation (0-based).
    pub slides: Vec<usize>,
}

fn is_p14(e: &RawElement, local: &str) -> bool {
    e.name.uri() == P14_NS && &*e.name.local == local
}

fn elements_mut(e: &mut RawElement) -> impl Iterator<Item = &mut RawElement> {
    e.children.iter_mut().filter_map(|c| match c {
        RawNode::Element(e) => Some(e),
        RawNode::Text(_) => None,
    })
}

/// The `p14:sldIdLst` of a section, created when missing.
fn slide_list(section: &mut RawElement) -> &mut RawElement {
    if !section.elements().any(|e| is_p14(e, "sldIdLst")) {
        let list =
            RawElement::parse(&format!(r#"<p14:sldIdLst xmlns:p14="{P14_NS}"/>"#)).expect("well-formed");
        section.children.insert(0, RawNode::Element(list));
    }
    elements_mut(section)
        .find(|e| is_p14(e, "sldIdLst"))
        .expect("just ensured")
}

fn slide_id_element(id: u32) -> RawElement {
    RawElement::parse(&format!(r#"<p14:sldId xmlns:p14="{P14_NS}" id="{id}"/>"#)).expect("well-formed")
}

fn slide_id_of(e: &RawElement) -> Option<u32> {
    e.attr(openxml_xml::Ns::NONE, "id")?.parse().ok()
}

impl Presentation {
    fn section_list(&self) -> Option<&RawElement> {
        self.presentation
            .ext_lst
            .as_ref()?
            .ext
            .iter()
            .filter(|e| e.uri.as_deref() == Some(SECTIONS_URI))
            .flat_map(|e| &e.any)
            .find(|raw| is_p14(raw, "sectionLst"))
    }

    fn section_list_mut(&mut self) -> Option<&mut RawElement> {
        self.presentation
            .ext_lst
            .as_mut()?
            .ext
            .iter_mut()
            .filter(|e| e.uri.as_deref() == Some(SECTIONS_URI))
            .flat_map(|e| e.any.iter_mut())
            .find(|raw| is_p14(raw, "sectionLst"))
    }

    /// The sections of the presentation (empty when it has none).
    pub fn sections(&self) -> Vec<Section> {
        let Some(list) = self.section_list() else {
            return Vec::new();
        };
        list.elements()
            .filter(|e| is_p14(e, "section"))
            .map(|s| Section {
                name: s.attr(openxml_xml::Ns::NONE, "name").unwrap_or("").to_owned(),
                slides: s
                    .elements()
                    .filter(|e| is_p14(e, "sldIdLst"))
                    .flat_map(|l| l.elements())
                    .filter_map(slide_id_of)
                    .filter_map(|id| self.slides.iter().position(|sl| sl.id == id))
                    .collect(),
            })
            .collect()
    }

    /// Removes a slide identifier from the sections.
    pub(crate) fn sections_remove_slide(&mut self, id: u32) {
        let Some(list) = self.section_list_mut() else {
            return;
        };
        for section in elements_mut(list).filter(|e| is_p14(e, "section")) {
            for ids in elements_mut(section).filter(|e| is_p14(e, "sldIdLst")) {
                ids.children
                    .retain(|c| !matches!(c, RawNode::Element(e) if slide_id_of(e) == Some(id)));
            }
        }
    }

    /// Places slide `id` right after slide `after` in its section; `None`
    /// means the start of the first section, and an `after` that no section
    /// lists means the end of the last section.
    pub(crate) fn sections_insert_slide(&mut self, after: Option<u32>, id: u32) {
        let Some(list) = self.section_list_mut() else {
            return;
        };
        let Some(after) = after else {
            if let Some(first) = elements_mut(list).find(|e| is_p14(e, "section")) {
                slide_list(first)
                    .children
                    .insert(0, RawNode::Element(slide_id_element(id)));
            }
            return;
        };
        for section in elements_mut(list).filter(|e| is_p14(e, "section")) {
            let ids = slide_list(section);
            let pos = ids
                .children
                .iter()
                .position(|c| matches!(c, RawNode::Element(e) if slide_id_of(e) == Some(after)));
            if let Some(p) = pos {
                ids.children.insert(p + 1, RawNode::Element(slide_id_element(id)));
                return;
            }
        }
        if let Some(last) = elements_mut(list).filter(|e| is_p14(e, "section")).last() {
            slide_list(last)
                .children
                .push(RawNode::Element(slide_id_element(id)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slide_lists_are_created() {
        let mut section = RawElement::parse(&format!(
            r#"<p14:section xmlns:p14="{P14_NS}" name="A" id="{{X}}"/>"#
        ))
        .unwrap();
        slide_list(&mut section)
            .children
            .push(RawNode::Element(slide_id_element(300)));
        let ids: Vec<u32> = section
            .elements()
            .flat_map(|l| l.elements())
            .filter_map(slide_id_of)
            .collect();
        assert_eq!(ids, vec![300]);
    }
}
