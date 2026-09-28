//! Runs: contiguous text with the same character formatting.

use openxml_core::{FontSize, Result};
use openxml_schema::wml::{
    self, EG_RPrBase, EG_RunInnerContent, ST_BrType, ST_HighlightColor, ST_HpsMeasure, ST_Underline,
};

use crate::text;
use crate::util::{hex_color, hex_color_string, off, on, on_off_value, string_val, text_node};

/// Schema order of the run properties (`EG_RPrBase`, ECMA-376 Part 1 §17.3.2).
const RPR_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
];

fn rank(p: &EG_RPrBase) -> usize {
    let name = p.element_name().1;
    RPR_ORDER
        .iter()
        .position(|n| *n == name)
        .unwrap_or(RPR_ORDER.len())
}

/// Sets a run property, replacing a property of the same kind and keeping
/// the schema order.
pub(crate) fn set_property(props: &mut Vec<EG_RPrBase>, prop: EG_RPrBase) {
    let name = prop.element_name().1.to_owned();
    props.retain(|p| p.element_name().1 != name);
    let r = rank(&prop);
    let at = props.iter().position(|p| rank(p) > r).unwrap_or(props.len());
    props.insert(at, prop);
}

/// Removes the run property with the given element name.
pub(crate) fn remove_property(props: &mut Vec<EG_RPrBase>, name: &str) {
    props.retain(|p| p.element_name().1 != name);
}

/// Kind of break inserted with [`RunMut::add_break`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakKind {
    /// Line break (`<w:br/>`).
    Line,
    /// Page break.
    Page,
    /// Column break.
    Column,
}

/// Vertical alignment of text in a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    /// Normal text.
    Baseline,
    /// Superscript.
    Superscript,
    /// Subscript.
    Subscript,
}

/// Read-only view of a run.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    run: &'a wml::CT_R,
}

impl<'a> Run<'a> {
    pub(crate) fn new(run: &'a wml::CT_R) -> Self {
        Run { run }
    }

    /// The underlying schema object.
    pub fn raw(&self) -> &'a wml::CT_R {
        self.run
    }

    /// Text of the run (tabs as `\t`, breaks as `\n`).
    pub fn text(&self) -> String {
        text::run_text(self.run)
    }

    fn props(&self) -> &'a [EG_RPrBase] {
        self.run
            .r_pr
            .as_deref()
            .map(|p| p.r_pr_base.as_slice())
            .unwrap_or(&[])
    }

    fn flag(&self, f: impl Fn(&EG_RPrBase) -> Option<&wml::CT_OnOff>) -> bool {
        self.props().iter().find_map(f).is_some_and(on_off_value)
    }

    /// Whether bold is applied directly to the run.
    pub fn is_bold(&self) -> bool {
        self.flag(|p| if let EG_RPrBase::B(v) = p { Some(v) } else { None })
    }

    /// Whether italic is applied directly to the run.
    pub fn is_italic(&self) -> bool {
        self.flag(|p| if let EG_RPrBase::I(v) = p { Some(v) } else { None })
    }

    /// Whether single strikethrough is applied directly to the run.
    pub fn is_strike(&self) -> bool {
        self.flag(|p| {
            if let EG_RPrBase::Strike(v) = p {
                Some(v)
            } else {
                None
            }
        })
    }

    /// Underline style applied directly to the run.
    pub fn underline(&self) -> Option<ST_Underline> {
        self.props()
            .iter()
            .find_map(|p| if let EG_RPrBase::U(u) = p { u.val } else { None })
    }

    /// Font size applied directly to the run.
    pub fn size(&self) -> Option<FontSize> {
        self.props().iter().find_map(|p| match p {
            EG_RPrBase::Sz(s) => match s.val {
                Some(ST_HpsMeasure::UnsignedDecimalNumber(v)) => Some(FontSize::from_half_points(v)),
                _ => None,
            },
            _ => None,
        })
    }

    /// Text color (`RRGGBB` or `auto`) applied directly to the run.
    pub fn color(&self) -> Option<String> {
        self.props().iter().find_map(|p| {
            if let EG_RPrBase::Color(c) = p {
                c.val.as_ref().map(hex_color_string)
            } else {
                None
            }
        })
    }

    /// Latin font (`w:rFonts/@w:ascii`) applied directly to the run.
    pub fn font(&self) -> Option<&'a str> {
        self.props().iter().find_map(|p| {
            if let EG_RPrBase::RFonts(f) = p {
                f.ascii.as_deref()
            } else {
                None
            }
        })
    }

    /// Highlight color applied to the run.
    pub fn highlight(&self) -> Option<ST_HighlightColor> {
        self.props().iter().find_map(|p| {
            if let EG_RPrBase::Highlight(h) = p {
                h.val
            } else {
                None
            }
        })
    }

    /// Character style of the run.
    pub fn style_id(&self) -> Option<&'a str> {
        self.props().iter().find_map(|p| {
            if let EG_RPrBase::RStyle(s) = p {
                s.val.as_deref()
            } else {
                None
            }
        })
    }

    /// Vertical alignment (superscript/subscript) applied to the run.
    pub fn vertical_align(&self) -> Option<VerticalAlign> {
        use openxml_schema::shared_types::ST_VerticalAlignRun as V;
        self.props().iter().find_map(|p| match p {
            EG_RPrBase::VertAlign(v) => v.val.map(|v| match v {
                V::Baseline => VerticalAlign::Baseline,
                V::Superscript => VerticalAlign::Superscript,
                V::Subscript => VerticalAlign::Subscript,
            }),
            _ => None,
        })
    }

    /// Whether the run contains a drawing (picture, chart, shape).
    pub fn has_drawing(&self) -> bool {
        self.run
            .run_inner_content
            .iter()
            .any(|c| matches!(c, EG_RunInnerContent::Drawing(_)))
    }
}

/// Mutable access to a run, with builder-style formatting methods.
///
/// ```
/// use openxml_docx::{Document, FontSize};
///
/// let mut doc = Document::new();
/// let mut p = doc.add_paragraph("");
/// p.add_run("Important").bold(true).size(FontSize(14.0)).color("C00000")?;
/// assert_eq!(doc.paragraphs()[0].runs()[0].is_bold(), true);
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Debug)]
pub struct RunMut<'a> {
    run: &'a mut wml::CT_R,
}

impl<'a> RunMut<'a> {
    pub(crate) fn new(run: &'a mut wml::CT_R) -> Self {
        RunMut { run }
    }

    /// The underlying schema object.
    pub fn raw(&mut self) -> &mut wml::CT_R {
        self.run
    }

    /// Read-only view of the run.
    pub fn view(&self) -> Run<'_> {
        Run::new(self.run)
    }

    fn props(&mut self) -> &mut Vec<EG_RPrBase> {
        &mut self.run.r_pr.get_or_insert_with(Default::default).r_pr_base
    }

    fn toggle(&mut self, value: bool, make: fn(Box<wml::CT_OnOff>) -> EG_RPrBase) -> &mut Self {
        if value {
            set_property(self.props(), make(on()));
        } else if self.run.r_pr.is_some() {
            // An explicit "off" overrides formatting inherited from styles.
            set_property(self.props(), make(off()));
        }
        self
    }

    /// Appends text to the run.
    pub fn add_text(&mut self, text: &str) -> &mut Self {
        self.run
            .run_inner_content
            .push(EG_RunInnerContent::T(text_node(text)));
        self
    }

    /// Appends a tab character.
    pub fn add_tab(&mut self) -> &mut Self {
        self.run
            .run_inner_content
            .push(EG_RunInnerContent::Tab(Box::default()));
        self
    }

    /// Appends a line, page or column break.
    pub fn add_break(&mut self, kind: BreakKind) -> &mut Self {
        let type_ = match kind {
            BreakKind::Line => None,
            BreakKind::Page => Some(ST_BrType::Page),
            BreakKind::Column => Some(ST_BrType::Column),
        };
        self.run
            .run_inner_content
            .push(EG_RunInnerContent::Br(Box::new(wml::CT_Br {
                type_,
                ..Default::default()
            })));
        self
    }

    /// Replaces the run's content with `text`.
    pub fn set_text(&mut self, text: &str) -> &mut Self {
        self.run
            .run_inner_content
            .retain(|c| !matches!(c, EG_RunInnerContent::T(_)));
        self.add_text(text)
    }

    /// Sets or clears bold (`w:b` and `w:bCs`).
    pub fn bold(&mut self, value: bool) -> &mut Self {
        self.toggle(value, EG_RPrBase::B);
        self.toggle(value, EG_RPrBase::BCs)
    }

    /// Sets or clears italic (`w:i` and `w:iCs`).
    pub fn italic(&mut self, value: bool) -> &mut Self {
        self.toggle(value, EG_RPrBase::I);
        self.toggle(value, EG_RPrBase::ICs)
    }

    /// Sets or clears single strikethrough.
    pub fn strike(&mut self, value: bool) -> &mut Self {
        self.toggle(value, EG_RPrBase::Strike)
    }

    /// Sets or clears small capitals.
    pub fn small_caps(&mut self, value: bool) -> &mut Self {
        self.toggle(value, EG_RPrBase::SmallCaps)
    }

    /// Sets the underline style (`None` removes direct underline).
    pub fn underline(&mut self, style: Option<ST_Underline>) -> &mut Self {
        match style {
            Some(val) => set_property(
                self.props(),
                EG_RPrBase::U(Box::new(wml::CT_Underline {
                    val: Some(val),
                    ..Default::default()
                })),
            ),
            None => remove_property(self.props(), "u"),
        }
        self
    }

    /// Sets the font size (`w:sz` and `w:szCs`).
    pub fn size(&mut self, size: FontSize) -> &mut Self {
        let hps = || {
            Box::new(wml::CT_HpsMeasure {
                val: Some(ST_HpsMeasure::UnsignedDecimalNumber(size.half_points())),
                ..Default::default()
            })
        };
        set_property(self.props(), EG_RPrBase::Sz(hps()));
        set_property(self.props(), EG_RPrBase::SzCs(hps()));
        self
    }

    /// Sets the text color from `RRGGBB` (with or without `#`) or `auto`.
    pub fn color(&mut self, hex: &str) -> Result<&mut Self> {
        let val = hex_color(hex)?;
        set_property(
            self.props(),
            EG_RPrBase::Color(Box::new(wml::CT_Color {
                val: Some(val),
                ..Default::default()
            })),
        );
        Ok(self)
    }

    /// Sets the font for Latin, complex-script and East Asian text.
    pub fn font(&mut self, name: &str) -> &mut Self {
        let fonts = wml::CT_Fonts {
            ascii: Some(name.to_owned()),
            h_ansi: Some(name.to_owned()),
            east_asia: Some(name.to_owned()),
            cs: Some(name.to_owned()),
            ..Default::default()
        };
        set_property(self.props(), EG_RPrBase::RFonts(Box::new(fonts)));
        self
    }

    /// Sets the highlight color (`None` removes it).
    pub fn highlight(&mut self, color: Option<ST_HighlightColor>) -> &mut Self {
        match color {
            Some(val) => set_property(
                self.props(),
                EG_RPrBase::Highlight(Box::new(wml::CT_Highlight {
                    val: Some(val),
                    ..Default::default()
                })),
            ),
            None => remove_property(self.props(), "highlight"),
        }
        self
    }

    /// Applies a character style.
    pub fn style(&mut self, style_id: &str) -> &mut Self {
        set_property(self.props(), EG_RPrBase::RStyle(string_val(style_id)));
        self
    }

    /// Sets superscript/subscript.
    pub fn vertical_align(&mut self, align: VerticalAlign) -> &mut Self {
        use openxml_schema::shared_types::ST_VerticalAlignRun as V;
        let val = match align {
            VerticalAlign::Baseline => V::Baseline,
            VerticalAlign::Superscript => V::Superscript,
            VerticalAlign::Subscript => V::Subscript,
        };
        set_property(
            self.props(),
            EG_RPrBase::VertAlign(Box::new(wml::CT_VerticalAlignRun {
                val: Some(val),
                ..Default::default()
            })),
        );
        self
    }

    /// Whether bold is currently applied directly.
    pub fn is_bold(&self) -> bool {
        Run::new(self.run).is_bold()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(r: &wml::CT_R) -> Vec<String> {
        r.r_pr
            .as_ref()
            .unwrap()
            .r_pr_base
            .iter()
            .map(|p| p.element_name().1.to_owned())
            .collect()
    }

    #[test]
    fn properties_are_kept_in_schema_order() {
        let mut r = wml::CT_R::default();
        let mut m = RunMut::new(&mut r);
        m.size(FontSize(12.0))
            .color("FF0000")
            .unwrap()
            .bold(true)
            .font("Arial")
            .italic(true);
        m.highlight(Some(ST_HighlightColor::Yellow))
            .underline(Some(ST_Underline::Double))
            .style("Emphasis");
        m.vertical_align(VerticalAlign::Superscript)
            .strike(true)
            .small_caps(true);
        assert_eq!(
            names(&r),
            [
                "rStyle",
                "rFonts",
                "b",
                "bCs",
                "i",
                "iCs",
                "smallCaps",
                "strike",
                "color",
                "sz",
                "szCs",
                "highlight",
                "u",
                "vertAlign"
            ]
        );
    }

    #[test]
    fn setting_twice_replaces() {
        let mut r = wml::CT_R::default();
        let mut m = RunMut::new(&mut r);
        m.size(FontSize(10.0))
            .size(FontSize(20.0))
            .color("000000")
            .unwrap()
            .color("FFFFFF")
            .unwrap();
        let v = Run::new(&r);
        assert_eq!(v.size(), Some(FontSize(20.0)));
        assert_eq!(v.color().as_deref(), Some("FFFFFF"));
        assert_eq!(names(&r).len(), 3);
    }

    #[test]
    fn toggles_and_removal() {
        let mut r = wml::CT_R::default();
        let mut m = RunMut::new(&mut r);
        m.bold(false);
        assert!(m.run.r_pr.is_none(), "turning off on a plain run adds nothing");
        m.bold(true).italic(true);
        assert!(m.is_bold());
        m.bold(false);
        assert!(!m.is_bold());
        m.underline(Some(ST_Underline::Single))
            .underline(None)
            .highlight(Some(ST_HighlightColor::Red))
            .highlight(None);
        let v = Run::new(&r);
        assert!(!v.is_bold());
        assert!(v.is_italic());
        assert_eq!(v.underline(), None);
        assert_eq!(v.highlight(), None);
        assert!(!v.is_strike());
    }

    #[test]
    fn content_and_views() {
        let mut r = wml::CT_R::default();
        let mut m = RunMut::new(&mut r);
        m.add_text("a")
            .add_tab()
            .add_text("b")
            .add_break(BreakKind::Line)
            .add_break(BreakKind::Page);
        m.add_break(BreakKind::Column);
        assert_eq!(m.view().text(), "a\tb\n\n\n");
        m.set_text("new");
        assert_eq!(m.view().text(), "\t\n\n\nnew");
        m.font("Arial")
            .style("Strong")
            .vertical_align(VerticalAlign::Subscript);
        assert!(m.color("nope").is_err());
        let _ = m.raw();
        let v = Run::new(&r);
        assert_eq!(v.font(), Some("Arial"));
        assert_eq!(v.style_id(), Some("Strong"));
        assert_eq!(v.vertical_align(), Some(VerticalAlign::Subscript));
        assert!(!v.has_drawing());
        assert!(std::ptr::eq(v.raw(), &r));
    }
}
