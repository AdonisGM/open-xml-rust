//! Paragraph and run formatting inside text bodies.

use openxml_core::{FontSize, Length};
use openxml_schema::dml;

pub use openxml_schema::dml::ST_TextAutonumberScheme as AutoNumberScheme;

use crate::drawing::{Color, Shadow};
use crate::format::{percent_string, thousandths_percent};
use crate::text::{self, Alignment};

/// The bullet of a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bullet {
    /// No bullet (overrides an inherited one).
    None,
    /// A character bullet, optionally in a specific font (e.g. `Wingdings`).
    Char {
        /// The bullet character.
        char: char,
        /// Typeface of the bullet; `None` follows the text.
        font: Option<String>,
    },
    /// Automatic numbering.
    Numbered {
        /// Numbering scheme, e.g. [`AutoNumberScheme::ArabicPeriod`] (`1.`, `2.`, …).
        scheme: AutoNumberScheme,
        /// First number (1–32767).
        start_at: u32,
    },
}

impl Bullet {
    /// Arabic numbers followed by a period, starting at 1.
    pub fn numbered() -> Bullet {
        Bullet::Numbered {
            scheme: AutoNumberScheme::ArabicPeriod,
            start_at: 1,
        }
    }
}

/// Line or paragraph spacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    /// A multiple of the line height (`1.0` = single spacing).
    Lines(f64),
    /// An absolute amount in points.
    Points(f64),
}

impl Spacing {
    fn to_dml(self) -> dml::CT_TextSpacing {
        let choice = match self {
            Spacing::Lines(v) => dml::CT_TextSpacing_Choice::SpcPct(Box::new(dml::CT_TextSpacingPercent {
                val: Some(dml::ST_TextSpacingPercentOrPercentString::TextSpacingPercent(
                    thousandths_percent(v.clamp(0.0, 132.0)),
                )),
                ..Default::default()
            })),
            Spacing::Points(p) => dml::CT_TextSpacing_Choice::SpcPts(Box::new(dml::CT_TextSpacingPoint {
                val: Some((p * 100.0).round().clamp(0.0, 158_400.0) as i32),
                ..Default::default()
            })),
        };
        dml::CT_TextSpacing {
            choice: Some(choice),
            ..Default::default()
        }
    }

    fn from_dml(s: &dml::CT_TextSpacing) -> Option<Spacing> {
        Some(match s.choice.as_ref()? {
            dml::CT_TextSpacing_Choice::SpcPct(p) => Spacing::Lines(match p.val.as_ref()? {
                dml::ST_TextSpacingPercentOrPercentString::TextSpacingPercent(v) => f64::from(*v) / 100_000.0,
                dml::ST_TextSpacingPercentOrPercentString::Percentage(s) => percent_string(s)?,
            }),
            dml::CT_TextSpacing_Choice::SpcPts(p) => Spacing::Points(f64::from(p.val?) / 100.0),
            dml::CT_TextSpacing_Choice::Other(_) => return None,
        })
    }
}

fn margin(v: Length) -> i32 {
    v.as_emu().clamp(-51_206_400, 51_206_400) as i32
}

/// Mutable access to a paragraph (`a:p`).
pub struct ParagraphMut<'a> {
    p: &'a mut dml::CT_TextParagraph,
}

impl<'a> ParagraphMut<'a> {
    pub(crate) fn new(p: &'a mut dml::CT_TextParagraph) -> Self {
        ParagraphMut { p }
    }

    fn props(&mut self) -> &mut dml::CT_TextParagraphProperties {
        self.p.p_pr.get_or_insert_with(Box::default)
    }

    /// Text of the paragraph (line breaks as `\n`).
    pub fn text(&self) -> String {
        text::paragraph_text(self.p)
    }

    /// Replaces the runs with a single run, keeping the formatting of the first run.
    pub fn set_text(&mut self, value: &str) -> &mut Self {
        let props = self.p.text_run.iter().find_map(|r| match r {
            dml::EG_TextRun::R(run) => run.r_pr.clone(),
            _ => None,
        });
        let mut fresh = text::paragraph(value, 0);
        if let (Some(props), Some(dml::EG_TextRun::R(run))) = (props, fresh.text_run.first_mut()) {
            run.r_pr = Some(props);
        }
        self.p.text_run = fresh.text_run;
        self
    }

    /// Outline level (0 = top level).
    pub fn level(&self) -> u8 {
        self.p.p_pr.as_ref().and_then(|p| p.lvl).unwrap_or(0).clamp(0, 8) as u8
    }

    /// Sets the outline level (0–8).
    pub fn set_level(&mut self, level: u8) -> &mut Self {
        self.props().lvl = (level > 0).then(|| i32::from(level.min(8)));
        self
    }

    /// Horizontal alignment, when set on the paragraph.
    pub fn alignment(&self) -> Option<Alignment> {
        Alignment::from_dml(self.p.p_pr.as_ref()?.algn?)
    }

    /// Sets the horizontal alignment.
    pub fn set_alignment(&mut self, alignment: Alignment) -> &mut Self {
        self.props().algn = Some(alignment.to_dml());
        self
    }

    /// Sets the bullet.
    pub fn set_bullet(&mut self, bullet: Bullet) -> &mut Self {
        let pr = self.props();
        match bullet {
            Bullet::None => {
                pr.text_bullet_typeface = None;
                pr.text_bullet = Some(dml::EG_TextBullet::BuNone(Box::default()));
            }
            Bullet::Char { char, font } => {
                pr.text_bullet_typeface =
                    font.map(|f| dml::EG_TextBulletTypeface::BuFont(Box::new(text::font(&f))));
                pr.text_bullet = Some(dml::EG_TextBullet::BuChar(Box::new(dml::CT_TextCharBullet {
                    char: Some(char.to_string()),
                    ..Default::default()
                })));
            }
            Bullet::Numbered { scheme, start_at } => {
                pr.text_bullet_typeface = None;
                pr.text_bullet = Some(dml::EG_TextBullet::BuAutoNum(Box::new(
                    dml::CT_TextAutonumberBullet {
                        type_: Some(scheme),
                        start_at: (start_at != 1).then(|| start_at.clamp(1, 32_767) as i32),
                        ..Default::default()
                    },
                )));
            }
        }
        self
    }

    /// The bullet set on the paragraph itself.
    pub fn bullet(&self) -> Option<Bullet> {
        let pr = self.p.p_pr.as_ref()?;
        Some(match pr.text_bullet.as_ref()? {
            dml::EG_TextBullet::BuNone(_) => Bullet::None,
            dml::EG_TextBullet::BuChar(c) => Bullet::Char {
                char: c.char.as_deref()?.chars().next()?,
                font: match &pr.text_bullet_typeface {
                    Some(dml::EG_TextBulletTypeface::BuFont(f)) => f.typeface.clone(),
                    _ => None,
                },
            },
            dml::EG_TextBullet::BuAutoNum(a) => Bullet::Numbered {
                scheme: a.type_?,
                start_at: a.start_at.map_or(1, |s| s.max(1) as u32),
            },
            _ => return None,
        })
    }

    /// Sets the colour of the bullet.
    pub fn set_bullet_color(&mut self, color: impl Into<Color>) -> &mut Self {
        self.props().text_bullet_color = Some(dml::EG_TextBulletColor::BuClr(Box::new(color.into().to_ct())));
        self
    }

    /// Sets the left margin of the text and the indent of the first line
    /// relative to it (negative for a hanging bullet).
    pub fn set_indent(&mut self, left_margin: Length, first_line: Length) -> &mut Self {
        let pr = self.props();
        pr.mar_l = Some(margin(left_margin));
        pr.indent = Some(margin(first_line));
        self
    }

    /// Left margin and first-line indent, when set on the paragraph.
    pub fn indent(&self) -> Option<(Length, Length)> {
        let pr = self.p.p_pr.as_ref()?;
        Some((
            Length::emu(i64::from(pr.mar_l?)),
            Length::emu(i64::from(pr.indent.unwrap_or(0))),
        ))
    }

    /// Sets the line spacing.
    pub fn set_line_spacing(&mut self, spacing: Spacing) -> &mut Self {
        self.props().ln_spc = Some(Box::new(spacing.to_dml()));
        self
    }

    /// Line spacing, when set on the paragraph.
    pub fn line_spacing(&self) -> Option<Spacing> {
        Spacing::from_dml(self.p.p_pr.as_ref()?.ln_spc.as_deref()?)
    }

    /// Sets the space before the paragraph.
    pub fn set_space_before(&mut self, spacing: Spacing) -> &mut Self {
        self.props().spc_bef = Some(Box::new(spacing.to_dml()));
        self
    }

    /// Space before the paragraph, when set.
    pub fn space_before(&self) -> Option<Spacing> {
        Spacing::from_dml(self.p.p_pr.as_ref()?.spc_bef.as_deref()?)
    }

    /// Sets the space after the paragraph.
    pub fn set_space_after(&mut self, spacing: Spacing) -> &mut Self {
        self.props().spc_aft = Some(Box::new(spacing.to_dml()));
        self
    }

    /// Space after the paragraph, when set.
    pub fn space_after(&self) -> Option<Spacing> {
        Spacing::from_dml(self.p.p_pr.as_ref()?.spc_aft.as_deref()?)
    }

    /// Sets right-to-left reading order.
    pub fn set_right_to_left(&mut self, rtl: bool) -> &mut Self {
        self.props().rtl = rtl.then_some(true);
        self
    }

    /// Appends a run of text and returns it for formatting.
    pub fn add_run(&mut self, value: &str) -> RunMut<'_> {
        self.p
            .text_run
            .push(dml::EG_TextRun::R(Box::new(dml::CT_RegularTextRun {
                r_pr: Some(Box::new(text::lang_props())),
                t: Some(value.to_owned()),
                ..Default::default()
            })));
        match self.p.text_run.last_mut() {
            Some(dml::EG_TextRun::R(r)) => RunMut::new(r),
            _ => unreachable!("just pushed a run"),
        }
    }

    /// Appends a line break (`a:br`).
    pub fn add_line_break(&mut self) -> &mut Self {
        self.p
            .text_run
            .push(dml::EG_TextRun::Br(Box::new(dml::CT_TextLineBreak {
                r_pr: Some(Box::new(text::lang_props())),
                ..Default::default()
            })));
        self
    }

    /// Number of text runs (`a:r`).
    pub fn run_count(&self) -> usize {
        self.p
            .text_run
            .iter()
            .filter(|r| matches!(r, dml::EG_TextRun::R(_)))
            .count()
    }

    /// The text run at `index` (counting `a:r` elements only).
    pub fn run_mut(&mut self, index: usize) -> Option<RunMut<'_>> {
        self.p
            .text_run
            .iter_mut()
            .filter_map(|r| match r {
                dml::EG_TextRun::R(r) => Some(r),
                _ => None,
            })
            .nth(index)
            .map(|r| RunMut::new(r))
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &dml::CT_TextParagraph {
        self.p
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut dml::CT_TextParagraph {
        self.p
    }
}

/// Mutable access to a text run (`a:r`).
pub struct RunMut<'a> {
    r: &'a mut dml::CT_RegularTextRun,
}

impl<'a> RunMut<'a> {
    pub(crate) fn new(r: &'a mut dml::CT_RegularTextRun) -> Self {
        RunMut { r }
    }

    fn props(&mut self) -> &mut dml::CT_TextCharacterProperties {
        self.r.r_pr.get_or_insert_with(|| Box::new(text::lang_props()))
    }

    fn props_ref(&self) -> Option<&dml::CT_TextCharacterProperties> {
        self.r.r_pr.as_deref()
    }

    /// Text of the run.
    pub fn text(&self) -> &str {
        self.r.t.as_deref().unwrap_or("")
    }

    /// Replaces the text of the run.
    pub fn set_text(&mut self, value: &str) -> &mut Self {
        self.r.t = Some(value.to_owned());
        self
    }

    /// Sets the font size.
    pub fn size(&mut self, size: FontSize) -> &mut Self {
        self.props().sz = Some(size.hundredths());
        self
    }

    /// Font size, when set on the run.
    pub fn font_size(&self) -> Option<FontSize> {
        self.props_ref()?.sz.map(|v| FontSize(f64::from(v) / 100.0))
    }

    /// Sets or clears bold.
    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.props().b = Some(on);
        self
    }

    /// Whether the run is bold.
    pub fn is_bold(&self) -> bool {
        self.props_ref().and_then(|p| p.b).unwrap_or(false)
    }

    /// Sets or clears italic.
    pub fn italic(&mut self, on: bool) -> &mut Self {
        self.props().i = Some(on);
        self
    }

    /// Whether the run is italic.
    pub fn is_italic(&self) -> bool {
        self.props_ref().and_then(|p| p.i).unwrap_or(false)
    }

    /// Sets or clears single underline.
    pub fn underline(&mut self, on: bool) -> &mut Self {
        self.props().u = Some(if on {
            dml::ST_TextUnderlineType::Sng
        } else {
            dml::ST_TextUnderlineType::None
        });
        self
    }

    /// Sets or clears single strike-through.
    pub fn strike(&mut self, on: bool) -> &mut Self {
        self.props().strike = Some(if on {
            dml::ST_TextStrikeType::SngStrike
        } else {
            dml::ST_TextStrikeType::NoStrike
        });
        self
    }

    /// Sets the text colour.
    pub fn color(&mut self, color: impl Into<Color>) -> &mut Self {
        let c = color.into();
        self.props().fill_properties = Some(dml::EG_FillProperties::SolidFill(Box::new(
            dml::CT_SolidColorFillProperties {
                color_choice: Some(c.to_dml()),
                ..Default::default()
            },
        )));
        self
    }

    /// Text colour, when set on the run.
    pub fn text_color(&self) -> Option<Color> {
        match self.props_ref()?.fill_properties.as_ref()? {
            dml::EG_FillProperties::SolidFill(s) => Color::from_dml(s.color_choice.as_ref()?),
            _ => None,
        }
    }

    /// Highlights the text with a background colour.
    pub fn highlight(&mut self, color: impl Into<Color>) -> &mut Self {
        self.props().highlight = Some(Box::new(color.into().to_ct()));
        self
    }

    /// The highlight colour, when set.
    pub fn highlight_color(&self) -> Option<Color> {
        Color::from_dml(self.props_ref()?.highlight.as_ref()?.color_choice.as_ref()?)
    }

    /// Sets the Latin typeface.
    pub fn font(&mut self, typeface: &str) -> &mut Self {
        self.props().latin = Some(Box::new(text::font(typeface)));
        self
    }

    /// Sets the East Asian typeface.
    pub fn east_asian_font(&mut self, typeface: &str) -> &mut Self {
        self.props().ea = Some(Box::new(text::font(typeface)));
        self
    }

    /// Sets the complex-script typeface (Arabic, Hebrew, Thai, …).
    pub fn complex_script_font(&mut self, typeface: &str) -> &mut Self {
        self.props().cs = Some(Box::new(text::font(typeface)));
        self
    }

    /// Typefaces `(latin, east_asian, complex_script)` set on the run.
    pub fn fonts(&self) -> (Option<&str>, Option<&str>, Option<&str>) {
        fn face(f: Option<&dml::CT_TextFont>) -> Option<&str> {
            f.and_then(|f| f.typeface.as_deref())
        }
        let p = self.props_ref();
        (
            face(p.and_then(|p| p.latin.as_deref())),
            face(p.and_then(|p| p.ea.as_deref())),
            face(p.and_then(|p| p.cs.as_deref())),
        )
    }

    /// Raises (positive) or lowers (negative) the text by a fraction of the
    /// font size: `0.3` for superscript, `-0.25` for subscript, `0.0` for normal.
    pub fn baseline(&mut self, offset: f64) -> &mut Self {
        self.props().baseline = (offset != 0.0).then(|| crate::drawing::percentage(offset));
        self
    }

    /// Sets the language tag, e.g. `vi-VN`.
    pub fn language(&mut self, lang: &str) -> &mut Self {
        self.props().lang = Some(lang.to_owned());
        self
    }

    /// Sets the spacing between characters, in points (negative condenses).
    pub fn character_spacing(&mut self, points: f64) -> &mut Self {
        self.props().spc = Some(dml::ST_TextPoint::TextPointUnqualified(
            (points * 100.0).round().clamp(-400_000.0, 400_000.0) as i32,
        ));
        self
    }

    /// Adds an outer shadow to the text (`None` removes text effects).
    pub fn shadow(&mut self, shadow: Option<Shadow>) -> &mut Self {
        self.props().effect_properties = shadow.map(Shadow::effect_list);
        self
    }

    /// The underlying schema type.
    pub fn raw(&self) -> &dml::CT_RegularTextRun {
        self.r
    }

    /// The underlying schema type, mutably.
    pub fn raw_mut(&mut self) -> &mut dml::CT_RegularTextRun {
        self.r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::SchemeColor;
    use crate::text::Rgb;

    #[test]
    fn paragraph_formatting() {
        let mut p = text::paragraph("hello", 0);
        let mut m = ParagraphMut::new(&mut p);
        assert_eq!(m.level(), 0);
        m.set_level(12);
        assert_eq!(m.level(), 8);
        m.set_level(0);
        assert_eq!(m.raw().p_pr.as_ref().unwrap().lvl, None);
        assert_eq!(m.alignment(), None);
        m.set_alignment(Alignment::Right);
        assert_eq!(m.alignment(), Some(Alignment::Right));
        assert_eq!(m.bullet(), None);
        for b in [
            Bullet::None,
            Bullet::Char {
                char: '§',
                font: Some("Wingdings".into()),
            },
            Bullet::Char {
                char: '-',
                font: None,
            },
            Bullet::numbered(),
            Bullet::Numbered {
                scheme: AutoNumberScheme::RomanUcPeriod,
                start_at: 4,
            },
        ] {
            m.set_bullet(b.clone());
            assert_eq!(m.bullet(), Some(b));
        }
        m.set_bullet_color(SchemeColor::Accent2);
        assert_eq!(m.indent(), None);
        m.set_indent(Length::cm(1.0), Length::cm(-0.5));
        assert_eq!(m.indent(), Some((Length::cm(1.0), Length::cm(-0.5))));
        m.set_line_spacing(Spacing::Lines(1.5))
            .set_space_before(Spacing::Points(6.0))
            .set_space_after(Spacing::Points(12.5))
            .set_right_to_left(true);
        assert_eq!(m.line_spacing(), Some(Spacing::Lines(1.5)));
        assert_eq!(m.space_before(), Some(Spacing::Points(6.0)));
        assert_eq!(m.space_after(), Some(Spacing::Points(12.5)));
        m.set_text("replaced");
        assert_eq!(m.text(), "replaced");
        m.add_line_break();
        m.add_run("more");
        assert_eq!(m.text(), "replaced\nmore");
        assert_eq!(m.run_count(), 2);
        assert_eq!(m.run_mut(1).unwrap().text(), "more");
        assert!(m.run_mut(2).is_none());
        assert!(m.raw_mut().p_pr.is_some());
    }

    #[test]
    fn run_formatting() {
        let mut p = text::paragraph("", 0);
        let mut para = ParagraphMut::new(&mut p);
        let mut r = para.add_run("x");
        assert!(!r.is_bold() && !r.is_italic());
        assert_eq!(r.font_size(), None);
        r.size(FontSize(18.0))
            .bold(true)
            .italic(true)
            .underline(true)
            .strike(true)
            .color(Rgb(9, 8, 7))
            .highlight(Rgb::WHITE)
            .font("Calibri")
            .east_asian_font("MS Mincho")
            .complex_script_font("Arial")
            .baseline(0.3)
            .language("vi-VN")
            .character_spacing(-1.0)
            .shadow(Some(Shadow::default()));
        assert!(r.is_bold() && r.is_italic());
        assert_eq!(r.font_size(), Some(FontSize(18.0)));
        assert_eq!(r.text_color(), Some(Color::Rgb(Rgb(9, 8, 7))));
        assert_eq!(r.highlight_color(), Some(Color::Rgb(Rgb::WHITE)));
        assert_eq!(r.fonts(), (Some("Calibri"), Some("MS Mincho"), Some("Arial")));
        r.underline(false).strike(false).baseline(0.0).set_text("y");
        assert_eq!(r.text(), "y");
        let props = r.raw().r_pr.as_ref().unwrap();
        assert_eq!(props.baseline, None);
        assert_eq!(props.lang.as_deref(), Some("vi-VN"));
        assert!(r.raw_mut().t.is_some());
    }

    #[test]
    fn spacing_parsing() {
        let pct = dml::CT_TextSpacing {
            choice: Some(dml::CT_TextSpacing_Choice::SpcPct(Box::new(
                dml::CT_TextSpacingPercent {
                    val: Some(dml::ST_TextSpacingPercentOrPercentString::Percentage(
                        "150%".into(),
                    )),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        };
        assert_eq!(Spacing::from_dml(&pct), Some(Spacing::Lines(1.5)));
        assert_eq!(Spacing::from_dml(&dml::CT_TextSpacing::default()), None);
    }
}
