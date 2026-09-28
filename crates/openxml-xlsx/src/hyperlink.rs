//! Hyperlinks: links from cells to web pages, files, e-mail addresses
//! (external relationships) or places in the workbook (ECMA-376 Part 1
//! §18.3.1.47).
//!
//! ```
//! use openxml_xlsx::{Workbook, LinkTarget};
//!
//! let mut wb = Workbook::new();
//! wb.add_worksheet("Data")?;
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.set_link("A1", "Rust website", LinkTarget::Url("https://www.rust-lang.org/".into()))?;
//! sheet.set_link("A2", "Go to data", LinkTarget::Location("Data!A1".into()))?;
//! let links = sheet.as_view().hyperlinks();
//! assert_eq!(links[0].target, LinkTarget::Url("https://www.rust-lang.org/".into()));
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_opc::known::rel_types;
use openxml_schema::sml;

use crate::cell_ref::{CellRange, ToCellRange, ToCellRef};
use crate::styles::{CellStyle, Color};
use crate::worksheet::{Worksheet, WorksheetMut};

/// Longest link target Excel accepts.
const MAX_URL_LEN: usize = 2079;

/// Where a hyperlink goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// An external address: `https://…`, `mailto:…`, a file path.
    Url(String),
    /// A place in the workbook: `Sheet2!B3`, `'My Sheet'!A1`, a defined name.
    Location(String),
}

/// A hyperlink on a cell or range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperlink {
    /// The linked cells.
    pub range: CellRange,
    /// The target.
    pub target: LinkTarget,
    /// Text shown when hovering.
    pub tooltip: Option<String>,
    /// Display text recorded with the link.
    pub display: Option<String>,
}

impl Hyperlink {
    /// A link on `range` to `target`.
    pub fn new(range: impl ToCellRange, target: LinkTarget) -> Result<Self> {
        Ok(Hyperlink {
            range: range.to_cell_range()?,
            target,
            tooltip: None,
            display: None,
        })
    }

    /// Hover text.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

impl Worksheet<'_> {
    /// Hyperlinks of the sheet in document order.
    pub fn hyperlinks(&self) -> Vec<Hyperlink> {
        let rels = self.env.package.relationships(Some(self.part));
        self.data
            .hyperlinks
            .iter()
            .flat_map(|h| h.hyperlink.iter())
            .filter_map(|h| {
                let range = CellRange::parse(h.ref_.as_deref()?).ok()?;
                let url = h
                    .r_id
                    .as_deref()
                    .and_then(|id| rels?.get(id))
                    .map(|r| r.target.clone());
                let target = match (url, &h.location) {
                    (Some(u), Some(l)) => LinkTarget::Url(format!("{u}#{l}")),
                    (Some(u), None) => LinkTarget::Url(u),
                    (None, Some(l)) => LinkTarget::Location(l.clone()),
                    (None, None) => return None,
                };
                Some(Hyperlink {
                    range,
                    target,
                    tooltip: h.tooltip.clone(),
                    display: h.display.clone(),
                })
            })
            .collect()
    }

    /// The hyperlink covering a cell.
    pub fn hyperlink(&self, at: impl ToCellRef) -> Result<Option<Hyperlink>> {
        let at = at.to_cell_ref()?;
        Ok(self.hyperlinks().into_iter().find(|h| h.range.contains(at)))
    }
}

impl WorksheetMut<'_> {
    /// Links cells to an external address.
    pub fn add_hyperlink(&mut self, at: impl ToCellRange, url: &str) -> Result<()> {
        self.add_hyperlink_with(&Hyperlink::new(at, LinkTarget::Url(url.to_owned()))?)
    }

    /// Links cells to a place in the workbook (`Sheet2!A1`, a defined name).
    pub fn add_internal_link(&mut self, at: impl ToCellRange, location: &str) -> Result<()> {
        self.add_hyperlink_with(&Hyperlink::new(at, LinkTarget::Location(location.to_owned()))?)
    }

    /// Adds a hyperlink, replacing any link on the same range.
    pub fn add_hyperlink_with(&mut self, link: &Hyperlink) -> Result<()> {
        let mut h = sml::CT_Hyperlink {
            ref_: Some(link.range.to_string()),
            tooltip: link.tooltip.clone(),
            display: link.display.clone(),
            ..Default::default()
        };
        match &link.target {
            LinkTarget::Url(u) => {
                if u.is_empty() || u.chars().count() > MAX_URL_LEN {
                    return Err(Error::InvalidArgument(format!(
                        "a link address must have 1 to {MAX_URL_LEN} characters"
                    )));
                }
                h.r_id = Some(self.package.add_external_relationship(
                    Some(self.part),
                    rel_types::HYPERLINK,
                    u,
                )?);
            }
            LinkTarget::Location(l) => {
                if l.is_empty() {
                    return Err(Error::InvalidArgument("empty link location".into()));
                }
                h.location = Some(l.trim_start_matches('#').to_owned());
            }
        }
        self.remove_hyperlinks_where(|r| r == link.range);
        self.data
            .hyperlinks
            .get_or_insert_with(Box::default)
            .hyperlink
            .push(h);
        Ok(())
    }

    /// Writes `text` into a cell and links it, giving it Excel's built-in
    /// *Hyperlink* cell style (blue, underlined) when the cell has no style.
    pub fn set_link(&mut self, at: impl ToCellRef, text: &str, target: LinkTarget) -> Result<()> {
        let at = at.to_cell_ref()?;
        self.set_value(at, text)?;
        self.add_hyperlink_with(&Hyperlink {
            range: CellRange::single(at),
            target,
            tooltip: None,
            display: None,
        })?;
        if self.as_view().cell_style(at)?.is_none() {
            let style = CellStyle::new().underline().font_color(Color::Theme(10));
            let id = self
                .styles
                .add_builtin_style("Hyperlink", 8, &style)
                .map_err(Error::InvalidArgument)?;
            self.set_cell_style(at, id)?;
        }
        Ok(())
    }

    /// Removes the hyperlinks covering a cell. Returns whether one existed.
    pub fn remove_hyperlink(&mut self, at: impl ToCellRef) -> Result<bool> {
        let at = at.to_cell_ref()?;
        Ok(self.remove_hyperlinks_where(|r| r.contains(at)) > 0)
    }

    /// Removes links whose range matches, with their unused relationships.
    pub(crate) fn remove_hyperlinks_where(&mut self, matches: impl Fn(CellRange) -> bool) -> usize {
        let Some(links) = self.data.hyperlinks.as_mut() else {
            return 0;
        };
        let before = links.hyperlink.len();
        let mut dropped_ids = Vec::new();
        links.hyperlink.retain(|h| {
            let hit = h
                .ref_
                .as_deref()
                .and_then(|r| CellRange::parse(r).ok())
                .is_some_and(&matches);
            if hit && let Some(id) = &h.r_id {
                dropped_ids.push(id.clone());
            }
            !hit
        });
        let removed = before - links.hyperlink.len();
        let still_used: Vec<String> = links.hyperlink.iter().filter_map(|h| h.r_id.clone()).collect();
        if links.hyperlink.is_empty() {
            self.data.hyperlinks = None;
        }
        if let Some(rels) = self.package.relationships_mut(Some(self.part)) {
            for id in dropped_ids.iter().filter(|id| !still_used.contains(id)) {
                rels.remove(id);
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;

    #[test]
    fn links_replace_each_other() {
        let mut wb = Workbook::new();
        let mut s = wb.worksheet_mut("Sheet1").unwrap();
        s.add_hyperlink("A1", "https://a.example/").unwrap();
        s.add_hyperlink("A1", "https://b.example/").unwrap();
        assert_eq!(s.as_view().hyperlinks().len(), 1);
        let part = s.part_name().clone();
        let external = s
            .package_mut()
            .relationships(Some(&part))
            .unwrap()
            .iter()
            .filter(|r| r.is_external())
            .count();
        assert_eq!(external, 1, "the replaced link's relationship is removed");
        s.add_internal_link("B1:B3", "#Sheet1!A1").unwrap();
        assert_eq!(
            s.as_view().hyperlink("B2").unwrap().unwrap().target,
            LinkTarget::Location("Sheet1!A1".into())
        );
        assert!(s.add_hyperlink("C1", &"x".repeat(MAX_URL_LEN + 1)).is_err());
        assert!(s.add_internal_link("C1", "").is_err());
        assert!(s.remove_hyperlink("B3").unwrap());
        assert!(
            s.as_view().hyperlink("B1").unwrap().is_none(),
            "the whole range is unlinked"
        );
    }
}
