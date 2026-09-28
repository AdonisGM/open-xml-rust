//! The presentation document.

use std::collections::HashSet;
use std::io::{Read, Seek, Write};
use std::path::Path;

use openxml_core::part::{read_part, write_part};
use openxml_core::{Error, Length, Result};
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_opc::{CoreProperties, Package, PartName};
use openxml_schema::{pml, shared_extended_properties as ep};

use crate::layout::{Layout, LayoutRef};
use crate::shape;
use crate::slide::{Notes, Slide, SlideMut};
use crate::template;

/// Smallest and largest slide dimension allowed by `ST_SlideSizeCoordinate`.
const SLIDE_SIZE_RANGE: std::ops::RangeInclusive<i64> = 914_400..=51_206_400;
/// Slide identifiers start at 256 (`ST_SlideId`).
const FIRST_SLIDE_ID: u32 = 256;

/// A PresentationML document (`.pptx`).
///
/// The presentation keeps the whole package in memory. Parts it understands
/// (the presentation part, slides and notes) are parsed into the generated
/// schema types; when saving, only the parts that were modified are written
/// again, so everything else — and every untouched slide — stays
/// byte-for-byte identical.
///
/// ```
/// use openxml_pptx::{LayoutKind, Presentation};
///
/// let mut deck = Presentation::new();
/// let mut slide = deck.add_slide(LayoutKind::TitleAndContent)?;
/// slide.set_title("Quarterly results")?;
/// slide.set_body_text(&["Revenue up 12%", "Costs down 3%"])?;
/// let bytes = deck.to_bytes()?;
///
/// let reopened = Presentation::from_bytes(&bytes)?;
/// assert_eq!(reopened.slide_count(), 1);
/// assert_eq!(reopened.slide(0).unwrap().title().as_deref(), Some("Quarterly results"));
/// # Ok::<(), openxml_core::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Presentation {
    pub(crate) package: Package,
    pub(crate) part: PartName,
    pub(crate) presentation: pml::CT_Presentation,
    pub(crate) slides: Vec<Slide>,
    pub(crate) layouts: Vec<Layout>,
    pub(crate) dirty: bool,
}

impl Default for Presentation {
    fn default() -> Self {
        Self::new()
    }
}

fn xml_error(part: &PartName) -> impl FnOnce(openxml_xml::Error) -> Error + '_ {
    move |source| Error::Xml {
        part: part.to_string(),
        source,
    }
}

impl Presentation {
    /// A new, empty 16:9 presentation with the default master, layouts and theme.
    pub fn new() -> Self {
        let pkg = template::blank_package().expect("the built-in template is valid");
        Self::from_package(pkg).expect("the built-in template is valid")
    }

    /// Opens a presentation from a file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open_path(path)?)
    }

    /// Reads a presentation from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    /// Reads a presentation from a seekable reader.
    pub fn from_reader<R: Read + Seek>(reader: R) -> Result<Self> {
        Self::from_package(Package::open(reader)?)
    }

    /// Interprets an already opened package as a presentation.
    pub fn from_package(package: Package) -> Result<Self> {
        let part = package
            .main_part()
            .ok_or_else(|| Error::InvalidDocument("the package has no main document".into()))?;
        let content_type = package
            .part(&part)
            .ok_or_else(|| Error::MissingPart(part.to_string()))?
            .content_type();
        if !(content_type.contains("presentationml") || content_type.contains("powerpoint")) {
            return Err(Error::InvalidDocument(format!(
                "{part} is not a presentation ({content_type})"
            )));
        }
        let presentation = read_part(&package, &part, &pml::elements::PRESENTATION)?;

        let mut layouts = Vec::new();
        for entry in presentation
            .sld_master_id_lst
            .iter()
            .flat_map(|l| &l.sld_master_id)
        {
            let Some(master) = entry
                .r_id
                .as_deref()
                .and_then(|id| package.relationship_target(Some(&part), id))
            else {
                continue;
            };
            if !package.contains(&master) {
                continue;
            }
            let master_data = read_part(&package, &master, &pml::elements::SLD_MASTER)?;
            for le in master_data
                .sld_layout_id_lst
                .iter()
                .flat_map(|l| &l.sld_layout_id)
            {
                let Some(lp) = le
                    .r_id
                    .as_deref()
                    .and_then(|id| package.relationship_target(Some(&master), id))
                else {
                    continue;
                };
                if !package.contains(&lp) || layouts.iter().any(|l: &Layout| l.part == lp) {
                    continue;
                }
                let data = read_part(&package, &lp, &pml::elements::SLD_LAYOUT)?;
                layouts.push(Layout {
                    part: lp,
                    master: master.clone(),
                    data,
                    dirty: false,
                });
            }
        }

        let mut slides = Vec::new();
        for entry in presentation.sld_id_lst.iter().flat_map(|l| &l.sld_id) {
            let rid = entry.r_id.as_deref().unwrap_or("");
            let slide_part = package.relationship_target(Some(&part), rid).ok_or_else(|| {
                Error::InvalidDocument(format!("slide relationship {rid} does not resolve"))
            })?;
            let data = read_part(&package, &slide_part, &pml::elements::SLD)?;
            let layout = package.related_part(Some(&slide_part), rel_types::SLIDE_LAYOUT);
            let layout_name = layout.as_ref().and_then(|lp| {
                layouts
                    .iter()
                    .find(|l| &l.part == lp)
                    .map(|l| l.name().to_owned())
                    .or_else(|| {
                        read_part(&package, lp, &pml::elements::SLD_LAYOUT)
                            .ok()
                            .and_then(|d| d.c_sld.and_then(|c| c.name))
                    })
            });
            let notes = match package.related_part(Some(&slide_part), rel_types::NOTES_SLIDE) {
                Some(np) if package.contains(&np) => {
                    let data = read_part(&package, &np, &pml::elements::NOTES)?;
                    Some(Notes {
                        part: np,
                        data,
                        dirty: false,
                    })
                }
                _ => None,
            };
            slides.push(Slide {
                part: slide_part,
                id: entry.id.unwrap_or(0),
                data,
                layout,
                layout_name,
                notes,
                dirty: false,
            });
        }
        Ok(Presentation {
            package,
            part,
            presentation,
            slides,
            layouts,
            dirty: false,
        })
    }

    // ----- saving --------------------------------------------------------------

    /// Writes every modified typed part back into the package.
    pub fn flush(&mut self) -> Result<()> {
        for layout in self.layouts.iter_mut().filter(|l| l.dirty) {
            write_part(
                &mut self.package,
                &layout.part,
                ct::PML_SLIDE_LAYOUT,
                &pml::elements::SLD_LAYOUT,
                &layout.data,
            )?;
            layout.dirty = false;
        }
        for slide in &mut self.slides {
            if slide.dirty {
                let ct = self
                    .package
                    .part(&slide.part)
                    .map(|p| p.content_type().to_owned());
                let ct = ct.as_deref().unwrap_or(ct::PML_SLIDE);
                write_part(
                    &mut self.package,
                    &slide.part,
                    ct,
                    &pml::elements::SLD,
                    &slide.data,
                )?;
                slide.dirty = false;
            }
            if let Some(notes) = slide.notes.as_mut().filter(|n| n.dirty) {
                write_part(
                    &mut self.package,
                    &notes.part,
                    ct::PML_NOTES_SLIDE,
                    &pml::elements::NOTES,
                    &notes.data,
                )?;
                notes.dirty = false;
            }
        }
        if self.dirty {
            let ct = self.package.part(&self.part).map(|p| p.content_type().to_owned());
            let ct = ct.as_deref().unwrap_or(ct::PML_PRESENTATION);
            write_part(
                &mut self.package,
                &self.part,
                ct,
                &pml::elements::PRESENTATION,
                &self.presentation,
            )?;
            self.update_app_properties()?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Keeps the slide counts of `docProps/app.xml` in sync (when the part exists).
    fn update_app_properties(&mut self) -> Result<()> {
        let Some(app) = self.package.related_part(None, rel_types::EXTENDED_PROPERTIES) else {
            return Ok(());
        };
        let Some(part) = self.package.part(&app) else {
            return Ok(());
        };
        let Ok(mut props) = ep::elements::PROPERTIES.parse_bytes(part.data()) else {
            return Ok(());
        };
        let count = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        props.slides = Some(count(self.slides.len()));
        props.notes = Some(count(self.slides.iter().filter(|s| s.notes.is_some()).count()));
        props.hidden_slides = Some(count(self.slides.iter().filter(|s| s.is_hidden()).count()));
        write_part(
            &mut self.package,
            &app,
            ct::EXTENDED_PROPERTIES,
            &ep::elements::PROPERTIES,
            &props,
        )
    }

    /// Saves the presentation to a file.
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.flush()?;
        self.package.save_path(path)?;
        Ok(())
    }

    /// Serializes the presentation.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        self.flush()?;
        Ok(self.package.to_bytes()?)
    }

    /// Writes the presentation to a seekable writer.
    pub fn write_to<W: Write + Seek>(&mut self, writer: W) -> Result<W> {
        self.flush()?;
        Ok(self.package.save(writer)?)
    }

    // ----- access --------------------------------------------------------------

    /// The underlying package. Typed parts may be stale until [`Presentation::flush`].
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The underlying package, mutably. Changes to typed parts made here are
    /// overwritten by the typed model when those parts are modified and saved.
    pub fn package_mut(&mut self) -> &mut Package {
        &mut self.package
    }

    /// Name of the presentation part.
    pub fn part_name(&self) -> &PartName {
        &self.part
    }

    /// The presentation part (`p:presentation`).
    pub fn presentation(&self) -> &pml::CT_Presentation {
        &self.presentation
    }

    /// The presentation part, mutably.
    pub fn presentation_mut(&mut self) -> &mut pml::CT_Presentation {
        self.dirty = true;
        &mut self.presentation
    }

    /// Document core properties (title, author, dates…).
    pub fn core_properties(&self) -> Result<CoreProperties> {
        Ok(self.package.core_properties()?)
    }

    /// Replaces the document core properties.
    pub fn set_core_properties(&mut self, props: &CoreProperties) -> Result<()> {
        Ok(self.package.set_core_properties(props)?)
    }

    /// Number of slides.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// The slides in presentation order.
    pub fn slides(&self) -> &[Slide] {
        &self.slides
    }

    /// The slide at `index` (0-based).
    pub fn slide(&self, index: usize) -> Option<&Slide> {
        self.slides.get(index)
    }

    /// Mutable access to the slide at `index` (the slide is written on save).
    pub fn slide_mut(&mut self, index: usize) -> Option<SlideMut<'_>> {
        (index < self.slides.len()).then(|| {
            self.slides[index].dirty = true;
            SlideMut { pres: self, index }
        })
    }

    /// The slide layouts of all slide masters.
    pub fn layouts(&self) -> &[Layout] {
        &self.layouts
    }

    /// Text of all slides, separated by blank lines.
    pub fn text(&self) -> String {
        self.slides
            .iter()
            .map(Slide::text)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Slide width and height.
    pub fn slide_size(&self) -> (Length, Length) {
        let sz = self.presentation.sld_sz.as_deref();
        let w = sz.and_then(|s| s.cx).map_or(template::SLIDE_WIDTH, i64::from);
        let h = sz.and_then(|s| s.cy).map_or(template::SLIDE_HEIGHT, i64::from);
        (Length::emu(w), Length::emu(h))
    }

    /// Changes the slide size (shapes are not rescaled). Each dimension must be
    /// between 1 and 56 inches.
    pub fn set_slide_size(&mut self, width: Length, height: Length) -> Result<()> {
        for v in [width, height] {
            if !SLIDE_SIZE_RANGE.contains(&v.as_emu()) {
                return Err(Error::InvalidArgument(format!(
                    "slide dimension {} EMU is out of range",
                    v.as_emu()
                )));
            }
        }
        let kind = match (width.as_emu(), height.as_emu()) {
            (12_192_000, 6_858_000) => None,
            (9_144_000, 6_858_000) => Some(pml::ST_SlideSizeType::Screen4x3),
            (9_144_000, 5_143_500) => Some(pml::ST_SlideSizeType::Screen16x9),
            (9_144_000, 5_715_000) => Some(pml::ST_SlideSizeType::Screen16x10),
            _ => Some(pml::ST_SlideSizeType::Custom),
        };
        self.presentation.sld_sz = Some(Box::new(pml::CT_SlideSize {
            cx: Some(width.as_emu() as i32),
            cy: Some(height.as_emu() as i32),
            type_: kind,
            ..Default::default()
        }));
        self.dirty = true;
        Ok(())
    }

    // ----- slide management ----------------------------------------------------

    fn find_layout(&self, selector: LayoutRef<'_>) -> Result<usize> {
        self.layouts
            .iter()
            .enumerate()
            .position(|(i, l)| l.matches(i, &selector))
            .ok_or_else(|| Error::NotFound(format!("layout {selector:?}")))
    }

    /// Adds a slide at the end, based on a layout selected by kind, name or index.
    ///
    /// The layout's placeholders (except date, footer and slide number) are
    /// copied as empty placeholders, ready for [`SlideMut::set_title`] and friends.
    pub fn add_slide<'a>(&mut self, layout: impl Into<LayoutRef<'a>>) -> Result<SlideMut<'_>> {
        let li = self.find_layout(layout.into())?;
        let layout = &self.layouts[li];
        let part = self.package.next_part_name("/ppt/slides/slide{}.xml")?;
        let mut data = pml::elements::SLD
            .parse(&template::empty_slide_xml())
            .map_err(xml_error(&part))?;
        let tree = data
            .c_sld
            .as_mut()
            .and_then(|c| c.sp_tree.as_mut())
            .expect("template has a shape tree");
        for sp in layout.placeholder_shapes() {
            if shape::shape_placeholder(sp).is_some_and(|p| p.kind.is_footer_area()) {
                continue;
            }
            let id = shape::max_shape_id(tree) + 1;
            if let Some(copy) = shape::placeholder_from_layout(sp, id) {
                tree.choice.push(pml::CT_GroupShape_Choice::Sp(Box::new(copy)));
            }
        }
        let layout_part = layout.part.clone();
        let layout_name = layout.name().to_owned();
        self.package
            .add_part(part.clone(), ct::PML_SLIDE, pml::elements::SLD.to_bytes(&data))?;
        self.package
            .add_relationship(Some(&part), rel_types::SLIDE_LAYOUT, &layout_part)?;
        let rid = self
            .package
            .add_relationship(Some(&self.part), rel_types::SLIDE, &part)?;
        let id = self.slides.iter().map(|s| s.id + 1).chain(
            self.presentation
                .sld_id_lst
                .iter()
                .flat_map(|l| &l.sld_id)
                .filter_map(|e| e.id)
                .map(|i| i + 1),
        );
        let id = id.max().unwrap_or(FIRST_SLIDE_ID).max(FIRST_SLIDE_ID);
        self.presentation
            .sld_id_lst
            .get_or_insert_with(Box::default)
            .sld_id
            .push(pml::CT_SlideIdListEntry {
                id: Some(id),
                r_id: Some(rid),
                ..Default::default()
            });
        self.slides.push(Slide {
            part,
            id,
            data,
            layout: Some(layout_part),
            layout_name: Some(layout_name),
            notes: None,
            dirty: true,
        });
        let previous = self.slides.len().checked_sub(2).map(|i| self.slides[i].id);
        self.sections_insert_slide(previous, id);
        self.dirty = true;
        let index = self.slides.len() - 1;
        Ok(SlideMut { pres: self, index })
    }

    /// Removes the slide at `index` together with its notes page and every
    /// part (images, charts, …) that nothing else refers to any more.
    pub fn remove_slide(&mut self, index: usize) -> Result<()> {
        if index >= self.slides.len() {
            return Err(Error::NotFound(format!("slide {index}")));
        }
        let slide = self.slides.remove(index);
        if let Some(list) = self.presentation.sld_id_lst.as_mut() {
            list.sld_id.retain(|e| e.id != Some(slide.id));
            if list.sld_id.is_empty() {
                self.presentation.sld_id_lst = None;
            }
        }
        let mut removed = vec![slide.part.clone()];
        if let Some(notes) = &slide.notes {
            removed.push(notes.part.clone());
        }
        self.remove_parts_and_orphans(removed);
        self.sections_remove_slide(slide.id);
        self.scrub_dangling_links();
        self.remove_from_custom_shows();
        self.dirty = true;
        Ok(())
    }

    /// Removes parts, then every part that is no longer the target of any relationship.
    pub(crate) fn remove_parts_and_orphans(&mut self, parts: Vec<PartName>) {
        let explicit: HashSet<PartName> = parts.iter().cloned().collect();
        let mut queue = parts;
        while let Some(name) = queue.pop() {
            // A part that is still referenced may become an orphan later, when the
            // last part referring to it is removed; it is then queued again.
            if !self.package.contains(&name) || (!explicit.contains(&name) && self.is_referenced(&name)) {
                continue;
            }
            let targets: Vec<PartName> = self
                .package
                .relationships(Some(&name))
                .map(|rels| {
                    rels.iter()
                        .filter(|r| !r.is_external())
                        .filter_map(|r| PartName::resolve(Some(&name), &r.target).ok())
                        .collect()
                })
                .unwrap_or_default();
            if self.package.remove_part(&name).is_some() {
                queue.extend(targets);
            }
        }
    }

    pub(crate) fn is_referenced(&self, target: &PartName) -> bool {
        let resolves = |source: Option<&PartName>, rels: &openxml_opc::Relationships| {
            rels.iter()
                .filter(|r| !r.is_external())
                .any(|r| PartName::resolve(source, &r.target).is_ok_and(|t| &t == target))
        };
        resolves(None, self.package.package_relationships())
            || self
                .package
                .parts()
                .any(|(name, part)| name != target && resolves(Some(name), part.relationships()))
    }

    /// Moves the slide at `from` so that it ends up at position `to`.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<()> {
        let n = self.slides.len();
        if from >= n || to >= n {
            return Err(Error::NotFound(format!(
                "slide position {} (have {n})",
                from.max(to)
            )));
        }
        let slide = self.slides.remove(from);
        let id = slide.id;
        self.slides.insert(to, slide);
        self.sections_remove_slide(id);
        let previous = to.checked_sub(1).map(|i| self.slides[i].id);
        self.sections_insert_slide(previous, id);
        if let Some(list) = self.presentation.sld_id_lst.as_mut() {
            let order: Vec<u32> = self.slides.iter().map(|s| s.id).collect();
            list.sld_id.sort_by_key(|e| {
                e.id.and_then(|id| order.iter().position(|&o| o == id))
                    .unwrap_or(usize::MAX)
            });
        }
        self.dirty = true;
        Ok(())
    }

    // ----- notes master --------------------------------------------------------

    /// The notes master part, creating it (with its own theme) when the
    /// presentation has none.
    pub(crate) fn ensure_notes_master(&mut self) -> Result<PartName> {
        let existing = self
            .presentation
            .notes_master_id_lst
            .as_ref()
            .and_then(|l| l.notes_master_id.as_ref())
            .and_then(|e| e.r_id.as_deref())
            .and_then(|id| self.package.relationship_target(Some(&self.part), id))
            .or_else(|| {
                self.package
                    .related_part(Some(&self.part), rel_types::NOTES_MASTER)
            })
            .filter(|p| self.package.contains(p));
        if let Some(p) = existing {
            return Ok(p);
        }
        let master = self
            .package
            .next_part_name("/ppt/notesMasters/notesMaster{}.xml")?;
        self.package.add_part(
            master.clone(),
            ct::PML_NOTES_MASTER,
            template::notes_master_xml().into_bytes(),
        )?;
        let theme = self.package.next_part_name("/ppt/theme/theme{}.xml")?;
        self.package.add_part(
            theme.clone(),
            ct::THEME,
            template::theme_xml("Office Theme").into_bytes(),
        )?;
        self.package
            .add_relationship(Some(&master), rel_types::THEME, &theme)?;
        let rid = self
            .package
            .add_relationship(Some(&self.part), rel_types::NOTES_MASTER, &master)?;
        self.presentation.notes_master_id_lst = Some(Box::new(pml::CT_NotesMasterIdList {
            notes_master_id: Some(Box::new(pml::CT_NotesMasterIdListEntry {
                r_id: Some(rid),
                ..Default::default()
            })),
            ..Default::default()
        }));
        self.dirty = true;
        Ok(master)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutKind;

    #[test]
    fn new_presentations_are_clean() {
        let mut deck = Presentation::new();
        assert!(!deck.dirty);
        deck.flush().unwrap();
        assert_eq!(deck.part_name().as_str(), "/ppt/presentation.xml");
        assert_eq!(deck.find_layout(LayoutRef::Kind(LayoutKind::Blank)).unwrap(), 5);
        assert!(deck.find_layout(LayoutRef::Name("nope")).is_err());
    }

    #[test]
    fn notes_master_is_created_once() {
        let mut deck = Presentation::new();
        let a = deck.ensure_notes_master().unwrap();
        let b = deck.ensure_notes_master().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "/ppt/notesMasters/notesMaster1.xml");
        let theme = deck.package.related_part(Some(&a), rel_types::THEME).unwrap();
        assert_eq!(theme.as_str(), "/ppt/theme/theme2.xml");
        assert!(deck.dirty);
    }

    #[test]
    fn orphaned_parts_are_collected_transitively() {
        let mut deck = Presentation::new();
        let a = PartName::new("/x/a.xml").unwrap();
        let b = PartName::new("/x/b.xml").unwrap();
        let shared = PartName::new("/x/shared.xml").unwrap();
        let layout = deck.layouts[0].part_name().clone();
        let pkg = &mut deck.package;
        for p in [&a, &b, &shared] {
            pkg.add_part(p.clone(), ct::XML, b"<x/>".to_vec()).unwrap();
        }
        pkg.add_relationship(Some(&a), "t", &b).unwrap();
        pkg.add_relationship(Some(&b), "t", &shared).unwrap();
        pkg.add_relationship(None, "t", &shared).unwrap();
        pkg.add_relationship(Some(&b), rel_types::SLIDE_LAYOUT, &layout)
            .unwrap();
        assert!(deck.is_referenced(&b));
        deck.remove_parts_and_orphans(vec![a.clone()]);
        assert!(!deck.package.contains(&a));
        assert!(!deck.package.contains(&b), "only referenced by the removed part");
        assert!(deck.package.contains(&shared), "still referenced by the package");
        assert!(deck.package.contains(&layout), "layouts stay");
    }

    #[test]
    fn parts_shared_by_removed_parts_are_collected() {
        // `c` is referenced by both `a` and `b`; removing both must remove `c`
        // whatever order they are processed in.
        let mut deck = Presentation::new();
        let a = PartName::new("/x/a.xml").unwrap();
        let b = PartName::new("/x/b.xml").unwrap();
        let c = PartName::new("/x/c.xml").unwrap();
        let pkg = &mut deck.package;
        for p in [&a, &b, &c] {
            pkg.add_part(p.clone(), ct::XML, b"<x/>".to_vec()).unwrap();
        }
        pkg.add_relationship(Some(&a), "t", &c).unwrap();
        pkg.add_relationship(Some(&b), "t", &c).unwrap();
        pkg.add_relationship(Some(&a), "t", &b).unwrap();
        deck.remove_parts_and_orphans(vec![a.clone(), b.clone()]);
        for p in [&a, &b, &c] {
            assert!(!deck.package.contains(p), "{p} should be gone");
        }
    }
}
