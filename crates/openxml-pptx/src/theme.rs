//! Themes (colour and font schemes), slide master backgrounds, footers
//! (date, footer text, slide number) and the `p:hf` settings.

use openxml_core::part::{read_part, write_part};
use openxml_core::{Error, Result};
use openxml_opc::PartName;
use openxml_opc::known::{content_types as ct, rel_types};
use openxml_schema::{dml, pml};

use crate::drawing::{Color, Fill};
use crate::layout::LayoutKind;
use crate::presentation::Presentation;
use crate::shape::{self, PlaceholderKind};
use crate::slide::{Slide, SlideMut};
use crate::text::{self, Rgb};

/// The twelve colours of a theme's colour scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThemeColors {
    /// Dark 1 (usually the text colour).
    pub dark1: Rgb,
    /// Light 1 (usually the background colour).
    pub light1: Rgb,
    /// Dark 2.
    pub dark2: Rgb,
    /// Light 2.
    pub light2: Rgb,
    /// Accent 1.
    pub accent1: Rgb,
    /// Accent 2.
    pub accent2: Rgb,
    /// Accent 3.
    pub accent3: Rgb,
    /// Accent 4.
    pub accent4: Rgb,
    /// Accent 5.
    pub accent5: Rgb,
    /// Accent 6.
    pub accent6: Rgb,
    /// Hyperlinks.
    pub hyperlink: Rgb,
    /// Followed hyperlinks.
    pub followed_hyperlink: Rgb,
}

/// The typefaces of one font collection (headings or body) of a theme.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontSet {
    /// Latin typeface.
    pub latin: String,
    /// East Asian typeface (empty for none).
    pub east_asian: String,
    /// Complex-script typeface (empty for none).
    pub complex_script: String,
}

/// The font scheme of a theme.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThemeFonts {
    /// Heading fonts (`+mj-lt`, …).
    pub major: FontSet,
    /// Body fonts (`+mn-lt`, …).
    pub minor: FontSet,
}

/// The date shown in the date placeholder.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DateField {
    /// Fixed text.
    Fixed(String),
    /// The current date, updated automatically (`M/d/yyyy`).
    Automatic,
}

/// Header and footer settings ("Insert > Header & Footer").
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct HeaderFooter {
    /// Footer text.
    pub footer: Option<String>,
    /// Show slide numbers.
    pub slide_number: bool,
    /// Show a date.
    pub date: Option<DateField>,
    /// Leave title slides without footers.
    pub skip_title_slides: bool,
}

fn rgb_of(c: Option<&dml::CT_Color>) -> Rgb {
    c.and_then(|c| c.color_choice.as_ref())
        .and_then(Color::from_dml)
        .and_then(|c| match c {
            Color::Rgb(rgb) => Some(rgb),
            Color::Scheme(_) => None,
        })
        .unwrap_or(Rgb::BLACK)
}

fn set_color(slot: &mut Option<Box<dml::CT_Color>>, value: Rgb) {
    if rgb_of(slot.as_deref()) != value || slot.is_none() {
        *slot = Some(Box::new(Color::Rgb(value).to_ct()));
    }
}

fn font_set(c: Option<&dml::CT_FontCollection>) -> FontSet {
    let face = |f: Option<&dml::CT_TextFont>| f.and_then(|f| f.typeface.clone()).unwrap_or_default();
    FontSet {
        latin: face(c.and_then(|c| c.latin.as_deref())),
        east_asian: face(c.and_then(|c| c.ea.as_deref())),
        complex_script: face(c.and_then(|c| c.cs.as_deref())),
    }
}

fn apply_font_set(c: &mut dml::CT_FontCollection, f: &FontSet) {
    c.latin = Some(Box::new(text::font(&f.latin)));
    c.ea = Some(Box::new(text::font(&f.east_asian)));
    c.cs = Some(Box::new(text::font(&f.complex_script)));
}

/// The id of the slide-number field of new footers.
const SLIDE_NUMBER_FIELD_ID: &str = "{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}";
/// The id of the date field of new footers.
const DATE_FIELD_ID: &str = "{8A4C0F4B-5D2B-4E0E-9E26-1B7F2C3D4E5F}";

impl Presentation {
    /// Slide master parts in `sldMasterIdLst` order.
    pub(crate) fn master_parts(&self) -> Vec<PartName> {
        self.presentation
            .sld_master_id_lst
            .iter()
            .flat_map(|l| &l.sld_master_id)
            .filter_map(|m| {
                self.package
                    .relationship_target(Some(&self.part), m.r_id.as_deref()?)
            })
            .filter(|p| self.package.contains(p))
            .collect()
    }

    /// Theme parts of the slide masters (each once).
    fn master_themes(&self) -> Vec<PartName> {
        let mut out: Vec<PartName> = Vec::new();
        for m in self.master_parts() {
            if let Some(t) = self.package.related_part(Some(&m), rel_types::THEME)
                && !out.contains(&t)
            {
                out.push(t);
            }
        }
        out
    }

    fn first_theme(&self) -> Result<dml::CT_OfficeStyleSheet> {
        let part = self
            .master_themes()
            .into_iter()
            .next()
            .ok_or_else(|| Error::MissingPart("theme of the slide master".into()))?;
        read_part(&self.package, &part, &dml::elements::THEME)
    }

    fn edit_themes(&mut self, f: impl Fn(&mut dml::CT_BaseStyles)) -> Result<()> {
        for part in self.master_themes() {
            let mut theme = read_part(&self.package, &part, &dml::elements::THEME)?;
            f(theme.theme_elements.get_or_insert_with(Box::default));
            write_part(&mut self.package, &part, ct::THEME, &dml::elements::THEME, &theme)?;
        }
        Ok(())
    }

    /// The colour scheme of the (first) slide master's theme. System colours
    /// are reported by their last computed value.
    pub fn theme_colors(&self) -> Result<ThemeColors> {
        let theme = self.first_theme()?;
        let s = theme
            .theme_elements
            .as_ref()
            .and_then(|e| e.clr_scheme.as_deref())
            .ok_or_else(|| Error::InvalidDocument("the theme has no colour scheme".into()))?;
        Ok(ThemeColors {
            dark1: rgb_of(s.dk1.as_deref()),
            light1: rgb_of(s.lt1.as_deref()),
            dark2: rgb_of(s.dk2.as_deref()),
            light2: rgb_of(s.lt2.as_deref()),
            accent1: rgb_of(s.accent1.as_deref()),
            accent2: rgb_of(s.accent2.as_deref()),
            accent3: rgb_of(s.accent3.as_deref()),
            accent4: rgb_of(s.accent4.as_deref()),
            accent5: rgb_of(s.accent5.as_deref()),
            accent6: rgb_of(s.accent6.as_deref()),
            hyperlink: rgb_of(s.hlink.as_deref()),
            followed_hyperlink: rgb_of(s.fol_hlink.as_deref()),
        })
    }

    /// Replaces the colour scheme of every slide master's theme (colours that
    /// do not change keep their original definition, e.g. system colours).
    ///
    /// ```
    /// use openxml_pptx::{Presentation, Rgb};
    ///
    /// let mut deck = Presentation::new();
    /// let mut colors = deck.theme_colors()?;
    /// colors.accent1 = Rgb(0xE0, 0x40, 0x10);
    /// deck.set_theme_colors(&colors, Some("Sunset"))?;
    /// assert_eq!(deck.theme_colors()?.accent1, Rgb(0xE0, 0x40, 0x10));
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn set_theme_colors(&mut self, colors: &ThemeColors, scheme_name: Option<&str>) -> Result<()> {
        let c = *colors;
        let name = scheme_name.map(str::to_owned);
        self.edit_themes(|e| {
            let s = e.clr_scheme.get_or_insert_with(Box::default);
            if let Some(n) = &name {
                s.name = Some(n.clone());
            } else if s.name.is_none() {
                s.name = Some("Custom".into());
            }
            set_color(&mut s.dk1, c.dark1);
            set_color(&mut s.lt1, c.light1);
            set_color(&mut s.dk2, c.dark2);
            set_color(&mut s.lt2, c.light2);
            set_color(&mut s.accent1, c.accent1);
            set_color(&mut s.accent2, c.accent2);
            set_color(&mut s.accent3, c.accent3);
            set_color(&mut s.accent4, c.accent4);
            set_color(&mut s.accent5, c.accent5);
            set_color(&mut s.accent6, c.accent6);
            set_color(&mut s.hlink, c.hyperlink);
            set_color(&mut s.fol_hlink, c.followed_hyperlink);
        })
    }

    /// The font scheme of the (first) slide master's theme.
    pub fn theme_fonts(&self) -> Result<ThemeFonts> {
        let theme = self.first_theme()?;
        let s = theme
            .theme_elements
            .as_ref()
            .and_then(|e| e.font_scheme.as_deref());
        Ok(ThemeFonts {
            major: font_set(s.and_then(|s| s.major_font.as_deref())),
            minor: font_set(s.and_then(|s| s.minor_font.as_deref())),
        })
    }

    /// Replaces the heading and body fonts of every slide master's theme
    /// (script-specific supplemental fonts are kept).
    pub fn set_theme_fonts(&mut self, fonts: &ThemeFonts, scheme_name: Option<&str>) -> Result<()> {
        let name = scheme_name.map(str::to_owned);
        self.edit_themes(|e| {
            let s = e.font_scheme.get_or_insert_with(Box::default);
            if let Some(n) = &name {
                s.name = Some(n.clone());
            } else if s.name.is_none() {
                s.name = Some("Custom".into());
            }
            apply_font_set(s.major_font.get_or_insert_with(Box::default), &fonts.major);
            apply_font_set(s.minor_font.get_or_insert_with(Box::default), &fonts.minor);
        })
    }

    fn edit_masters(&mut self, mut f: impl FnMut(&mut pml::CT_SlideMaster)) -> Result<()> {
        for part in self.master_parts() {
            let mut master = read_part(&self.package, &part, &pml::elements::SLD_MASTER)?;
            f(&mut master);
            write_part(
                &mut self.package,
                &part,
                ct::PML_SLIDE_MASTER,
                &pml::elements::SLD_MASTER,
                &master,
            )?;
        }
        Ok(())
    }

    /// Sets the background of every slide master (and so of every slide
    /// that does not override it). Picture fills are not supported here.
    pub fn set_master_background(&mut self, fill: Fill) -> Result<()> {
        let bg = background(&fill);
        self.edit_masters(|m| {
            m.c_sld.get_or_insert_with(Box::default).bg = Some(Box::new(bg.clone()));
        })
    }

    /// The background fill of the first slide master, when it is one [`Fill`] describes.
    pub fn master_background(&self) -> Result<Option<Fill>> {
        let Some(part) = self.master_parts().into_iter().next() else {
            return Ok(None);
        };
        let master = read_part(&self.package, &part, &pml::elements::SLD_MASTER)?;
        Ok(match master.c_sld.and_then(|c| c.bg).and_then(|b| b.background) {
            Some(pml::EG_Background::BgPr(pr)) => pr.fill_properties.as_ref().and_then(Fill::from_dml),
            _ => None,
        })
    }

    /// Which footer placeholders the first slide master shows
    /// `(slide number, footer, date)` according to its `p:hf` (all shown when absent).
    pub fn master_header_footer(&self) -> Result<(bool, bool, bool)> {
        let Some(part) = self.master_parts().into_iter().next() else {
            return Ok((true, true, true));
        };
        let master = read_part(&self.package, &part, &pml::elements::SLD_MASTER)?;
        let hf = master.hf.as_deref();
        Ok((
            hf.and_then(|h| h.sld_num).unwrap_or(true),
            hf.and_then(|h| h.ftr).unwrap_or(true),
            hf.and_then(|h| h.dt).unwrap_or(true),
        ))
    }

    /// Applies header and footer settings to every slide ("Apply to All"):
    /// footer-area placeholders are added to, updated on or removed from
    /// each slide, and `p:hf` of the masters and layouts records the choice.
    ///
    /// ```
    /// use openxml_pptx::{DateField, HeaderFooter, LayoutKind, Presentation};
    ///
    /// let mut deck = Presentation::new();
    /// deck.add_slide(LayoutKind::Title)?;
    /// deck.add_slide(LayoutKind::TitleAndContent)?;
    /// deck.apply_header_footer(&HeaderFooter {
    ///     footer: Some("ACME confidential".into()),
    ///     slide_number: true,
    ///     date: Some(DateField::Fixed("May 2024".into())),
    ///     skip_title_slides: true,
    /// })?;
    /// assert_eq!(deck.slide(0).unwrap().footer_text(), None);
    /// assert_eq!(deck.slide(1).unwrap().footer_text().as_deref(), Some("ACME confidential"));
    /// assert!(deck.slide(1).unwrap().has_slide_number());
    /// # Ok::<(), openxml_core::Error>(())
    /// ```
    pub fn apply_header_footer(&mut self, settings: &HeaderFooter) -> Result<()> {
        for i in 0..self.slides.len() {
            let is_title = self.slides[i]
                .layout
                .as_ref()
                .and_then(|lp| self.layouts.iter().find(|l| &l.part == lp))
                .is_some_and(|l| l.kind() == Some(LayoutKind::Title));
            let skip = settings.skip_title_slides && is_title;
            let mut slide = self.slide_mut(i).expect("in range");
            slide.set_footer_text(if skip { None } else { settings.footer.as_deref() })?;
            slide.set_slide_number(!skip && settings.slide_number)?;
            slide.set_date(if skip { None } else { settings.date.clone() })?;
        }
        let hf = pml::CT_HeaderFooter {
            sld_num: (!settings.slide_number).then_some(false),
            hdr: Some(false),
            ftr: settings.footer.is_none().then_some(false),
            dt: settings.date.is_none().then_some(false),
            ..Default::default()
        };
        self.edit_masters(|m| m.hf = Some(Box::new(hf.clone())))?;
        for layout in &mut self.layouts {
            if layout.data.hf.is_some() {
                layout.data.hf = Some(Box::new(hf.clone()));
                layout.dirty = true;
            }
        }
        Ok(())
    }
}

/// A background (`p:bg`) with the given fill.
pub(crate) fn background(fill: &Fill) -> pml::CT_Background {
    pml::CT_Background {
        background: Some(pml::EG_Background::BgPr(Box::new(pml::CT_BackgroundProperties {
            fill_properties: Some(fill.to_dml()),
            effect_properties: Some(dml::EG_EffectProperties::EffectLst(Box::default())),
            ..Default::default()
        }))),
        ..Default::default()
    }
}

fn footer_placeholder_text(slide: &Slide, kind: PlaceholderKind) -> Option<String> {
    let tree = slide.data.c_sld.as_ref()?.sp_tree.as_deref()?;
    tree.choice.iter().find_map(|c| match c {
        pml::CT_GroupShape_Choice::Sp(sp) if shape::shape_placeholder(sp).is_some_and(|p| p.kind == kind) => {
            Some(sp.tx_body.as_deref().map(text::body_text).unwrap_or_default())
        }
        _ => None,
    })
}

impl Slide {
    /// Text of the footer placeholder, when the slide has one.
    pub fn footer_text(&self) -> Option<String> {
        footer_placeholder_text(self, PlaceholderKind::Footer)
    }

    /// Whether the slide shows a slide number placeholder.
    pub fn has_slide_number(&self) -> bool {
        footer_placeholder_text(self, PlaceholderKind::SlideNumber).is_some()
    }

    /// Text of the date placeholder, when the slide has one.
    pub fn date_text(&self) -> Option<String> {
        footer_placeholder_text(self, PlaceholderKind::Date)
    }

    /// The fill set on the slide's own background, when it is one [`Fill`] describes.
    pub fn background_fill(&self) -> Option<Fill> {
        match self.data.c_sld.as_ref()?.bg.as_ref()?.background.as_ref()? {
            pml::EG_Background::BgPr(pr) => Fill::from_dml(pr.fill_properties.as_ref()?),
            _ => None,
        }
    }
}

fn field_paragraph(id: &str, kind: &str, shown: &str) -> dml::CT_TextParagraph {
    crate::util::fragment(&format!(
        r#"<a:p><a:fld id="{id}" type="{kind}"><a:rPr lang="en-US"/><a:t>{shown}</a:t></a:fld><a:endParaRPr lang="en-US"/></a:p>"#,
        shown = crate::util::xml_escape(shown)
    ))
}

/// The id of the first field of `kind` in a layout placeholder, to reuse it.
fn layout_field_id(sp: &pml::CT_Shape, kind: &str) -> Option<String> {
    sp.tx_body
        .as_ref()?
        .p
        .iter()
        .flat_map(|p| &p.text_run)
        .find_map(|r| match r {
            dml::EG_TextRun::Fld(f) if f.type_.as_deref() == Some(kind) => f.id.clone(),
            _ => None,
        })
}

impl SlideMut<'_> {
    fn remove_placeholder(&mut self, kind: PlaceholderKind) {
        self.tree_mut().choice.retain(|c| {
            !matches!(c, pml::CT_GroupShape_Choice::Sp(sp) if shape::shape_placeholder(sp).is_some_and(|p| p.kind == kind))
        });
    }

    fn layout_field(&self, kind: PlaceholderKind, field: &str) -> Option<String> {
        let layout = self
            .pres
            .layouts
            .iter()
            .find(|l| Some(&l.part) == self.layout.as_ref())?;
        layout
            .placeholder_shapes()
            .filter(|sp| shape::shape_placeholder(sp).is_some_and(|p| p.kind == kind))
            .find_map(|sp| layout_field_id(sp, field))
    }

    /// Shows footer text (copying the layout's footer placeholder) or, with
    /// `None`, removes the footer from this slide.
    pub fn set_footer_text(&mut self, footer: Option<&str>) -> Result<()> {
        match footer {
            None => self.remove_placeholder(PlaceholderKind::Footer),
            Some(t) => {
                self.placeholder("footer", |k| k == PlaceholderKind::Footer)?
                    .set_text(t);
            }
        }
        Ok(())
    }

    /// Shows or removes the slide number (a `slidenum` field in the layout's
    /// slide number placeholder).
    pub fn set_slide_number(&mut self, on: bool) -> Result<()> {
        if !on {
            self.remove_placeholder(PlaceholderKind::SlideNumber);
            return Ok(());
        }
        let id = self
            .layout_field(PlaceholderKind::SlideNumber, "slidenum")
            .unwrap_or_else(|| SLIDE_NUMBER_FIELD_ID.to_owned());
        let number = (self.index + 1).to_string();
        let mut sp = self.placeholder("slide number", |k| k == PlaceholderKind::SlideNumber)?;
        text::set_paragraphs(sp.body(), vec![field_paragraph(&id, "slidenum", &number)]);
        Ok(())
    }

    /// Shows a date (fixed text or an automatically updated field) or, with
    /// `None`, removes the date from this slide.
    pub fn set_date(&mut self, date: Option<DateField>) -> Result<()> {
        match date {
            None => self.remove_placeholder(PlaceholderKind::Date),
            Some(DateField::Fixed(t)) => {
                self.placeholder("date", |k| k == PlaceholderKind::Date)?
                    .set_text(&t);
            }
            Some(DateField::Automatic) => {
                let id = self
                    .layout_field(PlaceholderKind::Date, "datetime1")
                    .unwrap_or_else(|| DATE_FIELD_ID.to_owned());
                let today = openxml_opc::w3cdtf_now();
                let shown = match (today.get(5..7), today.get(8..10), today.get(0..4)) {
                    (Some(m), Some(d), Some(y)) => {
                        format!("{}/{}/{y}", m.trim_start_matches('0'), d.trim_start_matches('0'))
                    }
                    _ => String::new(),
                };
                let mut sp = self.placeholder("date", |k| k == PlaceholderKind::Date)?;
                text::set_paragraphs(sp.body(), vec![field_paragraph(&id, "datetime1", &shown)]);
            }
        }
        Ok(())
    }

    /// Fills the slide background (overriding the master), e.g. with a gradient.
    pub fn set_background(&mut self, fill: Fill) {
        let c_sld = self.raw_mut().c_sld.get_or_insert_with(Box::default);
        c_sld.bg = Some(Box::new(background(&fill)));
    }

    /// Removes the slide's own background so the master's shows through.
    pub fn clear_background(&mut self) {
        if let Some(c) = self.raw_mut().c_sld.as_mut() {
            c.bg = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_fonts_helpers() {
        let mut slot = None;
        set_color(&mut slot, Rgb(1, 2, 3));
        assert_eq!(rgb_of(slot.as_deref()), Rgb(1, 2, 3));
        let sys = Some(Box::new(dml::CT_Color {
            color_choice: Some(dml::EG_ColorChoice::SysClr(Box::new(dml::CT_SystemColor {
                val: Some(dml::ST_SystemColorVal::WindowText),
                last_clr: Some(openxml_xml::HexBinary(vec![0, 0, 0])),
                ..Default::default()
            }))),
            ..Default::default()
        }));
        let mut kept = sys.clone();
        set_color(&mut kept, Rgb::BLACK);
        assert_eq!(kept, sys, "unchanged colours keep their definition");
        let mut c = dml::CT_FontCollection::default();
        let f = FontSet {
            latin: "Aptos".into(),
            east_asian: "".into(),
            complex_script: "".into(),
        };
        apply_font_set(&mut c, &f);
        assert_eq!(font_set(Some(&c)), f);
        let p = field_paragraph("{X}", "slidenum", "<3>");
        assert_eq!(text::paragraph_text(&p), "<3>");
    }
}
