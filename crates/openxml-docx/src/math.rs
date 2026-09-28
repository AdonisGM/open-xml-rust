//! Equations (Office Math Markup Language, `m:oMath`).

use openxml_schema::shared_math::{self as m, EG_OMathElements};
use openxml_schema::shared_types::ST_OnOff;
use openxml_schema::wml::{self, EG_BlockLevelElts, EG_PContent};

use crate::document::Document;
use crate::paragraph::{Paragraph, ParagraphMut};
use crate::run::set_property;

/// An equation built from Office Math structures.
///
/// ```
/// use openxml_docx::{Document, Math};
///
/// // x = (-b ± √(b² - 4ac)) / 2a
/// let b2 = Math::superscript(Math::text("b"), Math::text("2"));
/// let root = Math::sqrt(Math::row([b2, Math::text("-4ac")]));
/// let quadratic = Math::row([
///     Math::text("x="),
///     Math::frac(Math::row([Math::text("-b±"), root]), Math::text("2a")),
/// ]);
/// let mut doc = Document::new();
/// doc.add_equation(&quadratic);
/// assert_eq!(doc.equations(), ["x=(-b±√(b^(2)-4ac))/(2a)"]);
/// # Ok::<(), openxml_docx::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Math {
    /// Text: variables, numbers and operators (`m:r`).
    Text(String),
    /// A sequence of expressions.
    Row(Vec<Math>),
    /// Numerator over denominator (`m:f`).
    Fraction(Box<Math>, Box<Math>),
    /// Base with superscript (`m:sSup`).
    Superscript(Box<Math>, Box<Math>),
    /// Base with subscript (`m:sSub`).
    Subscript(Box<Math>, Box<Math>),
    /// Base with subscript and superscript (`m:sSubSup`).
    SubSuperscript(Box<Math>, Box<Math>, Box<Math>),
    /// Radical with an optional degree (`m:rad`).
    Radical(Option<Box<Math>>, Box<Math>),
    /// N-ary operator such as ∑ or ∫ with optional limits (`m:nary`).
    Nary {
        /// Operator character.
        operator: char,
        /// Lower limit.
        lower: Option<Box<Math>>,
        /// Upper limit.
        upper: Option<Box<Math>>,
        /// Operand.
        body: Box<Math>,
    },
    /// Items between delimiters, separated by `|` (`m:d`).
    Delimited {
        /// Opening character.
        open: char,
        /// Closing character.
        close: char,
        /// Enclosed items.
        items: Vec<Math>,
    },
    /// A matrix given row by row (`m:m`).
    Matrix(Vec<Vec<Math>>),
}

impl Math {
    /// Text.
    pub fn text(s: &str) -> Self {
        Math::Text(s.to_owned())
    }

    /// A sequence of expressions.
    pub fn row(items: impl IntoIterator<Item = Math>) -> Self {
        Math::Row(items.into_iter().collect())
    }

    /// A fraction.
    pub fn frac(numerator: Math, denominator: Math) -> Self {
        Math::Fraction(Box::new(numerator), Box::new(denominator))
    }

    /// A superscript.
    pub fn superscript(base: Math, superscript: Math) -> Self {
        Math::Superscript(Box::new(base), Box::new(superscript))
    }

    /// A subscript.
    pub fn subscript(base: Math, subscript: Math) -> Self {
        Math::Subscript(Box::new(base), Box::new(subscript))
    }

    /// Subscript and superscript.
    pub fn sub_superscript(base: Math, subscript: Math, superscript: Math) -> Self {
        Math::SubSuperscript(Box::new(base), Box::new(subscript), Box::new(superscript))
    }

    /// A square root.
    pub fn sqrt(x: Math) -> Self {
        Math::Radical(None, Box::new(x))
    }

    /// An n-th root.
    pub fn root(degree: Math, x: Math) -> Self {
        Math::Radical(Some(Box::new(degree)), Box::new(x))
    }

    /// An n-ary operator.
    pub fn nary(operator: char, lower: Option<Math>, upper: Option<Math>, body: Math) -> Self {
        Math::Nary {
            operator,
            lower: lower.map(Box::new),
            upper: upper.map(Box::new),
            body: Box::new(body),
        }
    }

    /// A summation (∑).
    pub fn sum(lower: Option<Math>, upper: Option<Math>, body: Math) -> Self {
        Self::nary('\u{2211}', lower, upper, body)
    }

    /// An integral (∫).
    pub fn integral(lower: Option<Math>, upper: Option<Math>, body: Math) -> Self {
        Self::nary('\u{222B}', lower, upper, body)
    }

    /// Parentheses around an expression.
    pub fn parens(x: Math) -> Self {
        Math::Delimited {
            open: '(',
            close: ')',
            items: vec![x],
        }
    }

    /// Items between custom delimiters.
    pub fn delimited(open: char, close: char, items: impl IntoIterator<Item = Math>) -> Self {
        Math::Delimited {
            open,
            close,
            items: items.into_iter().collect(),
        }
    }

    /// A matrix.
    pub fn matrix(rows: impl IntoIterator<Item = Vec<Math>>) -> Self {
        Math::Matrix(rows.into_iter().collect())
    }

    fn arg(&self) -> Box<m::CT_OMathArg> {
        Box::new(m::CT_OMathArg {
            o_math_elements: self.elements(),
            ..Default::default()
        })
    }

    fn opt_arg(x: &Option<Box<Math>>) -> Box<m::CT_OMathArg> {
        x.as_deref().map_or_else(Box::default, Math::arg)
    }

    /// The OMML elements of the expression.
    pub(crate) fn elements(&self) -> Vec<EG_OMathElements> {
        let on = || {
            Some(Box::new(m::CT_OnOff {
                val: Some(ST_OnOff::Boolean(true)),
                ..Default::default()
            }))
        };
        let chr = |c: char| {
            Some(Box::new(m::CT_Char {
                val: Some(c.to_string()),
                ..Default::default()
            }))
        };
        match self {
            Math::Text(s) => vec![EG_OMathElements::R(Box::new(math_run(s)))],
            Math::Row(items) => items.iter().flat_map(Math::elements).collect(),
            Math::Fraction(n, d) => vec![EG_OMathElements::F(Box::new(m::CT_F {
                num: Some(n.arg()),
                den: Some(d.arg()),
                ..Default::default()
            }))],
            Math::Superscript(b, s) => vec![EG_OMathElements::SSup(Box::new(m::CT_SSup {
                e: Some(b.arg()),
                sup: Some(s.arg()),
                ..Default::default()
            }))],
            Math::Subscript(b, s) => vec![EG_OMathElements::SSub(Box::new(m::CT_SSub {
                e: Some(b.arg()),
                sub: Some(s.arg()),
                ..Default::default()
            }))],
            Math::SubSuperscript(b, sub, sup) => vec![EG_OMathElements::SSubSup(Box::new(m::CT_SSubSup {
                e: Some(b.arg()),
                sub: Some(sub.arg()),
                sup: Some(sup.arg()),
                ..Default::default()
            }))],
            Math::Radical(degree, x) => vec![EG_OMathElements::Rad(Box::new(m::CT_Rad {
                rad_pr: degree.is_none().then(|| {
                    Box::new(m::CT_RadPr {
                        deg_hide: on(),
                        ..Default::default()
                    })
                }),
                deg: Some(Self::opt_arg(degree)),
                e: Some(x.arg()),
                ..Default::default()
            }))],
            Math::Nary {
                operator,
                lower,
                upper,
                body,
            } => {
                let integral = ('\u{222B}'..='\u{2233}').contains(operator);
                vec![EG_OMathElements::Nary(Box::new(m::CT_Nary {
                    nary_pr: Some(Box::new(m::CT_NaryPr {
                        chr: chr(*operator),
                        lim_loc: Some(Box::new(m::CT_LimLoc {
                            val: Some(if integral {
                                m::ST_LimLoc::SubSup
                            } else {
                                m::ST_LimLoc::UndOvr
                            }),
                            ..Default::default()
                        })),
                        sub_hide: if lower.is_none() { on() } else { None },
                        sup_hide: if upper.is_none() { on() } else { None },
                        ..Default::default()
                    })),
                    sub: Some(Self::opt_arg(lower)),
                    sup: Some(Self::opt_arg(upper)),
                    e: Some(body.arg()),
                    ..Default::default()
                }))]
            }
            Math::Delimited { open, close, items } => vec![EG_OMathElements::D(Box::new(m::CT_D {
                d_pr: Some(Box::new(m::CT_DPr {
                    beg_chr: chr(*open),
                    end_chr: chr(*close),
                    ..Default::default()
                })),
                e: if items.is_empty() {
                    vec![m::CT_OMathArg::default()]
                } else {
                    items.iter().map(|i| *i.arg()).collect()
                },
                ..Default::default()
            }))],
            Math::Matrix(rows) => vec![EG_OMathElements::M(Box::new(m::CT_M {
                mr: rows
                    .iter()
                    .map(|row| m::CT_MR {
                        e: row.iter().map(|c| *c.arg()).collect(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }))],
        }
    }

    /// The expression as an `m:oMath` element.
    pub fn to_omath(&self) -> m::CT_OMath {
        m::CT_OMath {
            o_math_elements: self.elements(),
            ..Default::default()
        }
    }
}

/// A math run in the Cambria Math font.
fn math_run(s: &str) -> m::CT_R {
    let mut rpr = wml::CT_RPr::default();
    set_property(
        &mut rpr.r_pr_base,
        wml::EG_RPrBase::RFonts(Box::new(wml::CT_Fonts {
            ascii: Some("Cambria Math".into()),
            h_ansi: Some("Cambria Math".into()),
            ..Default::default()
        })),
    );
    let preserve = s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace);
    m::CT_R {
        r_pr_2: Some(Box::new(rpr)),
        choice: vec![m::CT_R_Choice::MT(Box::new(m::CT_Text {
            value: s.to_owned(),
            xml_space: preserve.then(|| "preserve".to_owned()),
            ..Default::default()
        }))],
        ..Default::default()
    }
}

/// Wraps linear text in parentheses unless it is a single character.
fn group(s: String) -> String {
    if s.chars().count() <= 1 {
        s
    } else {
        format!("({s})")
    }
}

fn arg_text(a: &Option<Box<m::CT_OMathArg>>) -> String {
    a.as_deref()
        .map(|a| elements_text(&a.o_math_elements))
        .unwrap_or_default()
}

/// A linear (UnicodeMath-like) rendering of math elements.
pub(crate) fn elements_text(items: &[EG_OMathElements]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            EG_OMathElements::R(r) => {
                for c in &r.choice {
                    match c {
                        m::CT_R_Choice::MT(t) => out.push_str(&t.value),
                        m::CT_R_Choice::WT(t) => out.push_str(&t.value),
                        _ => {}
                    }
                }
            }
            EG_OMathElements::F(f) => {
                out.push_str(&format!("({})/({})", arg_text(&f.num), arg_text(&f.den)));
            }
            EG_OMathElements::SSup(s) => {
                out.push_str(&format!("{}^({})", arg_text(&s.e), arg_text(&s.sup)));
            }
            EG_OMathElements::SSub(s) => {
                out.push_str(&format!("{}_({})", arg_text(&s.e), arg_text(&s.sub)));
            }
            EG_OMathElements::SSubSup(s) => {
                out.push_str(&format!(
                    "{}_({})^({})",
                    arg_text(&s.e),
                    arg_text(&s.sub),
                    arg_text(&s.sup)
                ));
            }
            EG_OMathElements::Rad(r) => {
                let degree = arg_text(&r.deg);
                if degree.is_empty() {
                    out.push_str(&format!("√({})", arg_text(&r.e)));
                } else {
                    out.push_str(&format!("√({degree}&{})", arg_text(&r.e)));
                }
            }
            EG_OMathElements::Nary(n) => {
                let op = n
                    .nary_pr
                    .as_deref()
                    .and_then(|p| p.chr.as_deref())
                    .and_then(|c| c.val.clone())
                    .unwrap_or_else(|| "\u{222B}".into());
                out.push_str(&op);
                let lower = arg_text(&n.sub);
                let upper = arg_text(&n.sup);
                if !lower.is_empty() {
                    out.push('_');
                    out.push_str(&group(lower));
                }
                if !upper.is_empty() {
                    out.push('^');
                    out.push_str(&group(upper));
                }
                out.push(' ');
                out.push_str(&arg_text(&n.e));
            }
            EG_OMathElements::D(d) => {
                let pr = d.d_pr.as_deref();
                let get = |c: Option<&Box<m::CT_Char>>, default: &str| {
                    c.and_then(|c| c.val.clone())
                        .unwrap_or_else(|| default.to_owned())
                };
                let open = get(pr.and_then(|p| p.beg_chr.as_ref()), "(");
                let close = get(pr.and_then(|p| p.end_chr.as_ref()), ")");
                let sep = get(pr.and_then(|p| p.sep_chr.as_ref()), "|");
                let inner: Vec<String> = d.e.iter().map(|a| elements_text(&a.o_math_elements)).collect();
                out.push_str(&format!("{open}{}{close}", inner.join(&sep)));
            }
            EG_OMathElements::M(mat) => {
                let rows: Vec<String> = mat
                    .mr
                    .iter()
                    .map(|r| {
                        r.e.iter()
                            .map(|c| elements_text(&c.o_math_elements))
                            .collect::<Vec<_>>()
                            .join("&")
                    })
                    .collect();
                out.push_str(&format!("■({})", rows.join("@")));
            }
            EG_OMathElements::Func(f) => {
                out.push_str(&arg_text(&f.f_name));
                out.push_str(&group(arg_text(&f.e)));
            }
            EG_OMathElements::LimLow(l) => {
                out.push_str(&format!("{}_({})", arg_text(&l.e), arg_text(&l.lim)))
            }
            EG_OMathElements::LimUpp(l) => {
                out.push_str(&format!("{}^({})", arg_text(&l.e), arg_text(&l.lim)))
            }
            EG_OMathElements::SPre(s) => out.push_str(&format!(
                "_({})^({}){}",
                arg_text(&s.sub),
                arg_text(&s.sup),
                arg_text(&s.e)
            )),
            EG_OMathElements::Acc(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::Bar(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::Box(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::BorderBox(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::GroupChr(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::Phant(x) => out.push_str(&arg_text(&x.e)),
            EG_OMathElements::EqArr(x) => {
                let rows: Vec<String> = x.e.iter().map(|a| elements_text(&a.o_math_elements)).collect();
                out.push_str(&rows.join("\n"));
            }
            EG_OMathElements::OMath(o) => out.push_str(&elements_text(&o.o_math_elements)),
            EG_OMathElements::OMathPara(p) => {
                let parts: Vec<String> = p
                    .o_math
                    .iter()
                    .map(|o| elements_text(&o.o_math_elements))
                    .collect();
                out.push_str(&parts.join("\n"));
            }
            _ => {}
        }
    }
    out
}

fn content_equations(items: &[EG_PContent], out: &mut Vec<String>) {
    for item in items {
        match item {
            EG_PContent::OMath(o) => out.push(elements_text(&o.o_math_elements)),
            EG_PContent::OMathPara(p) => {
                for o in &p.o_math {
                    out.push(elements_text(&o.o_math_elements));
                }
            }
            EG_PContent::Hyperlink(h) => content_equations(&h.p_content, out),
            EG_PContent::Sdt(s) => {
                if let Some(c) = &s.sdt_content {
                    content_equations(&c.p_content, out);
                }
            }
            EG_PContent::CustomXml(x) => content_equations(&x.p_content, out),
            EG_PContent::SmartTag(x) => content_equations(&x.p_content, out),
            _ => {}
        }
    }
}

impl Paragraph<'_> {
    /// Linear text of the equations in the paragraph.
    pub fn equations(&self) -> Vec<String> {
        let mut out = Vec::new();
        content_equations(&self.p.p_content, &mut out);
        out
    }
}

impl ParagraphMut<'_> {
    /// Appends an inline equation (`m:oMath`).
    pub fn add_equation(&mut self, math: &Math) -> &mut Self {
        self.p
            .p_content
            .push(EG_PContent::OMath(Box::new(math.to_omath())));
        self
    }
}

impl Document {
    /// Appends a paragraph holding a display equation (`m:oMathPara`).
    pub fn add_equation(&mut self, math: &Math) -> ParagraphMut<'_> {
        let mut p = self.add_paragraph("");
        p.raw()
            .p_content
            .push(EG_PContent::OMathPara(Box::new(m::CT_OMathPara {
                o_math: vec![math.to_omath()],
                ..Default::default()
            })));
        p
    }

    /// Linear text of every equation in the body.
    pub fn equations(&self) -> Vec<String> {
        let mut out = Vec::new();
        crate::walk::walk_blocks_ref(&self.body().block_level_elts, &mut |p| {
            content_equations(&p.p_content, &mut out);
        });
        for item in &self.body().block_level_elts {
            match item {
                EG_BlockLevelElts::OMath(o) => out.push(elements_text(&o.o_math_elements)),
                EG_BlockLevelElts::OMathPara(p) => {
                    for o in &p.o_math {
                        out.push(elements_text(&o.o_math_elements));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(m: &Math) -> String {
        elements_text(&m.elements())
    }

    #[test]
    fn builders_produce_omml_and_linear_text() {
        assert_eq!(text(&Math::frac(Math::text("a"), Math::text("b"))), "(a)/(b)");
        assert_eq!(text(&Math::subscript(Math::text("x"), Math::text("i"))), "x_(i)");
        assert_eq!(
            text(&Math::sub_superscript(
                Math::text("x"),
                Math::text("i"),
                Math::text("2")
            )),
            "x_(i)^(2)"
        );
        assert_eq!(text(&Math::root(Math::text("3"), Math::text("x"))), "√(3&x)");
        assert_eq!(
            text(&Math::sum(
                Some(Math::text("i=1")),
                Some(Math::text("n")),
                Math::text("i")
            )),
            "∑_(i=1)^n i"
        );
        assert_eq!(text(&Math::integral(None, None, Math::text("f"))), "∫ f");
        assert_eq!(
            text(&Math::delimited('[', ']', [Math::text("a"), Math::text("b")])),
            "[a|b]"
        );
        assert_eq!(text(&Math::parens(Math::text("x"))), "(x)");
        assert_eq!(
            text(&Math::matrix([
                vec![Math::text("1"), Math::text("0")],
                vec![Math::text("0"), Math::text("1")]
            ])),
            "■(1&0@0&1)"
        );
        assert_eq!(text(&Math::delimited('{', '}', [])), "{}");
    }

    #[test]
    fn structure_details() {
        let sqrt = Math::sqrt(Math::text("x")).elements();
        let EG_OMathElements::Rad(r) = &sqrt[0] else {
            panic!()
        };
        assert!(r.rad_pr.as_ref().unwrap().deg_hide.is_some());
        assert!(r.deg.is_some());
        let int = Math::integral(Some(Math::text("0")), None, Math::text("x")).elements();
        let EG_OMathElements::Nary(n) = &int[0] else {
            panic!()
        };
        let pr = n.nary_pr.as_ref().unwrap();
        assert_eq!(pr.lim_loc.as_ref().unwrap().val, Some(m::ST_LimLoc::SubSup));
        assert!(pr.sub_hide.is_none() && pr.sup_hide.is_some());
        let run = math_run(" x ");
        let m::CT_R_Choice::MT(t) = &run.choice[0] else {
            panic!()
        };
        assert_eq!(t.xml_space.as_deref(), Some("preserve"));
        assert!(run.r_pr_2.is_some());
    }
}
