//! Hyperlinks on shapes, pictures and text runs: web and e-mail addresses,
//! jumps to slides and slide-show navigation actions.

use openxml_core::{Error, Result};
use openxml_opc::PartName;
use openxml_opc::known::rel_types;
use openxml_schema::{dml, pml};

use crate::paragraph::ParagraphMut;
use crate::presentation::Presentation;
use crate::slide::SlideMut;
use crate::table;
use crate::util;

const SLIDE_JUMP: &str = "ppaction://hlinksldjump";
const SHOW_JUMP: &str = "ppaction://hlinkshowjump?jump=";

/// The target of a hyperlink.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    /// A web address or any other URI.
    Url(String),
    /// An e-mail address (`mailto:`), with an optional subject.
    Email {
        /// The address.
        address: String,
        /// The subject line.
        subject: Option<String>,
    },
    /// A slide of the same presentation, by position (0-based).
    Slide(usize),
    /// The first slide of the show.
    FirstSlide,
    /// The last slide of the show.
    LastSlide,
    /// The next slide.
    NextSlide,
    /// The previous slide.
    PreviousSlide,
    /// Ends the slide show.
    EndShow,
}

impl Link {
    fn show_jump(&self) -> Option<&'static str> {
        Some(match self {
            Link::FirstSlide => "firstslide",
            Link::LastSlide => "lastslide",
            Link::NextSlide => "nextslide",
            Link::PreviousSlide => "previousslide",
            Link::EndShow => "endshow",
            _ => return None,
        })
    }
}

/// A hyperlink found on a slide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkInfo {
    /// The shape carrying the link (or holding the linked text).
    pub shape_id: u32,
    /// Text of the linked run, for links on text; `None` for links on the shape itself.
    pub text: Option<String>,
    /// The target.
    pub link: Link,
    /// The tooltip, if any.
    pub tooltip: Option<String>,
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mailto(address: &str, subject: Option<&str>) -> String {
    match subject {
        Some(s) => format!("mailto:{address}?subject={}", percent_encode(s)),
        None => format!("mailto:{address}"),
    }
}

fn parse_mailto(uri: &str) -> Option<Link> {
    let rest = uri.strip_prefix("mailto:")?;
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let subject = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("subject="))
        .map(percent_decode);
    Some(Link::Email {
        address: percent_decode(address),
        subject,
    })
}

/// Visits every hyperlink slot (`hlinkClick` / `hlinkHover` on shapes,
/// `hlinkClick` / `hlinkMouseOver` on runs) of a shape tree.
pub(crate) fn for_each_link_slot(
    tree: &mut pml::CT_GroupShape,
    f: &mut dyn FnMut(&mut Option<Box<dml::CT_Hyperlink>>),
) {
    fn runs(body: &mut dml::CT_TextBody, f: &mut dyn FnMut(&mut Option<Box<dml::CT_Hyperlink>>)) {
        for p in &mut body.p {
            for r in &mut p.text_run {
                let props = match r {
                    dml::EG_TextRun::R(run) => run.r_pr.as_deref_mut(),
                    dml::EG_TextRun::Br(br) => br.r_pr.as_deref_mut(),
                    dml::EG_TextRun::Fld(fld) => fld.r_pr.as_deref_mut(),
                    dml::EG_TextRun::Other(_) => None,
                };
                if let Some(props) = props {
                    f(&mut props.hlink_click);
                    f(&mut props.hlink_mouse_over);
                }
            }
            if let Some(end) = p.end_para_r_pr.as_deref_mut() {
                f(&mut end.hlink_click);
                f(&mut end.hlink_mouse_over);
            }
        }
    }
    for c in &mut tree.choice {
        if let Some(nv) = util::c_nv_pr_mut(c) {
            f(&mut nv.hlink_click);
            f(&mut nv.hlink_hover);
        }
        match c {
            pml::CT_GroupShape_Choice::Sp(sp) => {
                if let Some(body) = sp.tx_body.as_deref_mut() {
                    runs(body, f);
                }
            }
            pml::CT_GroupShape_Choice::GrpSp(g) => for_each_link_slot(g, f),
            pml::CT_GroupShape_Choice::GraphicFrame(frame) => {
                if let Some(mut t) = table::frame_table(frame) {
                    // Only links that exist can change; store the table back when there were any.
                    let mut touched = false;
                    let mut visit = |slot: &mut Option<Box<dml::CT_Hyperlink>>| {
                        touched |= slot.is_some();
                        f(slot);
                    };
                    for cell in t.tr.iter_mut().flat_map(|r| r.tc.iter_mut()) {
                        if let Some(body) = cell.tx_body.as_deref_mut() {
                            runs(body, &mut visit);
                        }
                    }
                    if touched {
                        table::store_table(frame, &t);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collects `(shape id, run text, hyperlink)` for every link of a tree.
fn collect_links(tree: &pml::CT_GroupShape, out: &mut Vec<(u32, Option<String>, dml::CT_Hyperlink)>) {
    fn runs(id: u32, body: &dml::CT_TextBody, out: &mut Vec<(u32, Option<String>, dml::CT_Hyperlink)>) {
        for p in &body.p {
            for r in &p.text_run {
                if let dml::EG_TextRun::R(run) = r
                    && let Some(h) = run.r_pr.as_ref().and_then(|p| p.hlink_click.as_deref())
                {
                    out.push((id, Some(run.t.clone().unwrap_or_default()), h.clone()));
                }
            }
        }
    }
    for c in &tree.choice {
        let id = util::choice_id(c).unwrap_or(0);
        if let Some(h) = util::c_nv_pr(c).and_then(|nv| nv.hlink_click.as_deref()) {
            out.push((id, None, h.clone()));
        }
        match c {
            pml::CT_GroupShape_Choice::Sp(sp) => {
                if let Some(body) = sp.tx_body.as_deref() {
                    runs(id, body, out);
                }
            }
            pml::CT_GroupShape_Choice::GrpSp(g) => collect_links(g, out),
            pml::CT_GroupShape_Choice::GraphicFrame(frame) => {
                if let Some(t) = table::frame_table(frame) {
                    for cell in t.tr.iter().flat_map(|r| r.tc.iter()) {
                        if let Some(body) = cell.tx_body.as_deref() {
                            runs(id, body, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Presentation {
    /// Resolves a hyperlink of the slide stored in `part`.
    fn resolve_link(&self, part: &PartName, h: &dml::CT_Hyperlink) -> Option<Link> {
        let action = h.action.as_deref().unwrap_or("");
        if let Some(jump) = action.strip_prefix(SHOW_JUMP) {
            return Some(match jump {
                "firstslide" => Link::FirstSlide,
                "lastslide" => Link::LastSlide,
                "nextslide" => Link::NextSlide,
                "previousslide" => Link::PreviousSlide,
                "endshow" => Link::EndShow,
                _ => return None,
            });
        }
        let rid = h.r_id.as_deref().filter(|r| !r.is_empty())?;
        let rel = self.package.relationships(Some(part))?.get(rid)?;
        if action == SLIDE_JUMP {
            let target = PartName::resolve(Some(part), &rel.target).ok()?;
            return self.slides.iter().position(|s| s.part == target).map(Link::Slide);
        }
        if !action.is_empty() || !rel.is_external() {
            return None;
        }
        Some(parse_mailto(&rel.target).unwrap_or_else(|| Link::Url(rel.target.clone())))
    }

    /// The hyperlinks of the slide at `index` (on shapes, pictures, groups,
    /// text runs and table cells), in document order.
    pub fn slide_links(&self, index: usize) -> Vec<LinkInfo> {
        let Some(slide) = self.slides.get(index) else {
            return Vec::new();
        };
        let Some(tree) = slide.data.c_sld.as_ref().and_then(|c| c.sp_tree.as_deref()) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        collect_links(tree, &mut found);
        found
            .into_iter()
            .filter_map(|(shape_id, text, h)| {
                Some(LinkInfo {
                    shape_id,
                    text,
                    link: self.resolve_link(&slide.part, &h)?,
                    tooltip: h.tooltip.clone(),
                })
            })
            .collect()
    }

    /// Removes hyperlinks whose relationship no longer exists (for example
    /// after the target slide was removed).
    pub(crate) fn scrub_dangling_links(&mut self) {
        for i in 0..self.slides.len() {
            let part = self.slides[i].part.clone();
            let rels: Vec<String> = self
                .package
                .relationships(Some(&part))
                .map(|r| r.iter().map(|r| r.id.clone()).collect())
                .unwrap_or_default();
            let Some(tree) = self.slides[i]
                .data
                .c_sld
                .as_mut()
                .and_then(|c| c.sp_tree.as_deref_mut())
            else {
                continue;
            };
            let mut changed = false;
            for_each_link_slot(tree, &mut |slot| {
                let dangling = slot
                    .as_ref()
                    .and_then(|h| h.r_id.as_deref())
                    .is_some_and(|r| !r.is_empty() && !rels.iter().any(|x| x == r));
                if dangling {
                    *slot = None;
                    changed = true;
                }
            });
            if changed {
                self.slides[i].dirty = true;
            }
        }
    }
}

impl SlideMut<'_> {
    /// Creates the relationship a link needs (if any) and returns the
    /// `a:hlinkClick` value to store on a shape or run.
    pub fn hyperlink(&mut self, link: &Link, tooltip: Option<&str>) -> Result<dml::CT_Hyperlink> {
        let part = self.part.clone();
        let pkg = &mut self.pres.package;
        let (r_id, action) = match link {
            Link::Url(url) => (
                pkg.add_external_relationship(Some(&part), rel_types::HYPERLINK, url)?,
                None,
            ),
            Link::Email { address, subject } => (
                pkg.add_external_relationship(
                    Some(&part),
                    rel_types::HYPERLINK,
                    &mailto(address, subject.as_deref()),
                )?,
                None,
            ),
            Link::Slide(i) => {
                let target = self
                    .pres
                    .slides
                    .get(*i)
                    .map(|s| s.part.clone())
                    .ok_or_else(|| Error::NotFound(format!("slide {i}")))?;
                let rid = crate::picture::relate(&mut self.pres.package, &part, rel_types::SLIDE, &target)?;
                (rid, Some(SLIDE_JUMP.to_owned()))
            }
            other => (
                String::new(),
                Some(format!(
                    "{SHOW_JUMP}{}",
                    other.show_jump().expect("navigation link")
                )),
            ),
        };
        Ok(dml::CT_Hyperlink {
            r_id: Some(r_id),
            action,
            tooltip: tooltip.map(str::to_owned),
            ..Default::default()
        })
    }

    fn release_link(&mut self, old: Option<Box<dml::CT_Hyperlink>>) {
        if let Some(rid) = old.and_then(|h| h.r_id).filter(|r| !r.is_empty()) {
            self.drop_relationship_if_unused(&rid);
        }
    }

    /// Sets (or with `None` removes) the click action of any graphic on the slide.
    ///
    /// ```
    /// use openxml_core::Length;
    /// use openxml_pptx::{LayoutKind, Link, Presentation, ShapeType};
    ///
    /// let mut deck = Presentation::new();
    /// deck.add_slide(LayoutKind::Blank)?;
    /// let mut slide = deck.add_slide(LayoutKind::Blank)?;
    /// let button = slide.add_shape(ShapeType::RoundRect, Length::cm(1.0), Length::cm(1.0), Length::cm(4.0), Length::cm(1.5)).id();
    /// slide.set_link(button, Some(Link::Slide(0)))?;
    /// let links = deck.slide_links(1);
    /// assert_eq!(links[0].link, Link::Slide(0));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_link(&mut self, shape_id: u32, link: Option<Link>) -> Result<()> {
        if util::find(self.tree_mut(), shape_id).is_none() {
            return Err(Error::NotFound(format!("shape {shape_id}")));
        }
        let value = link.map(|l| self.hyperlink(&l, None)).transpose()?;
        let nv = util::find_mut(self.tree_mut(), shape_id)
            .and_then(util::c_nv_pr_mut)
            .ok_or_else(|| Error::InvalidArgument(format!("shape {shape_id} cannot carry a link")))?;
        let old = std::mem::replace(&mut nv.hlink_click, value.map(Box::new));
        self.release_link(old);
        Ok(())
    }

    /// Links run `run` of paragraph `paragraph` of shape `shape_id` (or with
    /// `None` removes its link).
    pub fn set_run_link(
        &mut self,
        shape_id: u32,
        paragraph: usize,
        run: usize,
        link: Option<Link>,
    ) -> Result<()> {
        let missing = || Error::NotFound(format!("run {run} of paragraph {paragraph} of shape {shape_id}"));
        {
            let mut shape = self.shape_by_id(shape_id).ok_or_else(missing)?;
            let mut p = shape.paragraph_mut(paragraph).ok_or_else(missing)?;
            p.run_mut(run).ok_or_else(missing)?;
        }
        let value = link.map(|l| self.hyperlink(&l, None)).transpose()?;
        let mut shape = self.shape_by_id(shape_id).expect("checked");
        let mut p = shape.paragraph_mut(paragraph).expect("checked");
        let mut r = p.run_mut(run).expect("checked");
        let props = r.raw_mut().r_pr.get_or_insert_with(Box::default);
        let old = std::mem::replace(&mut props.hlink_click, value.map(Box::new));
        self.release_link(old);
        Ok(())
    }

    /// Appends a linked run to the last paragraph of shape `shape_id`.
    pub fn add_link_run(&mut self, shape_id: u32, text: &str, link: Link) -> Result<()> {
        if self.shape_by_id(shape_id).is_none() {
            return Err(Error::NotFound(format!("shape {shape_id}")));
        }
        let value = self.hyperlink(&link, None)?;
        let mut shape = self.shape_by_id(shape_id).expect("checked");
        let last = shape.paragraph_count().max(1) - 1;
        let mut p: ParagraphMut<'_> = match shape.paragraph_mut(last) {
            Some(p) => p,
            None => shape.add_paragraph(""),
        };
        let mut r = p.add_run(text);
        r.raw_mut().r_pr.get_or_insert_with(Box::default).hlink_click = Some(Box::new(value));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailto_round_trip() {
        let uri = mailto("a@b.org", Some("Hello world & more"));
        assert_eq!(uri, "mailto:a@b.org?subject=Hello%20world%20%26%20more");
        assert_eq!(
            parse_mailto(&uri),
            Some(Link::Email {
                address: "a@b.org".into(),
                subject: Some("Hello world & more".into())
            })
        );
        assert_eq!(
            parse_mailto("mailto:x@y.z"),
            Some(Link::Email {
                address: "x@y.z".into(),
                subject: None
            })
        );
        assert_eq!(parse_mailto("https://x"), None);
        assert_eq!(percent_decode("%E1%BB%87 %zz%"), "ệ %zz%");
    }

    #[test]
    fn navigation_actions() {
        assert_eq!(Link::FirstSlide.show_jump(), Some("firstslide"));
        assert_eq!(Link::EndShow.show_jump(), Some("endshow"));
        assert_eq!(Link::Slide(1).show_jump(), None);
    }
}
