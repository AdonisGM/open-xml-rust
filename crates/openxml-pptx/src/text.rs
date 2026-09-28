//! DrawingML text: colours, alignment, extraction and construction of text bodies.

use std::fmt;

use openxml_schema::dml;
use openxml_xml::HexBinary;

/// An RGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Black.
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    /// White.
    pub const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

    /// Parses `RRGGBB` or `#RRGGBB`.
    ///
    /// ```
    /// use openxml_pptx::Rgb;
    /// assert_eq!(Rgb::from_hex("#1F4E79"), Some(Rgb(0x1F, 0x4E, 0x79)));
    /// assert_eq!(Rgb::from_hex("nope"), None);
    /// ```
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    /// The colour as `RRGGBB`.
    pub fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    pub(crate) fn srgb(self) -> dml::EG_ColorChoice {
        dml::EG_ColorChoice::SrgbClr(Box::new(dml::CT_SRgbColor {
            val: Some(HexBinary(vec![self.0, self.1, self.2])),
            ..Default::default()
        }))
    }

    pub(crate) fn solid_fill(self) -> dml::EG_FillProperties {
        dml::EG_FillProperties::SolidFill(Box::new(dml::CT_SolidColorFillProperties {
            color_choice: Some(self.srgb()),
            ..Default::default()
        }))
    }

    pub(crate) fn from_fill(fill: &dml::EG_FillProperties) -> Option<Rgb> {
        let dml::EG_FillProperties::SolidFill(solid) = fill else {
            return None;
        };
        let Some(dml::EG_ColorChoice::SrgbClr(c)) = &solid.color_choice else {
            return None;
        };
        let bytes = c.val.as_ref()?.as_bytes();
        (bytes.len() == 3).then(|| Rgb(bytes[0], bytes[1], bytes[2]))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.hex())
    }
}

/// Horizontal alignment of paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// Left aligned.
    Left,
    /// Centred.
    Center,
    /// Right aligned.
    Right,
    /// Justified.
    Justify,
    /// Distributed across the line.
    Distributed,
}

impl Alignment {
    pub(crate) fn to_dml(self) -> dml::ST_TextAlignType {
        match self {
            Alignment::Left => dml::ST_TextAlignType::L,
            Alignment::Center => dml::ST_TextAlignType::Ctr,
            Alignment::Right => dml::ST_TextAlignType::R,
            Alignment::Justify => dml::ST_TextAlignType::Just,
            Alignment::Distributed => dml::ST_TextAlignType::Dist,
        }
    }

    pub(crate) fn from_dml(a: dml::ST_TextAlignType) -> Option<Alignment> {
        Some(match a {
            dml::ST_TextAlignType::L => Alignment::Left,
            dml::ST_TextAlignType::Ctr => Alignment::Center,
            dml::ST_TextAlignType::R => Alignment::Right,
            dml::ST_TextAlignType::Just => Alignment::Justify,
            dml::ST_TextAlignType::Dist => Alignment::Distributed,
            _ => return None,
        })
    }
}

/// Text of a paragraph: runs and fields concatenated, line breaks as `\n`.
pub(crate) fn paragraph_text(p: &dml::CT_TextParagraph) -> String {
    let mut out = String::new();
    for r in &p.text_run {
        match r {
            dml::EG_TextRun::R(run) => out.push_str(run.t.as_deref().unwrap_or("")),
            dml::EG_TextRun::Br(_) => out.push('\n'),
            dml::EG_TextRun::Fld(f) => out.push_str(f.t.as_deref().unwrap_or("")),
            dml::EG_TextRun::Other(raw) => out.push_str(&raw.text()),
        }
    }
    out
}

/// Text of a text body, paragraphs separated by `\n`.
pub(crate) fn body_text(body: &dml::CT_TextBody) -> String {
    body.p.iter().map(paragraph_text).collect::<Vec<_>>().join("\n")
}

/// Run properties with only the language set, as PowerPoint writes them.
pub(crate) fn lang_props() -> dml::CT_TextCharacterProperties {
    dml::CT_TextCharacterProperties {
        lang: Some("en-US".into()),
        ..Default::default()
    }
}

/// A paragraph with a single run (or none for empty text) at the given
/// outline level (0 = top level).
pub(crate) fn paragraph(text: &str, level: u8) -> dml::CT_TextParagraph {
    let p_pr = (level > 0).then(|| {
        Box::new(dml::CT_TextParagraphProperties {
            lvl: Some(i32::from(level.min(8))),
            ..Default::default()
        })
    });
    let text_run = if text.is_empty() {
        Vec::new()
    } else {
        vec![dml::EG_TextRun::R(Box::new(dml::CT_RegularTextRun {
            r_pr: Some(Box::new(lang_props())),
            t: Some(text.to_owned()),
            ..Default::default()
        }))]
    };
    dml::CT_TextParagraph {
        p_pr,
        text_run,
        end_para_r_pr: Some(Box::new(lang_props())),
        ..Default::default()
    }
}

/// One paragraph per line of `text`.
pub(crate) fn paragraphs_from_text(text: &str) -> Vec<dml::CT_TextParagraph> {
    text.split('\n')
        .map(|line| paragraph(line.trim_end_matches('\r'), 0))
        .collect()
}

/// Replaces the paragraphs of a body; a body always keeps at least one paragraph.
pub(crate) fn set_paragraphs(body: &mut dml::CT_TextBody, mut paragraphs: Vec<dml::CT_TextParagraph>) {
    if paragraphs.is_empty() {
        paragraphs.push(paragraph("", 0));
    }
    body.p = paragraphs;
}

/// A text body with default body properties and the given paragraphs.
pub(crate) fn text_body(paragraphs: Vec<dml::CT_TextParagraph>) -> dml::CT_TextBody {
    let mut body = dml::CT_TextBody {
        body_pr: Some(Box::default()),
        lst_style: Some(Box::default()),
        ..Default::default()
    };
    set_paragraphs(&mut body, paragraphs);
    body
}

/// Applies `f` to the properties of every run and of every end-of-paragraph mark.
pub(crate) fn for_each_run_props(
    body: &mut dml::CT_TextBody,
    mut f: impl FnMut(&mut dml::CT_TextCharacterProperties),
) {
    for p in &mut body.p {
        for r in &mut p.text_run {
            let props = match r {
                dml::EG_TextRun::R(run) => &mut run.r_pr,
                dml::EG_TextRun::Br(br) => &mut br.r_pr,
                dml::EG_TextRun::Fld(fld) => &mut fld.r_pr,
                dml::EG_TextRun::Other(_) => continue,
            };
            f(props.get_or_insert_with(|| Box::new(lang_props())));
        }
        f(p.end_para_r_pr.get_or_insert_with(|| Box::new(lang_props())));
    }
}

/// Applies `f` to the properties of every paragraph.
pub(crate) fn for_each_paragraph_props(
    body: &mut dml::CT_TextBody,
    mut f: impl FnMut(&mut dml::CT_TextParagraphProperties),
) {
    for p in &mut body.p {
        f(p.p_pr.get_or_insert_with(Box::default));
    }
}

/// Replaces `from` with `to` inside individual runs; returns the number of replacements.
pub(crate) fn replace_in_body(body: &mut dml::CT_TextBody, from: &str, to: &str) -> usize {
    if from.is_empty() {
        return 0;
    }
    let mut count = 0;
    for p in &mut body.p {
        for r in &mut p.text_run {
            if let dml::EG_TextRun::R(run) = r
                && let Some(t) = &mut run.t
            {
                let n = t.matches(from).count();
                if n > 0 {
                    *t = t.replace(from, to);
                    count += n;
                }
            }
        }
    }
    count
}

/// A font typeface reference.
pub(crate) fn font(name: &str) -> dml::CT_TextFont {
    dml::CT_TextFont {
        typeface: Some(name.to_owned()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Rgb::from_hex("FF8000"), Some(Rgb(255, 128, 0)));
        assert_eq!(Rgb::from_hex("#ff8000"), Some(Rgb(255, 128, 0)));
        assert_eq!(Rgb::from_hex("FF80"), None);
        assert_eq!(Rgb::from_hex("GG0000"), None);
        assert_eq!(Rgb::from_hex("ÿÿÿ"), None);
        assert_eq!(Rgb(1, 2, 3).hex(), "010203");
        assert_eq!(Rgb::WHITE.to_string(), "#FFFFFF");
        let fill = Rgb(10, 20, 30).solid_fill();
        assert_eq!(Rgb::from_fill(&fill), Some(Rgb(10, 20, 30)));
        let none = dml::EG_FillProperties::NoFill(Box::default());
        assert_eq!(Rgb::from_fill(&none), None);
    }

    #[test]
    fn alignment_round_trip() {
        for a in [
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
            Alignment::Distributed,
        ] {
            assert_eq!(Alignment::from_dml(a.to_dml()), Some(a));
        }
        assert_eq!(Alignment::from_dml(dml::ST_TextAlignType::JustLow), None);
    }

    #[test]
    fn building_and_reading_text() {
        let mut body = text_body(paragraphs_from_text("one\r\ntwo\n"));
        assert_eq!(body.p.len(), 3);
        assert_eq!(body_text(&body), "one\ntwo\n");
        body.p[0].text_run.push(dml::EG_TextRun::Br(Box::default()));
        body.p[0]
            .text_run
            .push(dml::EG_TextRun::Fld(Box::new(dml::CT_TextField {
                t: Some("7".into()),
                ..Default::default()
            })));
        assert_eq!(paragraph_text(&body.p[0]), "one\n7");
        let leveled = paragraph("x", 2);
        assert_eq!(leveled.p_pr.as_ref().unwrap().lvl, Some(2));
        assert!(paragraph("", 0).text_run.is_empty());
        set_paragraphs(&mut body, Vec::new());
        assert_eq!(body.p.len(), 1, "a text body keeps one paragraph");
    }

    #[test]
    fn run_properties_and_replacement() {
        let mut body = text_body(paragraphs_from_text("hello hello\nworld"));
        let mut n = 0;
        for_each_run_props(&mut body, |p| {
            p.b = Some(true);
            n += 1;
        });
        assert_eq!(n, 4, "two runs and two end marks");
        for_each_paragraph_props(&mut body, |p| p.algn = Some(dml::ST_TextAlignType::Ctr));
        assert!(
            body.p
                .iter()
                .all(|p| p.p_pr.as_ref().unwrap().algn == Some(dml::ST_TextAlignType::Ctr))
        );
        assert_eq!(replace_in_body(&mut body, "hello", "bye"), 2);
        assert_eq!(replace_in_body(&mut body, "", "x"), 0);
        assert_eq!(body_text(&body), "bye bye\nworld");
        assert_eq!(font("Arial").typeface.as_deref(), Some("Arial"));
        assert_eq!(lang_props().lang.as_deref(), Some("en-US"));
    }
}
