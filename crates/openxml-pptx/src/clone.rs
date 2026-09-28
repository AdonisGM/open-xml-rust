//! Copying slides: duplicating within a presentation and importing from another.
//!
//! Copies keep their relationship identifiers, so the copied XML needs no
//! rewriting. Parts owned by a slide (charts, diagrams, embedded objects,
//! tags, legacy comments) are deep-copied under new names; parts meant to be
//! shared (layouts, images, media) are shared within a presentation and
//! copied once (deduplicated by content) across presentations.

use std::collections::{HashMap, HashSet};

use openxml_core::{Error, Result};
use openxml_opc::known::rel_types;
use openxml_opc::{Package, PartName, Relationship, TargetMode};
use openxml_schema::pml;

use crate::layout::LayoutRef;
use crate::media::MEDIA_REL_TYPE;
use crate::presentation::Presentation;
use crate::slide::{Notes, Slide, SlideMut};

/// Relationship type of PowerPoint 365 threaded comments (not copied).
const MODERN_COMMENTS_REL: &str = "http://schemas.microsoft.com/office/2018/10/relationships/comments";

/// Kinds whose targets are shared by every slide using them.
const MEDIA_TYPES: &[&str] = &[
    rel_types::IMAGE,
    rel_types::VIDEO,
    rel_types::AUDIO,
    MEDIA_REL_TYPE,
];

/// URI of the `p:ext` holding PowerPoint's `p14:creationId` of a slide.
const CREATION_ID_URI: &str = "{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}";

/// Gives a copied slide a new `p14:creationId` (PowerPoint identifies
/// slides by it when merging and co-authoring).
fn renew_creation_id(data: &mut pml::CT_Slide, seed: &str) {
    let Some(ext_lst) = data.c_sld.as_mut().and_then(|c| c.ext_lst.as_mut()) else {
        return;
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    // FNV-1a over the seed, mixed with the clock.
    let mut hash: u32 = 0x811C_9DC5 ^ nanos;
    for b in seed.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    for ext in ext_lst
        .ext
        .iter_mut()
        .filter(|e| e.uri.as_deref() == Some(CREATION_ID_URI))
    {
        for raw in ext.any.iter_mut().filter(|r| &*r.name.local == "creationId") {
            raw.set_attr(openxml_xml::Ns::NONE, "val", hash.max(1).to_string());
        }
    }
}

/// A part to add, with its relationships already pointing at their new targets.
struct PlannedPart {
    name: PartName,
    content_type: String,
    data: Vec<u8>,
    rels: Vec<Relationship>,
}

/// Plans deep copies of parts from `src` into `dst` (which may be the same package).
struct CopyPlan<'a> {
    src: &'a Package,
    dst: &'a Package,
    same_package: bool,
    map: HashMap<PartName, PartName>,
    reserved: HashSet<PartName>,
    parts: Vec<PlannedPart>,
}

/// The numbering pattern of a part name: `/ppt/charts/chart3.xml` → `/ppt/charts/chart{}.xml`.
fn name_pattern(name: &PartName) -> String {
    let s = name.as_str();
    let (stem, ext) = match s.rfind('.') {
        Some(dot) if dot > s.rfind('/').unwrap_or(0) => (&s[..dot], &s[dot..]),
        _ => (s, ""),
    };
    let base = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    format!("{base}{{}}{ext}")
}

impl<'a> CopyPlan<'a> {
    fn new(src: &'a Package, dst: &'a Package, same_package: bool) -> Self {
        CopyPlan {
            src,
            dst,
            same_package,
            map: HashMap::new(),
            reserved: HashSet::new(),
            parts: Vec::new(),
        }
    }

    /// A part name from `pattern` free in the destination and not yet planned.
    fn free_name(&mut self, pattern: &str) -> Result<PartName> {
        for n in 1u32.. {
            let name = PartName::new(pattern.replacen("{}", &n.to_string(), 1))?;
            if !self.dst.contains(&name) && !self.reserved.contains(&name) {
                self.reserved.insert(name.clone());
                return Ok(name);
            }
        }
        unreachable!("unbounded search")
    }

    /// The destination of a media part: shared in the same package, otherwise
    /// an existing identical part or a planned copy.
    fn media(&mut self, part: &PartName) -> Result<PartName> {
        if self.same_package {
            return Ok(part.clone());
        }
        if let Some(done) = self.map.get(part) {
            return Ok(done.clone());
        }
        let source = self
            .src
            .part(part)
            .ok_or_else(|| Error::MissingPart(part.to_string()))?;
        let existing = self
            .dst
            .parts()
            .find(|(n, p)| n.as_str().starts_with("/ppt/media/") && p.data() == source.data())
            .map(|(n, _)| n.clone())
            .or_else(|| {
                self.parts
                    .iter()
                    .find(|p| p.name.as_str().starts_with("/ppt/media/") && p.data == source.data())
                    .map(|p| p.name.clone())
            });
        let name = match existing {
            Some(n) => n,
            None => {
                let name = self.free_name(&name_pattern(part))?;
                self.parts.push(PlannedPart {
                    name: name.clone(),
                    content_type: source.content_type().to_owned(),
                    data: source.data().to_vec(),
                    rels: Vec::new(),
                });
                name
            }
        };
        self.map.insert(part.clone(), name.clone());
        Ok(name)
    }

    /// Copies relationship `rel` of `source` for `new_source`, retargeting
    /// internal targets with `target`.
    fn copy_rel(
        &mut self,
        source: &PartName,
        new_source: &PartName,
        rel: &Relationship,
        target: impl FnOnce(&mut Self, &PartName, &str) -> Result<Option<PartName>>,
    ) -> Result<Option<Relationship>> {
        if rel.is_external() {
            return Ok(Some(rel.clone()));
        }
        let Ok(old) = PartName::resolve(Some(source), &rel.target) else {
            return Ok(Some(rel.clone()));
        };
        if !self.src.contains(&old) {
            return Ok(Some(rel.clone()));
        }
        Ok(target(self, &old, &rel.rel_type)?.map(|t| Relationship {
            id: rel.id.clone(),
            rel_type: rel.rel_type.clone(),
            target: t.relative_to(Some(new_source)),
            target_mode: TargetMode::Internal,
        }))
    }

    /// Deep-copies `part` and what it relates to (media is shared or deduplicated).
    fn deep(&mut self, part: &PartName) -> Result<PartName> {
        if let Some(done) = self.map.get(part) {
            return Ok(done.clone());
        }
        let source = self
            .src
            .part(part)
            .ok_or_else(|| Error::MissingPart(part.to_string()))?;
        let name = self.free_name(&name_pattern(part))?;
        self.map.insert(part.clone(), name.clone());
        let mut rels = Vec::new();
        for rel in source.relationships().iter() {
            let copied = self.copy_rel(part, &name, rel, |plan, old, kind| {
                Ok(Some(if MEDIA_TYPES.contains(&kind) {
                    plan.media(old)?
                } else if plan.same_package && (kind == rel_types::SLIDE_LAYOUT || kind == rel_types::SLIDE) {
                    old.clone()
                } else {
                    plan.deep(old)?
                }))
            })?;
            rels.extend(copied);
        }
        self.parts.push(PlannedPart {
            name: name.clone(),
            content_type: source.content_type().to_owned(),
            data: source.data().to_vec(),
            rels,
        });
        Ok(name)
    }
}

fn apply(pkg: &mut Package, parts: Vec<PlannedPart>) -> Result<()> {
    for p in &parts {
        pkg.add_part(p.name.clone(), &p.content_type, p.data.clone())?;
    }
    for p in parts {
        let rels = pkg
            .relationships_mut(Some(&p.name))
            .ok_or_else(|| Error::MissingPart(p.name.to_string()))?;
        for r in p.rels {
            rels.insert(r)?;
        }
    }
    Ok(())
}

/// What the slide-level copy needs to know about the destination.
struct SlideTargets {
    slide: PartName,
    notes: Option<PartName>,
    layout: Option<PartName>,
    notes_master: Option<PartName>,
}

/// Plans the relationships of a slide (or its notes) copied to `targets`.
fn plan_slide_rels(
    plan: &mut CopyPlan<'_>,
    source: &PartName,
    new_source: &PartName,
    targets: &SlideTargets,
    is_notes: bool,
) -> Result<Vec<Relationship>> {
    let rels: Vec<Relationship> = plan
        .src
        .relationships(Some(source))
        .map(|r| r.iter().cloned().collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for rel in &rels {
        if rel.rel_type == MODERN_COMMENTS_REL || rel.rel_type == rel_types::COMMENTS {
            // Legacy comments are re-created by the caller; threaded comments are not copied.
            continue;
        }
        let copied = plan.copy_rel(source, new_source, rel, |plan, old, kind| {
            Ok(Some(match kind {
                k if k == rel_types::SLIDE_LAYOUT => targets.layout.clone().unwrap_or_else(|| old.clone()),
                k if k == rel_types::NOTES_MASTER => {
                    targets.notes_master.clone().unwrap_or_else(|| old.clone())
                }
                k if k == rel_types::NOTES_SLIDE => match &targets.notes {
                    Some(n) => n.clone(),
                    None => return Ok(None),
                },
                // The notes page points back at its slide.
                k if k == rel_types::SLIDE && is_notes => targets.slide.clone(),
                // Jumps to other slides: kept within a presentation; across
                // presentations the slides do not exist, so they point at the copy itself.
                k if k == rel_types::SLIDE => {
                    if plan.same_package {
                        old.clone()
                    } else {
                        targets.slide.clone()
                    }
                }
                k if MEDIA_TYPES.contains(&k) => plan.media(old)?,
                _ => plan.deep(old)?,
            }))
        })?;
        out.extend(copied);
    }
    Ok(out)
}

impl Presentation {
    /// Inserts `slide` (whose part is already in the package) at `position`.
    fn insert_slide(&mut self, position: usize, mut slide: Slide) -> Result<usize> {
        let rid = self
            .package
            .add_relationship(Some(&self.part), rel_types::SLIDE, &slide.part)?;
        let id = self
            .slides
            .iter()
            .map(|s| s.id + 1)
            .chain(
                self.presentation
                    .sld_id_lst
                    .iter()
                    .flat_map(|l| &l.sld_id)
                    .filter_map(|e| e.id)
                    .map(|i| i + 1),
            )
            .max()
            .unwrap_or(256)
            .max(256);
        slide.id = id;
        let list = self.presentation.sld_id_lst.get_or_insert_with(Box::default);
        let entry = pml::CT_SlideIdListEntry {
            id: Some(id),
            r_id: Some(rid),
            ..Default::default()
        };
        // Keep the id list in presentation order.
        let at = if position == 0 {
            0
        } else {
            let before = self.slides[position - 1].id;
            list.sld_id
                .iter()
                .position(|e| e.id == Some(before))
                .map_or(list.sld_id.len(), |p| p + 1)
        };
        list.sld_id.insert(at, entry);
        let previous = position.checked_sub(1).map(|i| self.slides[i].id);
        self.slides.insert(position, slide);
        self.sections_insert_slide(previous, id);
        self.dirty = true;
        Ok(position)
    }

    /// Duplicates the slide at `index` (with its notes, legacy comments and
    /// owned objects such as charts) and inserts the copy right after it.
    /// Returns the position of the copy.
    ///
    /// ```
    /// use openxml_pptx::{LayoutKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// deck.add_slide(LayoutKind::Title)?.set_title("Hello")?;
    /// let copy = deck.duplicate_slide(0)?;
    /// assert_eq!(copy, 1);
    /// assert_eq!(deck.slide(1).unwrap().title().as_deref(), Some("Hello"));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn duplicate_slide(&mut self, index: usize) -> Result<usize> {
        let source = self
            .slides
            .get(index)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("slide {index}")))?;
        let mut copy_data = source.data.clone();
        let (planned, targets) = {
            let mut plan = CopyPlan::new(&self.package, &self.package, true);
            let slide_name = plan.free_name("/ppt/slides/slide{}.xml")?;
            let notes_name = match &source.notes {
                Some(_) => Some(plan.free_name("/ppt/notesSlides/notesSlide{}.xml")?),
                None => None,
            };
            let targets = SlideTargets {
                slide: slide_name.clone(),
                notes: notes_name.clone(),
                layout: None,
                notes_master: None,
            };
            renew_creation_id(&mut copy_data, slide_name.as_str());
            let slide_rels = plan_slide_rels(&mut plan, &source.part, &slide_name, &targets, false)?;
            let notes_rels = match (&source.notes, &notes_name) {
                (Some(n), Some(name)) => Some(plan_slide_rels(&mut plan, &n.part, name, &targets, true)?),
                _ => None,
            };
            let mut parts = plan.parts;
            parts.push(PlannedPart {
                name: slide_name.clone(),
                content_type: self
                    .package
                    .part(&source.part)
                    .map_or(openxml_opc::known::content_types::PML_SLIDE, |p| p.content_type())
                    .to_owned(),
                data: pml::elements::SLD.to_bytes(&copy_data),
                rels: slide_rels,
            });
            if let (Some(n), Some(name), Some(rels)) = (&source.notes, &notes_name, notes_rels) {
                parts.push(PlannedPart {
                    name: name.clone(),
                    content_type: openxml_opc::known::content_types::PML_NOTES_SLIDE.to_owned(),
                    data: pml::elements::NOTES.to_bytes(&n.data),
                    rels,
                });
            }
            (parts, targets)
        };
        apply(&mut self.package, planned)?;
        let copy = Slide {
            part: targets.slide.clone(),
            id: 0,
            data: copy_data,
            layout: source.layout.clone(),
            layout_name: source.layout_name.clone(),
            notes: source
                .notes
                .as_ref()
                .zip(targets.notes.clone())
                .map(|(n, part)| Notes {
                    part,
                    data: n.data.clone(),
                    dirty: false,
                }),
            dirty: false,
        };
        let position = self.insert_slide(index + 1, copy)?;
        self.copy_legacy_comments(&self.clone_comments_source(index)?, position)?;
        Ok(position)
    }

    fn clone_comments_source(&self, index: usize) -> Result<Vec<(crate::comments::Comment, String)>> {
        let authors = self.comment_authors()?;
        Ok(self
            .comments(index)?
            .into_iter()
            .map(|c| {
                let initials = authors
                    .iter()
                    .find(|a| a.id == c.author_id)
                    .map(|a| a.initials.clone())
                    .unwrap_or_default();
                (c, initials)
            })
            .collect())
    }

    fn copy_legacy_comments(
        &mut self,
        comments: &[(crate::comments::Comment, String)],
        position: usize,
    ) -> Result<()> {
        if comments.is_empty() {
            return Ok(());
        }
        let mut slide = self.slide_mut(position).expect("inserted");
        for (c, initials) in comments {
            slide.add_comment_dated(&c.author, initials, &c.text, c.position.0, c.position.1, &c.date)?;
        }
        Ok(())
    }

    /// Copies slide `index` of `source` to the end of this presentation.
    ///
    /// The copy uses this presentation's layout with the same name (or else
    /// the same kind, or else the first layout) and therefore its theme,
    /// like PowerPoint's "Use Destination Theme". Images and media are
    /// copied once, notes are attached to this presentation's notes master,
    /// legacy comments are re-created with this presentation's authors, and
    /// jumps to other slides of the source (which do not exist here) point
    /// at the copy itself. Returns the position of the copy.
    pub fn import_slide(&mut self, source: &Presentation, index: usize) -> Result<usize> {
        let src_slide = source
            .slides
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("slide {index} of the source")))?;
        let layout_index = self.matching_layout(source, src_slide);
        let layout = self.layouts[layout_index].part.clone();
        let layout_name = self.layouts[layout_index].name().to_owned();
        let notes_master = match src_slide.notes {
            Some(_) => Some(self.ensure_notes_master()?),
            None => None,
        };
        let mut copy_data = src_slide.data.clone();
        let (planned, targets) = {
            let mut plan = CopyPlan::new(&source.package, &self.package, false);
            let slide_name = plan.free_name("/ppt/slides/slide{}.xml")?;
            let notes_name = match &src_slide.notes {
                Some(_) => Some(plan.free_name("/ppt/notesSlides/notesSlide{}.xml")?),
                None => None,
            };
            let targets = SlideTargets {
                slide: slide_name.clone(),
                notes: notes_name.clone(),
                layout: Some(layout.clone()),
                notes_master,
            };
            renew_creation_id(&mut copy_data, slide_name.as_str());
            let slide_rels = plan_slide_rels(&mut plan, &src_slide.part, &slide_name, &targets, false)?;
            let notes_rels = match (&src_slide.notes, &notes_name) {
                (Some(n), Some(name)) => Some(plan_slide_rels(&mut plan, &n.part, name, &targets, true)?),
                _ => None,
            };
            let mut parts = plan.parts;
            parts.push(PlannedPart {
                name: slide_name.clone(),
                content_type: openxml_opc::known::content_types::PML_SLIDE.to_owned(),
                data: pml::elements::SLD.to_bytes(&copy_data),
                rels: slide_rels,
            });
            if let (Some(n), Some(name), Some(rels)) = (&src_slide.notes, &notes_name, notes_rels) {
                parts.push(PlannedPart {
                    name: name.clone(),
                    content_type: openxml_opc::known::content_types::PML_NOTES_SLIDE.to_owned(),
                    data: pml::elements::NOTES.to_bytes(&n.data),
                    rels,
                });
            }
            (parts, targets)
        };
        apply(&mut self.package, planned)?;
        let copy = Slide {
            part: targets.slide.clone(),
            id: 0,
            data: copy_data,
            layout: Some(layout),
            layout_name: Some(layout_name),
            notes: src_slide
                .notes
                .as_ref()
                .zip(targets.notes.clone())
                .map(|(n, part)| Notes {
                    part,
                    data: n.data.clone(),
                    dirty: false,
                }),
            dirty: false,
        };
        let position = self.slides.len();
        self.insert_slide(position, copy)?;
        let comments = source.clone_comments_source(index)?;
        self.copy_legacy_comments(&comments, position)?;
        Ok(position)
    }

    /// The layout of this presentation best matching the layout of `slide` of `source`.
    fn matching_layout(&self, source: &Presentation, slide: &Slide) -> usize {
        let src_layout = slide
            .layout
            .as_ref()
            .and_then(|lp| source.layouts.iter().find(|l| &l.part == lp));
        if let Some(name) = slide.layout_name.as_deref()
            && let Some(i) = self
                .layouts
                .iter()
                .enumerate()
                .position(|(i, l)| l.matches(i, &LayoutRef::Name(name)))
        {
            return i;
        }
        if let Some(t) = src_layout.and_then(|l| l.data.type_)
            && let Some(i) = self.layouts.iter().position(|l| l.data.type_ == Some(t))
        {
            return i;
        }
        0
    }
}

impl SlideMut<'_> {
    /// Duplicates this slide (see [`Presentation::duplicate_slide`]) and
    /// returns the position of the copy.
    pub fn duplicate(self) -> Result<usize> {
        let index = self.index;
        self.pres.duplicate_slide(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_patterns() {
        let p = |s: &str| name_pattern(&PartName::new(s).unwrap());
        assert_eq!(p("/ppt/charts/chart12.xml"), "/ppt/charts/chart{}.xml");
        assert_eq!(
            p("/ppt/embeddings/Microsoft_Excel_Worksheet.xlsx"),
            "/ppt/embeddings/Microsoft_Excel_Worksheet{}.xlsx"
        );
        assert_eq!(p("/ppt/tags/tag3"), "/ppt/tags/tag{}");
        assert_eq!(p("/ppt/v1.2/x"), "/ppt/v1.2/x{}");
    }
}
