//! Cell formatting: fonts, fills, borders, number formats and alignment
//! (the stylesheet `xl/styles.xml`, ECMA-376 Part 1 §18.8).
//!
//! A [`CellStyle`] describes the formatting of a cell. Registering it with
//! [`crate::Workbook::add_style`] adds the font, fill, border, number format
//! and cell format (`xf`) records it needs — reusing identical records — and
//! returns the [`StyleId`] to put on cells.

use openxml_schema::{shared_types, sml};
use openxml_xml::{ExtraChild, HexBinary, Ns, RawElement, XmlRead};

use crate::date::is_date_format;

pub use shared_types::ST_VerticalAlignRun as FontVerticalAlign;
pub use sml::{
    ST_BorderStyle as BorderStyle, ST_HorizontalAlignment as HorizontalAlignment,
    ST_PatternType as PatternType, ST_UnderlineValues as UnderlineStyle,
    ST_VerticalAlignment as VerticalAlignment,
};

/// Index of a cell format (`cellXfs` entry), stored in a cell's `s` attribute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StyleId(pub u32);

/// A colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// An opaque RGB colour.
    Rgb(u8, u8, u8),
    /// A theme colour index (0 = light 1, 1 = dark 1, 2 = light 2, 3 = dark 2, 4–9 = accents).
    Theme(u32),
    /// A legacy palette index.
    Indexed(u32),
}

impl Color {
    /// Parses `RRGGBB`, `#RRGGBB` or `AARRGGBB`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.trim().trim_start_matches('#');
        let h = if h.len() == 8 { &h[2..] } else { h };
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub(crate) fn to_ct(self) -> sml::CT_Color {
        match self {
            Color::Rgb(r, g, b) => sml::CT_Color {
                rgb: Some(HexBinary(vec![0xFF, r, g, b])),
                ..Default::default()
            },
            Color::Theme(t) => sml::CT_Color {
                theme: Some(t),
                ..Default::default()
            },
            Color::Indexed(i) => sml::CT_Color {
                indexed: Some(i),
                ..Default::default()
            },
        }
    }

    pub(crate) fn from_ct(c: &sml::CT_Color) -> Option<Self> {
        if let Some(rgb) = &c.rgb {
            let b = rgb.as_bytes();
            return match b.len() {
                4 => Some(Color::Rgb(b[1], b[2], b[3])),
                3 => Some(Color::Rgb(b[0], b[1], b[2])),
                _ => None,
            };
        }
        c.theme.map(Color::Theme).or(c.indexed.map(Color::Indexed))
    }
}

/// Font properties. Unset properties are taken from the workbook's default font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    /// Typeface name, e.g. `Arial`.
    pub name: Option<String>,
    /// Size in points.
    pub size: Option<f64>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Single underline.
    pub underline: bool,
    /// Underline style other than single (double, accounting); implies `underline`.
    pub underline_style: Option<UnderlineStyle>,
    /// Strikethrough.
    pub strike: bool,
    /// Superscript or subscript.
    pub vertical_align: Option<FontVerticalAlign>,
    /// Text colour.
    pub color: Option<Color>,
}

/// How the colours of a gradient fill are laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// A linear gradient at `degree` degrees (0 = left to right, 90 = top to bottom).
    Linear {
        /// Angle in degrees.
        degree: f64,
    },
    /// A path gradient spreading from a rectangle given as fractions (0–1) of the cell.
    Path {
        /// Left edge of the inner rectangle.
        left: f64,
        /// Right edge of the inner rectangle.
        right: f64,
        /// Top edge of the inner rectangle.
        top: f64,
        /// Bottom edge of the inner rectangle.
        bottom: f64,
    },
}

/// A gradient cell background (ECMA-376 Part 1 §18.8.24).
#[derive(Clone, Debug, PartialEq)]
pub struct GradientFill {
    /// Linear or path gradient.
    pub kind: GradientKind,
    /// Colour stops as `(position 0–1, colour)`.
    pub stops: Vec<(f64, Color)>,
}

/// A cell background.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// Pattern.
    pub pattern: PatternType,
    /// Foreground (pattern) colour; the colour of a solid fill.
    pub fg_color: Option<Color>,
    /// Background colour of the pattern.
    pub bg_color: Option<Color>,
    /// A gradient; when set, the pattern fields are ignored.
    pub gradient: Option<GradientFill>,
}

impl Fill {
    /// A solid fill.
    pub fn solid(color: Color) -> Self {
        Fill {
            pattern: PatternType::Solid,
            fg_color: Some(color),
            bg_color: None,
            gradient: None,
        }
    }

    /// A pattern fill (e.g. `PatternType::DarkGrid`) with foreground and background colours.
    pub fn pattern(pattern: PatternType, fg_color: Option<Color>, bg_color: Option<Color>) -> Self {
        Fill {
            pattern,
            fg_color,
            bg_color,
            gradient: None,
        }
    }

    /// A gradient fill.
    pub fn gradient(gradient: GradientFill) -> Self {
        Fill {
            pattern: PatternType::None,
            fg_color: None,
            bg_color: None,
            gradient: Some(gradient),
        }
    }

    /// A two-colour linear gradient at `degree` degrees.
    pub fn linear_gradient(degree: f64, from: Color, to: Color) -> Self {
        Fill::gradient(GradientFill {
            kind: GradientKind::Linear { degree },
            stops: vec![(0.0, from), (1.0, to)],
        })
    }
}

/// Cell protection flags; they take effect when the sheet is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellProtection {
    /// The cell cannot be edited (Excel's default for every cell).
    pub locked: bool,
    /// The formula is hidden in the formula bar.
    pub hidden: bool,
}

impl Default for CellProtection {
    fn default() -> Self {
        CellProtection {
            locked: true,
            hidden: false,
        }
    }
}

/// One edge of a border.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderSide {
    /// Line style.
    pub style: BorderStyle,
    /// Line colour (automatic when absent).
    pub color: Option<Color>,
}

/// Cell borders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Border {
    /// Left edge.
    pub left: Option<BorderSide>,
    /// Right edge.
    pub right: Option<BorderSide>,
    /// Top edge.
    pub top: Option<BorderSide>,
    /// Bottom edge.
    pub bottom: Option<BorderSide>,
    /// Diagonal line(s); drawn in the directions enabled below.
    pub diagonal: Option<BorderSide>,
    /// Draw the diagonal from bottom-left to top-right.
    pub diagonal_up: bool,
    /// Draw the diagonal from top-left to bottom-right.
    pub diagonal_down: bool,
}

impl Border {
    /// The same line on all four edges.
    pub fn all(style: BorderStyle, color: Option<Color>) -> Self {
        let side = Some(BorderSide { style, color });
        Border {
            left: side.clone(),
            right: side.clone(),
            top: side.clone(),
            bottom: side,
            ..Default::default()
        }
    }

    /// Adds diagonal lines.
    pub fn with_diagonal(mut self, side: BorderSide, up: bool, down: bool) -> Self {
        self.diagonal = Some(side);
        self.diagonal_up = up;
        self.diagonal_down = down;
        self
    }
}

/// A number format: one of the built-in formats (by id) or a custom format code.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NumberFormat {
    /// A built-in format id (0–49), see [`builtin_format_code`].
    Builtin(u32),
    /// A custom format code such as `#,##0.000` or `yyyy-mm-dd hh:mm`.
    Custom(String),
}

impl NumberFormat {
    /// `General` (id 0).
    pub const GENERAL: NumberFormat = NumberFormat::Builtin(0);
    /// `0` (id 1).
    pub const INTEGER: NumberFormat = NumberFormat::Builtin(1);
    /// `0.00` (id 2).
    pub const DECIMAL_2: NumberFormat = NumberFormat::Builtin(2);
    /// `#,##0` (id 3).
    pub const THOUSANDS: NumberFormat = NumberFormat::Builtin(3);
    /// `#,##0.00` (id 4).
    pub const THOUSANDS_DECIMAL_2: NumberFormat = NumberFormat::Builtin(4);
    /// `0%` (id 9).
    pub const PERCENT: NumberFormat = NumberFormat::Builtin(9);
    /// `0.00%` (id 10).
    pub const PERCENT_DECIMAL_2: NumberFormat = NumberFormat::Builtin(10);
    /// `0.00E+00` (id 11).
    pub const SCIENTIFIC: NumberFormat = NumberFormat::Builtin(11);
    /// Short date, `mm-dd-yy` (id 14; displayed in the user's locale).
    pub const DATE: NumberFormat = NumberFormat::Builtin(14);
    /// Time, `h:mm:ss` (id 21).
    pub const TIME: NumberFormat = NumberFormat::Builtin(21);
    /// Date and time, `m/d/yy h:mm` (id 22).
    pub const DATE_TIME: NumberFormat = NumberFormat::Builtin(22);
    /// Text, `@` (id 49).
    pub const TEXT: NumberFormat = NumberFormat::Builtin(49);
    /// Fraction with one digit, `# ?/?` (id 12).
    pub const FRACTION: NumberFormat = NumberFormat::Builtin(12);
    /// Fraction with two digits, `# ??/??` (id 13).
    pub const FRACTION_2: NumberFormat = NumberFormat::Builtin(13);
    /// `d-mmm-yy` (id 15).
    pub const DATE_D_MMM_YY: NumberFormat = NumberFormat::Builtin(15);
    /// `d-mmm` (id 16).
    pub const DATE_D_MMM: NumberFormat = NumberFormat::Builtin(16);
    /// `mmm-yy` (id 17).
    pub const DATE_MMM_YY: NumberFormat = NumberFormat::Builtin(17);
    /// `h:mm AM/PM` (id 18).
    pub const TIME_12H: NumberFormat = NumberFormat::Builtin(18);
    /// `h:mm:ss AM/PM` (id 19).
    pub const TIME_12H_SECONDS: NumberFormat = NumberFormat::Builtin(19);
    /// `h:mm` (id 20).
    pub const TIME_HOURS_MINUTES: NumberFormat = NumberFormat::Builtin(20);
    /// `#,##0 ;(#,##0)` — negatives in parentheses (id 37).
    pub const NEGATIVE_PARENS: NumberFormat = NumberFormat::Builtin(37);
    /// `#,##0 ;[Red](#,##0)` — negatives red in parentheses (id 38).
    pub const NEGATIVE_PARENS_RED: NumberFormat = NumberFormat::Builtin(38);
    /// `#,##0.00;(#,##0.00)` (id 39).
    pub const NEGATIVE_PARENS_DECIMAL_2: NumberFormat = NumberFormat::Builtin(39);
    /// `#,##0.00;[Red](#,##0.00)` (id 40).
    pub const NEGATIVE_PARENS_RED_DECIMAL_2: NumberFormat = NumberFormat::Builtin(40);
    /// `mm:ss` (id 45).
    pub const MINUTES_SECONDS: NumberFormat = NumberFormat::Builtin(45);
    /// Elapsed time, `[h]:mm:ss` (id 46).
    pub const DURATION: NumberFormat = NumberFormat::Builtin(46);
    /// `mmss.0` (id 47).
    pub const MINUTES_SECONDS_TENTHS: NumberFormat = NumberFormat::Builtin(47);
    /// Engineering notation, `##0.0E+0` (id 48).
    pub const ENGINEERING: NumberFormat = NumberFormat::Builtin(48);

    /// A custom format code.
    pub fn custom(code: impl Into<String>) -> Self {
        NumberFormat::Custom(code.into())
    }

    fn places(places: u8) -> String {
        if places == 0 {
            String::new()
        } else {
            format!(".{}", "0".repeat(usize::from(places)))
        }
    }

    /// A fixed number of decimal places, optionally with thousands separators
    /// (`decimal(2, true)` is `#,##0.00`).
    pub fn decimal(places: u8, thousands: bool) -> Self {
        let int = if thousands { "#,##0" } else { "0" };
        NumberFormat::Custom(format!("{int}{}", Self::places(places))).canonical()
    }

    /// A percentage with `places` decimals (`percent(1)` is `0.0%`).
    pub fn percent(places: u8) -> Self {
        NumberFormat::Custom(format!("0{}%", Self::places(places))).canonical()
    }

    /// Scientific notation with `places` decimals (`scientific(2)` is `0.00E+00`).
    pub fn scientific(places: u8) -> Self {
        NumberFormat::Custom(format!("0{}E+00", Self::places(places))).canonical()
    }

    /// A currency amount with the symbol before the number and negatives
    /// prefixed with a minus sign, e.g. `"$"#,##0.00`.
    pub fn currency(symbol: &str, places: u8) -> Self {
        let sym = symbol.replace('"', "");
        let n = format!("#,##0{}", Self::places(places));
        NumberFormat::Custom(format!("\"{sym}\"{n};-\"{sym}\"{n}"))
    }

    /// Excel's accounting layout: symbol aligned left, negatives in
    /// parentheses and zero shown as a dash.
    pub fn accounting(symbol: &str, places: u8) -> Self {
        let sym = symbol.replace('"', "");
        let p = Self::places(places);
        let dash = if places == 0 { "\"-\"" } else { "\"-\"??" };
        NumberFormat::Custom(format!(
            "_(\"{sym}\"* #,##0{p}_);_(\"{sym}\"* \\(#,##0{p}\\);_(\"{sym}\"* {dash}_);_(@_)"
        ))
    }

    /// The built-in id for codes of built-in formats, the value otherwise.
    fn canonical(self) -> Self {
        match &self {
            NumberFormat::Custom(code) => (0..FIRST_CUSTOM_NUMFMT)
                .find(|&id| builtin_format_code(id) == Some(code.as_str()))
                .map_or(self, NumberFormat::Builtin),
            NumberFormat::Builtin(_) => self,
        }
    }

    /// The format code (built-in codes are the invariant ones of §18.8.30).
    pub fn code(&self) -> Option<&str> {
        match self {
            NumberFormat::Builtin(id) => builtin_format_code(*id),
            NumberFormat::Custom(code) => Some(code),
        }
    }
}

/// Horizontal/vertical alignment, wrapping, indentation and rotation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Alignment {
    /// Horizontal alignment.
    pub horizontal: Option<HorizontalAlignment>,
    /// Vertical alignment.
    pub vertical: Option<VerticalAlignment>,
    /// Wrap text within the cell.
    pub wrap_text: bool,
    /// Indentation level.
    pub indent: Option<u32>,
    /// Text rotation in degrees (0–90 counter-clockwise, 91–180 clockwise).
    pub rotation: Option<u32>,
    /// Vertical (stacked) text: rotation value 255.
    pub vertical_text: bool,
    /// Shrink the text to fit the column width.
    pub shrink_to_fit: bool,
}

/// The formatting of a cell.
///
/// ```
/// use openxml_xlsx::{CellStyle, Color, NumberFormat, HorizontalAlignment};
///
/// let header = CellStyle::new()
///     .bold()
///     .font_color(Color::Rgb(255, 255, 255))
///     .fill_color(Color::Rgb(0x44, 0x72, 0xC4))
///     .horizontal(HorizontalAlignment::Center);
/// let money = CellStyle::new().number_format(NumberFormat::custom("#,##0.00 \"₫\""));
/// assert!(header.font.as_ref().unwrap().bold);
/// assert!(money.number_format.is_some());
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellStyle {
    /// Font.
    pub font: Option<Font>,
    /// Background.
    pub fill: Option<Fill>,
    /// Borders.
    pub border: Option<Border>,
    /// Number format.
    pub number_format: Option<NumberFormat>,
    /// Alignment.
    pub alignment: Option<Alignment>,
    /// Protection flags (locked / formula hidden).
    pub protection: Option<CellProtection>,
}

impl CellStyle {
    /// An empty style (the workbook default).
    pub fn new() -> Self {
        Self::default()
    }

    fn font_mut(&mut self) -> &mut Font {
        self.font.get_or_insert_with(Font::default)
    }

    fn alignment_mut(&mut self) -> &mut Alignment {
        self.alignment.get_or_insert_with(Alignment::default)
    }

    /// Bold text.
    pub fn bold(mut self) -> Self {
        self.font_mut().bold = true;
        self
    }
    /// Italic text.
    pub fn italic(mut self) -> Self {
        self.font_mut().italic = true;
        self
    }
    /// Underlined text.
    pub fn underline(mut self) -> Self {
        self.font_mut().underline = true;
        self
    }
    /// Struck-through text.
    pub fn strike(mut self) -> Self {
        self.font_mut().strike = true;
        self
    }
    /// Font size in points.
    pub fn font_size(mut self, points: f64) -> Self {
        self.font_mut().size = Some(points);
        self
    }
    /// Font name.
    pub fn font_name(mut self, name: impl Into<String>) -> Self {
        self.font_mut().name = Some(name.into());
        self
    }
    /// Text colour.
    pub fn font_color(mut self, color: Color) -> Self {
        self.font_mut().color = Some(color);
        self
    }
    /// Solid background colour.
    pub fn fill_color(mut self, color: Color) -> Self {
        self.fill = Some(Fill::solid(color));
        self
    }
    /// Background.
    pub fn fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
        self
    }
    /// Borders.
    pub fn border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }
    /// Number format.
    pub fn number_format(mut self, format: NumberFormat) -> Self {
        self.number_format = Some(format);
        self
    }
    /// Horizontal alignment.
    pub fn horizontal(mut self, h: HorizontalAlignment) -> Self {
        self.alignment_mut().horizontal = Some(h);
        self
    }
    /// Vertical alignment.
    pub fn vertical(mut self, v: VerticalAlignment) -> Self {
        self.alignment_mut().vertical = Some(v);
        self
    }
    /// Wrap text.
    pub fn wrap_text(mut self) -> Self {
        self.alignment_mut().wrap_text = true;
        self
    }
    /// Indentation level.
    pub fn indent(mut self, level: u32) -> Self {
        self.alignment_mut().indent = Some(level);
        self
    }
    /// Text rotation in degrees.
    pub fn rotation(mut self, degrees: u32) -> Self {
        self.alignment_mut().rotation = Some(degrees);
        self
    }
    /// Stacked (vertical) text.
    pub fn vertical_text(mut self) -> Self {
        self.alignment_mut().vertical_text = true;
        self
    }
    /// Shrink text to fit the cell.
    pub fn shrink_to_fit(mut self) -> Self {
        self.alignment_mut().shrink_to_fit = true;
        self
    }
    /// Underline with a specific style (double, accounting…).
    pub fn underline_style(mut self, style: UnderlineStyle) -> Self {
        let f = self.font_mut();
        f.underline = style != UnderlineStyle::None;
        f.underline_style =
            (style != UnderlineStyle::Single && style != UnderlineStyle::None).then_some(style);
        self
    }
    /// Double underline.
    pub fn double_underline(self) -> Self {
        self.underline_style(UnderlineStyle::Double)
    }
    /// Superscript text.
    pub fn superscript(mut self) -> Self {
        self.font_mut().vertical_align = Some(FontVerticalAlign::Superscript);
        self
    }
    /// Subscript text.
    pub fn subscript(mut self) -> Self {
        self.font_mut().vertical_align = Some(FontVerticalAlign::Subscript);
        self
    }
    /// Protection flags.
    pub fn protection(mut self, protection: CellProtection) -> Self {
        self.protection = Some(protection);
        self
    }
    /// The cell stays editable when the sheet is protected.
    pub fn unlocked(mut self) -> Self {
        self.protection.get_or_insert_with(CellProtection::default).locked = false;
        self
    }
    /// The formula is hidden when the sheet is protected.
    pub fn formula_hidden(mut self) -> Self {
        self.protection.get_or_insert_with(CellProtection::default).hidden = true;
        self
    }
}

/// Format code of a built-in number format (ECMA-376 Part 1 §18.8.30).
pub fn builtin_format_code(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// First id available for custom number formats.
const FIRST_CUSTOM_NUMFMT: u32 = 164;

/// One entry of a style collection: a typed record or an `mc:AlternateContent`
/// wrapper that stands for one record (whose fallback is used for reading).
enum Slot<'a, T> {
    Typed(usize, &'a T),
    Alternate(&'a RawElement),
}

fn is_alternate(e: &ExtraChild) -> bool {
    e.element.name.is(Ns::MC, "AlternateContent")
}

/// Entries of a collection whose records live in field 0, in document order.
fn slots<'a, T>(items: &'a [T], extras: &'a [ExtraChild]) -> Vec<Slot<'a, T>> {
    let mut out = Vec::with_capacity(items.len());
    let alternates = || extras.iter().filter(|e| is_alternate(e));
    for (k, item) in items.iter().enumerate() {
        out.extend(
            alternates()
                .filter(|e| e.anchor == 0 && e.index as usize == k)
                .map(|e| Slot::Alternate(&e.element)),
        );
        out.push(Slot::Typed(k, item));
    }
    out.extend(
        alternates()
            .filter(|e| e.anchor > 0 || e.index as usize >= items.len())
            .map(|e| Slot::Alternate(&e.element)),
    );
    out
}

/// Index (as used by references) of typed record `k`.
fn real_index<T>(items: &[T], extras: &[ExtraChild], k: usize) -> u32 {
    slots(items, extras)
        .iter()
        .position(|s| matches!(s, Slot::Typed(i, _) if *i == k))
        .expect("typed record is listed") as u32
}

/// The record an `mc:AlternateContent` stands for: its fallback, else its first choice.
fn alternate_record<T: XmlRead>(ac: &RawElement) -> Option<T> {
    let branch = ac
        .child(Ns::MC, "Fallback")
        .or_else(|| ac.child(Ns::MC, "Choice"))?;
    branch.elements().next()?.to_typed().ok()
}

/// The record referenced by index `real`.
fn record_at<T: XmlRead + Clone>(items: &[T], extras: &[ExtraChild], real: u32) -> Option<T> {
    match slots(items, extras).into_iter().nth(real as usize)? {
        Slot::Typed(_, item) => Some(item.clone()),
        Slot::Alternate(ac) => alternate_record(ac),
    }
}

/// Number of entries (typed records plus alternate-content wrappers).
fn entry_count<T>(items: &[T], extras: &[ExtraChild]) -> u32 {
    (items.len() + extras.iter().filter(|e| is_alternate(e)).count()) as u32
}

/// Adds `record` unless an equal typed record exists; returns its reference index.
fn intern<T: PartialEq>(items: &mut Vec<T>, extras: &[ExtraChild], record: T) -> u32 {
    let k = match items.iter().position(|r| *r == record) {
        Some(k) => k,
        None => {
            items.push(record);
            items.len() - 1
        }
    };
    real_index(items, extras, k)
}

/// The stylesheet of a workbook with record deduplication.
///
/// Style collections may contain `mc:AlternateContent` wrappers (written by
/// some producers for extended records); each counts as one entry, so
/// indices are resolved against the full list of entries.
#[derive(Debug, Clone)]
pub struct Styles {
    sheet: sml::CT_Stylesheet,
    date_xfs: Vec<bool>,
    dirty: bool,
}

fn default_font() -> sml::CT_Font {
    sml::CT_Font {
        choice: vec![
            sml::CT_Font_Choice::Sz(Box::new(sml::CT_FontSize {
                val: Some(11.0),
                ..Default::default()
            })),
            sml::CT_Font_Choice::Name(Box::new(sml::CT_FontName {
                val: Some("Calibri".into()),
                ..Default::default()
            })),
            sml::CT_Font_Choice::Family(Box::new(sml::CT_FontFamily {
                val: Some(2),
                ..Default::default()
            })),
        ],
        ..Default::default()
    }
}

fn pattern_fill(pattern: PatternType) -> sml::CT_Fill {
    sml::CT_Fill {
        choice: Some(sml::CT_Fill_Choice::PatternFill(Box::new(sml::CT_PatternFill {
            pattern_type: Some(pattern),
            ..Default::default()
        }))),
        ..Default::default()
    }
}

fn empty_border() -> sml::CT_Border {
    sml::CT_Border {
        left: Some(Box::default()),
        right: Some(Box::default()),
        top: Some(Box::default()),
        bottom: Some(Box::default()),
        diagonal: Some(Box::default()),
        ..Default::default()
    }
}

fn default_xf(with_parent: bool) -> sml::CT_Xf {
    sml::CT_Xf {
        num_fmt_id: Some(0),
        font_id: Some(0),
        fill_id: Some(0),
        border_id: Some(0),
        xf_id: with_parent.then_some(0),
        ..Default::default()
    }
}

/// A new stylesheet with the records every workbook needs.
pub fn default_stylesheet() -> sml::CT_Stylesheet {
    let mut s = Styles {
        sheet: sml::CT_Stylesheet::default(),
        date_xfs: Vec::new(),
        dirty: false,
    };
    s.ensure_defaults();
    let mut sheet = s.sheet;
    // New stylesheets carry counts (as Excel writes them); they are kept current on save.
    if let Some(n) = sheet.num_fmts.as_mut() {
        n.count = Some(0);
    }
    for count in [
        sheet.fonts.as_mut().map(|x| &mut x.count),
        sheet.fills.as_mut().map(|x| &mut x.count),
        sheet.borders.as_mut().map(|x| &mut x.count),
        sheet.cell_style_xfs.as_mut().map(|x| &mut x.count),
        sheet.cell_xfs.as_mut().map(|x| &mut x.count),
        sheet.cell_styles.as_mut().map(|x| &mut x.count),
    ]
    .into_iter()
    .flatten()
    {
        *count = Some(0);
    }
    sheet.dxfs = Some(Box::new(sml::CT_Dxfs {
        count: Some(0),
        ..Default::default()
    }));
    sheet.table_styles = Some(Box::new(sml::CT_TableStyles {
        count: Some(0),
        default_table_style: Some("TableStyleMedium2".into()),
        default_pivot_style: Some("PivotStyleLight16".into()),
        ..Default::default()
    }));
    sheet
}

impl Styles {
    /// Wraps a stylesheet.
    pub fn from_stylesheet(sheet: sml::CT_Stylesheet) -> Self {
        let mut s = Styles {
            sheet,
            date_xfs: Vec::new(),
            dirty: false,
        };
        s.recompute_dates();
        s
    }

    /// A new default stylesheet (marked as changed so that it gets saved).
    pub fn new_default() -> Self {
        let mut s = Self::from_stylesheet(default_stylesheet());
        s.dirty = true;
        s
    }

    /// The typed stylesheet.
    pub fn stylesheet(&self) -> &sml::CT_Stylesheet {
        &self.sheet
    }

    /// Mutable typed stylesheet (marks the stylesheet as changed).
    pub fn stylesheet_mut(&mut self) -> &mut sml::CT_Stylesheet {
        self.dirty = true;
        &mut self.sheet
    }

    /// Whether the stylesheet changed since it was loaded.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The cell format with reference index `index`.
    pub fn xf(&self, index: u32) -> Option<sml::CT_Xf> {
        let xfs = self.sheet.cell_xfs.as_deref()?;
        record_at(&xfs.xf, &xfs.extra_children, index)
    }

    fn font(&self, index: u32) -> Option<sml::CT_Font> {
        let fonts = self.sheet.fonts.as_deref()?;
        record_at(&fonts.font, &fonts.extra_children, index)
    }

    fn fill(&self, index: u32) -> Option<sml::CT_Fill> {
        let fills = self.sheet.fills.as_deref()?;
        record_at(&fills.fill, &fills.extra_children, index)
    }

    fn border(&self, index: u32) -> Option<sml::CT_Border> {
        let borders = self.sheet.borders.as_deref()?;
        record_at(&borders.border, &borders.extra_children, index)
    }

    fn recompute_dates(&mut self) {
        let count = self.len() as u32;
        let dates: Vec<bool> = (0..count)
            .map(|i| {
                let id = self.xf(i).and_then(|xf| xf.num_fmt_id).unwrap_or(0);
                is_date_format(id, self.format_code(id).as_deref())
            })
            .collect();
        self.date_xfs = dates;
    }

    /// Format code for a number format id (custom or built-in).
    pub fn format_code(&self, num_fmt_id: u32) -> Option<String> {
        let custom = self
            .sheet
            .num_fmts
            .as_ref()
            .and_then(|n| n.num_fmt.iter().find(|f| f.num_fmt_id == Some(num_fmt_id)))
            .and_then(|f| f.format_code.clone());
        custom.or_else(|| builtin_format_code(num_fmt_id).map(str::to_owned))
    }

    /// Number format id and code of a cell format.
    pub fn number_format_of(&self, style: StyleId) -> (u32, Option<String>) {
        let id = self.xf(style.0).and_then(|xf| xf.num_fmt_id).unwrap_or(0);
        (id, self.format_code(id))
    }

    /// Whether cells with this format display a date or time.
    pub fn is_date_style(&self, style: u32) -> bool {
        self.date_xfs.get(style as usize).copied().unwrap_or(false)
    }

    /// Number of cell formats.
    pub fn len(&self) -> usize {
        self.sheet
            .cell_xfs
            .as_deref()
            .map_or(0, |x| entry_count(&x.xf, &x.extra_children) as usize)
    }

    /// Whether there are no cell formats.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_defaults(&mut self) {
        let s = &mut self.sheet;
        let fonts = s.fonts.get_or_insert_with(Box::default);
        if entry_count(&fonts.font, &fonts.extra_children) == 0 {
            fonts.font.push(default_font());
        }
        let fills = s.fills.get_or_insert_with(Box::default);
        if entry_count(&fills.fill, &fills.extra_children) == 0 {
            fills.fill.push(pattern_fill(PatternType::None));
        }
        if entry_count(&fills.fill, &fills.extra_children) < 2 {
            fills.fill.push(pattern_fill(PatternType::Gray125));
        }
        let borders = s.borders.get_or_insert_with(Box::default);
        if entry_count(&borders.border, &borders.extra_children) == 0 {
            borders.border.push(empty_border());
        }
        let style_xfs = s.cell_style_xfs.get_or_insert_with(Box::default);
        if entry_count(&style_xfs.xf, &style_xfs.extra_children) == 0 {
            style_xfs.xf.push(default_xf(false));
        }
        let xfs = s.cell_xfs.get_or_insert_with(Box::default);
        if entry_count(&xfs.xf, &xfs.extra_children) == 0 {
            xfs.xf.push(default_xf(true));
        }
        let cell_styles = s.cell_styles.get_or_insert_with(Box::default);
        if cell_styles.cell_style.is_empty() {
            cell_styles.cell_style.push(sml::CT_CellStyle {
                name: Some("Normal".into()),
                xf_id: Some(0),
                builtin_id: Some(0),
                ..Default::default()
            });
        }
    }

    fn intern_font(&mut self, font: &Font) -> u32 {
        let base = self.font(0).unwrap_or_else(default_font);
        let record = font_record(font, Some(&base));
        let fonts = self.sheet.fonts.get_or_insert_with(Box::default);
        intern(&mut fonts.font, &fonts.extra_children, record)
    }

    fn intern_fill(&mut self, fill: &Fill) -> u32 {
        let record = fill_record(fill, false);
        let fills = self.sheet.fills.get_or_insert_with(Box::default);
        intern(&mut fills.fill, &fills.extra_children, record)
    }

    fn intern_border(&mut self, border: &Border) -> u32 {
        let record = border_record(border);
        let borders = self.sheet.borders.get_or_insert_with(Box::default);
        intern(&mut borders.border, &borders.extra_children, record)
    }
}

/// A font record: the properties of `font`, inheriting unset name, size,
/// colour, family, charset and scheme from `base` when given.
fn font_record(font: &Font, base: Option<&sml::CT_Font>) -> sml::CT_Font {
    let mut size = None;
    let mut color = None;
    let mut name = None;
    let mut family = None;
    let mut charset = None;
    let mut scheme = None;
    for c in base.map_or(&[][..], |b| &b.choice) {
        match c {
            sml::CT_Font_Choice::Sz(v) => size = v.val,
            sml::CT_Font_Choice::Color(v) => color = Some((**v).clone()),
            sml::CT_Font_Choice::Name(v) => name = v.val.clone(),
            sml::CT_Font_Choice::Family(v) => family = Some((**v).clone()),
            sml::CT_Font_Choice::Charset(v) => charset = Some((**v).clone()),
            sml::CT_Font_Choice::Scheme(v) => scheme = Some((**v).clone()),
            _ => {}
        }
    }
    if let Some(n) = &font.name {
        if name.as_deref() != Some(n.as_str()) {
            // A theme font scheme would override an explicit typeface.
            scheme = None;
            charset = None;
        }
        name = Some(n.clone());
    }
    if let Some(s) = font.size {
        size = Some(s);
    }
    if let Some(c) = font.color {
        color = Some(c.to_ct());
    }
    let flag = || Box::new(sml::CT_BooleanProperty::default());
    let mut choice = Vec::new();
    if font.bold {
        choice.push(sml::CT_Font_Choice::B(flag()));
    }
    if font.italic {
        choice.push(sml::CT_Font_Choice::I(flag()));
    }
    if font.strike {
        choice.push(sml::CT_Font_Choice::Strike(flag()));
    }
    if font.underline || font.underline_style.is_some() {
        choice.push(sml::CT_Font_Choice::U(Box::new(sml::CT_UnderlineProperty {
            val: font.underline_style.filter(|u| *u != UnderlineStyle::Single),
            ..Default::default()
        })));
    }
    if let Some(v) = font.vertical_align {
        choice.push(sml::CT_Font_Choice::VertAlign(Box::new(
            sml::CT_VerticalAlignFontProperty {
                val: Some(v),
                ..Default::default()
            },
        )));
    }
    if let Some(s) = size {
        choice.push(sml::CT_Font_Choice::Sz(Box::new(sml::CT_FontSize {
            val: Some(s),
            ..Default::default()
        })));
    }
    if let Some(c) = color {
        choice.push(sml::CT_Font_Choice::Color(Box::new(c)));
    }
    if let Some(n) = name {
        choice.push(sml::CT_Font_Choice::Name(Box::new(sml::CT_FontName {
            val: Some(n),
            ..Default::default()
        })));
    }
    if let Some(f) = family {
        choice.push(sml::CT_Font_Choice::Family(Box::new(f)));
    }
    if let Some(c) = charset {
        choice.push(sml::CT_Font_Choice::Charset(Box::new(c)));
    }
    if let Some(s) = scheme {
        choice.push(sml::CT_Font_Choice::Scheme(Box::new(s)));
    }
    sml::CT_Font {
        choice,
        ..Default::default()
    }
}

/// A fill record. In differential formats (`dxf == true`) a solid fill
/// carries its colour in `bgColor`, as Excel expects (§18.8.20).
fn fill_record(fill: &Fill, dxf: bool) -> sml::CT_Fill {
    if let Some(g) = &fill.gradient {
        let mut record = sml::CT_GradientFill {
            stop: g
                .stops
                .iter()
                .map(|(position, color)| sml::CT_GradientStop {
                    position: Some(*position),
                    color: Some(Box::new(color.to_ct())),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        match g.kind {
            GradientKind::Linear { degree } => {
                record.degree = (degree != 0.0).then_some(degree);
            }
            GradientKind::Path {
                left,
                right,
                top,
                bottom,
            } => {
                record.type_ = Some(sml::ST_GradientType::Path);
                record.left = Some(left);
                record.right = Some(right);
                record.top = Some(top);
                record.bottom = Some(bottom);
            }
        }
        return sml::CT_Fill {
            choice: Some(sml::CT_Fill_Choice::GradientFill(Box::new(record))),
            ..Default::default()
        };
    }
    let color = |c: Option<Color>| c.map(|c| Box::new(c.to_ct()));
    let pattern = if dxf && fill.pattern == PatternType::Solid {
        sml::CT_PatternFill {
            bg_color: color(fill.fg_color.or(fill.bg_color)),
            ..Default::default()
        }
    } else {
        let bg = match (fill.bg_color, fill.pattern) {
            (Some(c), _) => Some(Box::new(c.to_ct())),
            (None, PatternType::Solid) if !dxf => Some(Box::new(sml::CT_Color {
                indexed: Some(64),
                ..Default::default()
            })),
            _ => None,
        };
        sml::CT_PatternFill {
            pattern_type: Some(fill.pattern),
            fg_color: color(fill.fg_color),
            bg_color: bg,
            ..Default::default()
        }
    };
    sml::CT_Fill {
        choice: Some(sml::CT_Fill_Choice::PatternFill(Box::new(pattern))),
        ..Default::default()
    }
}

fn border_record(border: &Border) -> sml::CT_Border {
    let side = |s: &Option<BorderSide>| -> Option<Box<sml::CT_BorderPr>> {
        Some(Box::new(match s {
            Some(s) => sml::CT_BorderPr {
                style: Some(s.style),
                color: s.color.map(|c| Box::new(c.to_ct())),
                ..Default::default()
            },
            None => sml::CT_BorderPr::default(),
        }))
    };
    sml::CT_Border {
        diagonal_up: border.diagonal_up.then_some(true),
        diagonal_down: border.diagonal_down.then_some(true),
        left: side(&border.left),
        right: side(&border.right),
        top: side(&border.top),
        bottom: side(&border.bottom),
        diagonal: side(&border.diagonal),
        ..Default::default()
    }
}

fn alignment_record(a: &Alignment) -> sml::CT_CellAlignment {
    let rotation = if a.vertical_text {
        Some(sml::ST_TextRotation::Member2(sml::ST_TextRotation_Member2::V255))
    } else {
        a.rotation.map(|r| sml::ST_TextRotation::Member1(u64::from(r)))
    };
    sml::CT_CellAlignment {
        horizontal: a.horizontal,
        vertical: a.vertical,
        wrap_text: a.wrap_text.then_some(true),
        indent: a.indent,
        text_rotation: rotation,
        shrink_to_fit: a.shrink_to_fit.then_some(true),
        ..Default::default()
    }
}

fn alignment_properties(a: &sml::CT_CellAlignment) -> Alignment {
    Alignment {
        horizontal: a.horizontal,
        vertical: a.vertical,
        wrap_text: a.wrap_text.unwrap_or(false),
        indent: a.indent,
        rotation: match a.text_rotation {
            Some(sml::ST_TextRotation::Member1(r)) if r != 255 => Some(r as u32),
            _ => None,
        },
        vertical_text: matches!(
            a.text_rotation,
            Some(sml::ST_TextRotation::Member2(_) | sml::ST_TextRotation::Member1(255))
        ),
        shrink_to_fit: a.shrink_to_fit.unwrap_or(false),
    }
}

fn protection_record(p: &CellProtection) -> sml::CT_CellProtection {
    sml::CT_CellProtection {
        locked: (!p.locked).then_some(false),
        hidden: p.hidden.then_some(true),
        ..Default::default()
    }
}

fn protection_properties(p: &sml::CT_CellProtection) -> CellProtection {
    CellProtection {
        locked: p.locked.unwrap_or(true),
        hidden: p.hidden.unwrap_or(false),
    }
}

fn fill_properties(fill: &sml::CT_Fill, dxf: bool) -> Option<Fill> {
    match fill.choice.as_ref()? {
        sml::CT_Fill_Choice::PatternFill(p) => {
            let fg = p.fg_color.as_deref().and_then(Color::from_ct);
            let bg = p.bg_color.as_deref().and_then(Color::from_ct);
            if dxf && p.pattern_type.is_none_or(|t| t == PatternType::Solid) {
                // Differential solid fills carry their colour in bgColor.
                return Some(Fill::solid(bg.or(fg)?));
            }
            let pattern = p.pattern_type.filter(|p| *p != PatternType::None)?;
            Some(Fill {
                pattern,
                fg_color: fg,
                bg_color: bg.filter(|c| *c != Color::Indexed(64)),
                gradient: None,
            })
        }
        sml::CT_Fill_Choice::GradientFill(g) => {
            let kind = if g.type_ == Some(sml::ST_GradientType::Path) {
                GradientKind::Path {
                    left: g.left.unwrap_or(0.0),
                    right: g.right.unwrap_or(0.0),
                    top: g.top.unwrap_or(0.0),
                    bottom: g.bottom.unwrap_or(0.0),
                }
            } else {
                GradientKind::Linear {
                    degree: g.degree.unwrap_or(0.0),
                }
            };
            let stops = g
                .stop
                .iter()
                .filter_map(|s| Some((s.position?, s.color.as_deref().and_then(Color::from_ct)?)))
                .collect();
            Some(Fill::gradient(GradientFill { kind, stops }))
        }
        sml::CT_Fill_Choice::Other(_) => None,
    }
}

fn border_properties(b: &sml::CT_Border) -> Border {
    let side = |s: &Option<Box<sml::CT_BorderPr>>| {
        s.as_ref().and_then(|s| {
            s.style
                .filter(|st| *st != BorderStyle::None)
                .map(|style| BorderSide {
                    style,
                    color: s.color.as_deref().and_then(Color::from_ct),
                })
        })
    };
    Border {
        left: side(&b.left).or(side(&b.start)),
        right: side(&b.right).or(side(&b.end)),
        top: side(&b.top),
        bottom: side(&b.bottom),
        diagonal: side(&b.diagonal),
        diagonal_up: b.diagonal_up.unwrap_or(false),
        diagonal_down: b.diagonal_down.unwrap_or(false),
    }
}

impl Styles {
    fn intern_number_format(&mut self, format: &NumberFormat) -> u32 {
        let code = match format {
            NumberFormat::Builtin(id) => return *id,
            NumberFormat::Custom(code) => code,
        };
        if let Some(id) = (0..FIRST_CUSTOM_NUMFMT).find(|&id| builtin_format_code(id) == Some(code.as_str()))
        {
            return id;
        }
        let num_fmts = self.sheet.num_fmts.get_or_insert_with(Box::default);
        if let Some(id) = num_fmts
            .num_fmt
            .iter()
            .find(|f| f.format_code.as_deref() == Some(code.as_str()))
            .and_then(|f| f.num_fmt_id)
        {
            return id;
        }
        let id = num_fmts
            .num_fmt
            .iter()
            .filter_map(|f| f.num_fmt_id)
            .max()
            .map_or(FIRST_CUSTOM_NUMFMT, |m| (m + 1).max(FIRST_CUSTOM_NUMFMT));
        num_fmts.num_fmt.push(sml::CT_NumFmt {
            num_fmt_id: Some(id),
            format_code: Some(code.clone()),
            ..Default::default()
        });
        id
    }

    fn intern_xf(&mut self, xf: sml::CT_Xf) -> StyleId {
        let xfs = self.sheet.cell_xfs.get_or_insert_with(Box::default);
        let id = intern(&mut xfs.xf, &xfs.extra_children, xf);
        self.dirty = true;
        self.recompute_dates();
        StyleId(id)
    }

    /// The format records of a style: font, fill, border and number format
    /// are interned; the result has no parent style (`xfId`).
    fn build_xf(&mut self, style: &CellStyle) -> sml::CT_Xf {
        self.ensure_defaults();
        let mut xf = default_xf(false);
        if let Some(font) = &style.font {
            xf.font_id = Some(self.intern_font(font));
            xf.apply_font = Some(true);
        }
        if let Some(fill) = &style.fill {
            xf.fill_id = Some(self.intern_fill(fill));
            xf.apply_fill = Some(true);
        }
        if let Some(border) = &style.border {
            xf.border_id = Some(self.intern_border(border));
            xf.apply_border = Some(true);
        }
        if let Some(fmt) = &style.number_format {
            xf.num_fmt_id = Some(self.intern_number_format(fmt));
            xf.apply_number_format = Some(true);
        }
        if let Some(a) = &style.alignment {
            xf.alignment = Some(Box::new(alignment_record(a)));
            xf.apply_alignment = Some(true);
        }
        if let Some(p) = &style.protection {
            xf.protection = Some(Box::new(protection_record(p)));
            xf.apply_protection = Some(true);
        }
        xf
    }

    /// Registers a style and returns its cell format index.
    pub fn add(&mut self, style: &CellStyle) -> StyleId {
        let mut xf = self.build_xf(style);
        xf.xf_id = Some(0);
        self.intern_xf(xf)
    }

    /// Returns a cell format equal to `base` but with another number format.
    pub fn with_number_format(&mut self, base: Option<StyleId>, format: &NumberFormat) -> StyleId {
        self.ensure_defaults();
        let mut xf = base
            .and_then(|b| self.xf(b.0))
            .unwrap_or_else(|| default_xf(true));
        xf.num_fmt_id = Some(self.intern_number_format(format));
        xf.apply_number_format = Some(true);
        self.intern_xf(xf)
    }

    /// Registers a named cell style (shown in Excel's *Cell Styles*
    /// gallery) and returns a cell format that applies it.
    ///
    /// Adding a style whose name exists returns the existing style's cell
    /// format when the formatting is identical and fails otherwise.
    pub fn add_named_style(&mut self, name: &str, style: &CellStyle) -> Result<StyleId, String> {
        self.add_style_entry(name, None, style)
    }

    /// Registers the built-in cell style `builtin_id` (e.g. 8 = Hyperlink).
    pub(crate) fn add_builtin_style(
        &mut self,
        name: &str,
        builtin_id: u32,
        style: &CellStyle,
    ) -> Result<StyleId, String> {
        self.add_style_entry(name, Some(builtin_id), style)
    }

    fn add_style_entry(
        &mut self,
        name: &str,
        builtin_id: Option<u32>,
        style: &CellStyle,
    ) -> Result<StyleId, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(format!("invalid style name {name:?}"));
        }
        let parent = self.build_xf(style);
        let existing = self.sheet.cell_styles.as_ref().and_then(|c| {
            c.cell_style
                .iter()
                .find(|s| s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                .map(|s| s.xf_id.unwrap_or(0))
        });
        let xf_id = match existing {
            Some(id) => {
                let xfs = self.sheet.cell_style_xfs.as_deref();
                let current: Option<sml::CT_Xf> = xfs.and_then(|x| record_at(&x.xf, &x.extra_children, id));
                if current.as_ref() != Some(&parent) {
                    match (builtin_id, current) {
                        // A built-in style defined by the file (e.g. Excel's
                        // theme-coloured Hyperlink) is used as it is.
                        (Some(_), Some(mut record)) => {
                            record.xf_id = Some(id);
                            record.apply_font = record.font_id.map(|_| true);
                            return Ok(self.intern_xf(record));
                        }
                        _ => return Err(format!("a different cell style named {name:?} exists")),
                    }
                }
                id
            }
            None => {
                let xfs = self.sheet.cell_style_xfs.get_or_insert_with(Box::default);
                // A new entry even when an identical one exists: each named style owns its record.
                xfs.xf.push(parent.clone());
                let id = real_index(&xfs.xf, &xfs.extra_children, xfs.xf.len() - 1);
                let styles = self.sheet.cell_styles.get_or_insert_with(Box::default);
                styles.cell_style.push(sml::CT_CellStyle {
                    name: Some(name.to_owned()),
                    xf_id: Some(id),
                    builtin_id,
                    ..Default::default()
                });
                id
            }
        };
        let mut xf = parent;
        xf.xf_id = Some(xf_id);
        Ok(self.intern_xf(xf))
    }

    /// Names of the named cell styles.
    pub fn named_styles(&self) -> Vec<String> {
        self.sheet
            .cell_styles
            .as_ref()
            .map(|c| c.cell_style.iter().filter_map(|s| s.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Name of the named cell style a cell format is based on.
    pub fn style_name(&self, id: StyleId) -> Option<String> {
        let parent = self.xf(id.0)?.xf_id.unwrap_or(0);
        self.sheet
            .cell_styles
            .as_ref()?
            .cell_style
            .iter()
            .find(|s| s.xf_id.unwrap_or(0) == parent)
            .and_then(|s| s.name.clone())
    }

    /// Registers a differential format (used by conditional formatting and
    /// table styles) and returns its `dxfId`. Identical formats are shared.
    pub fn add_dxf(&mut self, style: &CellStyle) -> u32 {
        let mut dxf = sml::CT_Dxf::default();
        if let Some(font) = &style.font {
            dxf.font = Some(Box::new(font_record(font, None)));
        }
        if let Some(fmt) = &style.number_format {
            let id = self.intern_number_format(fmt);
            let code = self.format_code(id).unwrap_or_else(|| "General".into());
            dxf.num_fmt = Some(Box::new(sml::CT_NumFmt {
                num_fmt_id: Some(id),
                format_code: Some(code),
                ..Default::default()
            }));
        }
        if let Some(fill) = &style.fill {
            dxf.fill = Some(Box::new(fill_record(fill, true)));
        }
        if let Some(a) = &style.alignment {
            dxf.alignment = Some(Box::new(alignment_record(a)));
        }
        if let Some(b) = &style.border {
            let mut record = border_record(b);
            // Unset edges are left unchanged by a differential format.
            let keep = |s: &Option<BorderSide>, r: &mut Option<Box<sml::CT_BorderPr>>| {
                if s.is_none() {
                    *r = None;
                }
            };
            keep(&b.left, &mut record.left);
            keep(&b.right, &mut record.right);
            keep(&b.top, &mut record.top);
            keep(&b.bottom, &mut record.bottom);
            keep(&b.diagonal, &mut record.diagonal);
            dxf.border = Some(Box::new(record));
        }
        if let Some(p) = &style.protection {
            dxf.protection = Some(Box::new(sml::CT_CellProtection {
                locked: Some(p.locked),
                hidden: Some(p.hidden),
                ..Default::default()
            }));
        }
        self.dirty = true;
        let dxfs = self.sheet.dxfs.get_or_insert_with(Box::default);
        intern(&mut dxfs.dxf, &dxfs.extra_children, dxf)
    }

    /// Reconstructs the formatting of differential format `id`.
    pub fn dxf_style(&self, id: u32) -> Option<CellStyle> {
        let dxfs = self.sheet.dxfs.as_deref()?;
        let dxf: sml::CT_Dxf = record_at(&dxfs.dxf, &dxfs.extra_children, id)?;
        let mut style = CellStyle::new();
        style.font = dxf.font.as_deref().map(Self::font_properties);
        style.fill = dxf.fill.as_deref().and_then(|f| fill_properties(f, true));
        style.border = dxf.border.as_deref().map(border_properties);
        style.number_format = dxf.num_fmt.as_deref().map(|n| {
            let id = n.num_fmt_id.unwrap_or(0);
            match &n.format_code {
                Some(code) if builtin_format_code(id) != Some(code.as_str()) => {
                    NumberFormat::Custom(code.clone())
                }
                _ => NumberFormat::Builtin(id),
            }
        });
        style.alignment = dxf.alignment.as_deref().map(alignment_properties);
        style.protection = dxf.protection.as_deref().map(protection_properties);
        Some(style)
    }

    /// Number of differential formats.
    pub fn dxf_count(&self) -> usize {
        self.sheet
            .dxfs
            .as_deref()
            .map_or(0, |d| entry_count(&d.dxf, &d.extra_children) as usize)
    }

    fn font_properties(font: &sml::CT_Font) -> Font {
        let mut f = Font::default();
        for c in &font.choice {
            match c {
                sml::CT_Font_Choice::B(b) => f.bold = b.val.unwrap_or(true),
                sml::CT_Font_Choice::I(b) => f.italic = b.val.unwrap_or(true),
                sml::CT_Font_Choice::Strike(b) => f.strike = b.val.unwrap_or(true),
                sml::CT_Font_Choice::U(u) => {
                    let val = u.val.unwrap_or(UnderlineStyle::Single);
                    f.underline = val != UnderlineStyle::None;
                    f.underline_style =
                        (val != UnderlineStyle::Single && val != UnderlineStyle::None).then_some(val);
                }
                sml::CT_Font_Choice::VertAlign(v) => {
                    f.vertical_align = v.val.filter(|v| *v != FontVerticalAlign::Baseline);
                }
                sml::CT_Font_Choice::Sz(s) => f.size = s.val,
                sml::CT_Font_Choice::Name(n) => f.name = n.val.clone(),
                sml::CT_Font_Choice::Color(c) => f.color = Color::from_ct(c),
                _ => {}
            }
        }
        f
    }

    /// Reconstructs the [`CellStyle`] of a cell format (best effort). Font
    /// name, size and colour are reported only where they differ from the
    /// workbook's default font; records at index 0 (the defaults) are omitted.
    pub fn cell_style(&self, id: StyleId) -> Option<CellStyle> {
        let xf = self.xf(id.0)?;
        let mut style = CellStyle::new();
        let base = self
            .font(0)
            .map(|f| Self::font_properties(&f))
            .unwrap_or_default();
        if let Some(font) = xf.font_id.filter(|&i| i > 0).and_then(|i| self.font(i)) {
            let mut f = Self::font_properties(&font);
            if f.name == base.name {
                f.name = None;
            }
            if f.size == base.size {
                f.size = None;
            }
            if f.color == base.color {
                f.color = None;
            }
            style.font = Some(f);
        }
        if let Some(fill) = xf.fill_id.filter(|&i| i > 1).and_then(|i| self.fill(i)) {
            style.fill = fill_properties(&fill, false);
        }
        if let Some(b) = xf.border_id.and_then(|i| self.border(i)) {
            let border = border_properties(&b);
            if border != Border::default() {
                style.border = Some(border);
            }
        }
        let fmt = xf.num_fmt_id.unwrap_or(0);
        if fmt != 0 {
            style.number_format = Some(
                match self.sheet.num_fmts.as_ref().and_then(|n| {
                    n.num_fmt
                        .iter()
                        .find(|f| f.num_fmt_id == Some(fmt))
                        .and_then(|f| f.format_code.clone())
                }) {
                    Some(code) => NumberFormat::Custom(code),
                    None => NumberFormat::Builtin(fmt),
                },
            );
        }
        if let Some(a) = &xf.alignment {
            style.alignment = Some(alignment_properties(a));
        }
        if let Some(p) = &xf.protection {
            let p = protection_properties(p);
            if p != CellProtection::default() {
                style.protection = Some(p);
            }
        }
        Some(style)
    }

    /// The stylesheet ready to be written. Collection `count` attributes that
    /// are present are brought up to date (absent ones stay absent).
    pub fn to_stylesheet(&self) -> sml::CT_Stylesheet {
        fn refresh(count: &mut Option<u32>, value: u32) {
            if count.is_some() {
                *count = Some(value);
            }
        }
        let mut s = self.sheet.clone();
        if let Some(n) = s.num_fmts.as_mut() {
            refresh(&mut n.count, entry_count(&n.num_fmt, &n.extra_children));
        }
        if let Some(f) = s.fonts.as_mut() {
            refresh(&mut f.count, entry_count(&f.font, &f.extra_children));
        }
        if let Some(f) = s.fills.as_mut() {
            refresh(&mut f.count, entry_count(&f.fill, &f.extra_children));
        }
        if let Some(b) = s.borders.as_mut() {
            refresh(&mut b.count, entry_count(&b.border, &b.extra_children));
        }
        if let Some(x) = s.cell_style_xfs.as_mut() {
            refresh(&mut x.count, entry_count(&x.xf, &x.extra_children));
        }
        if let Some(x) = s.cell_xfs.as_mut() {
            refresh(&mut x.count, entry_count(&x.xf, &x.extra_children));
        }
        if let Some(c) = s.cell_styles.as_mut() {
            refresh(&mut c.count, entry_count(&c.cell_style, &c.extra_children));
        }
        if let Some(d) = s.dxfs.as_mut() {
            refresh(&mut d.count, entry_count(&d.dxf, &d.extra_children));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stylesheet_has_required_records() {
        let s = Styles::new_default();
        let sheet = s.to_stylesheet();
        assert_eq!(sheet.fonts.as_ref().unwrap().count, Some(1));
        let fills = &sheet.fills.as_ref().unwrap().fill;
        assert_eq!(fills.len(), 2, "none and gray125 are reserved");
        assert_eq!(sheet.cell_xfs.as_ref().unwrap().xf.len(), 1);
        assert_eq!(
            sheet.cell_styles.as_ref().unwrap().cell_style[0].name.as_deref(),
            Some("Normal")
        );
        assert!(s.is_dirty());
        assert_eq!(s.len(), 1);
        assert!(!s.is_empty());
    }

    #[test]
    fn identical_styles_are_deduplicated() {
        let mut s = Styles::new_default();
        let a = s.add(&CellStyle::new().bold());
        let b = s.add(&CellStyle::new().bold());
        assert_eq!(a, b);
        let c = s.add(&CellStyle::new().bold().italic());
        assert_ne!(a, c);
        let fonts = s.to_stylesheet().fonts.unwrap().font.len();
        assert_eq!(fonts, 3, "default, bold, bold+italic");
        let d = s.add(&CellStyle::new().fill_color(Color::Rgb(1, 2, 3)));
        let e = s.add(&CellStyle::new().fill_color(Color::Rgb(1, 2, 3)).bold());
        assert_ne!(d, e);
        assert_eq!(
            s.to_stylesheet().fills.unwrap().fill.len(),
            3,
            "the fill is shared"
        );
        assert_eq!(
            s.add(&CellStyle::new()),
            StyleId(0),
            "an empty style is the default format"
        );
    }

    #[test]
    fn number_formats() {
        let mut s = Styles::new_default();
        let pct = s.add(&CellStyle::new().number_format(NumberFormat::PERCENT));
        assert_eq!(s.number_format_of(pct), (9, Some("0%".into())));
        let c1 = s.add(&CellStyle::new().number_format(NumberFormat::custom("0.000")));
        let c2 = s.add(&CellStyle::new().number_format(NumberFormat::custom("0.000")));
        assert_eq!(c1, c2);
        assert_eq!(s.number_format_of(c1), (164, Some("0.000".into())));
        let c3 = s.add(&CellStyle::new().number_format(NumberFormat::custom("yyyy-mm-dd")));
        assert_eq!(s.number_format_of(c3).0, 165);
        assert!(s.is_date_style(c3.0));
        assert!(!s.is_date_style(c1.0));
        let builtin_by_code = s.add(&CellStyle::new().number_format(NumberFormat::custom("0.00")));
        assert_eq!(
            s.number_format_of(builtin_by_code).0,
            2,
            "codes of built-in formats map to their ids"
        );
        let date = s.with_number_format(Some(pct), &NumberFormat::DATE);
        assert!(s.is_date_style(date.0));
        assert!(!s.is_date_style(999));
        assert_eq!(s.format_code(22).as_deref(), Some("m/d/yy h:mm"));
        assert_eq!(s.format_code(500), None);
    }

    #[test]
    fn fonts_inherit_the_default_font() {
        let mut s = Styles::new_default();
        let id = s.add(&CellStyle::new().bold().font_size(14.0));
        let font = &s.stylesheet().fonts.as_ref().unwrap().font[1];
        let written = Styles::font_properties(font);
        assert_eq!(
            written.name.as_deref(),
            Some("Calibri"),
            "the typeface is inherited in the record"
        );
        assert_eq!(written.size, Some(14.0));
        let f = s.cell_style(id).unwrap().font.unwrap();
        assert!(f.bold);
        assert_eq!(f.size, Some(14.0));
        assert_eq!(f.name, None, "inherited properties are not reported");
        let named = s.add(
            &CellStyle::new()
                .font_name("Arial")
                .font_color(Color::Rgb(255, 0, 0)),
        );
        let f = s.cell_style(named).unwrap().font.unwrap();
        assert_eq!(f.name.as_deref(), Some("Arial"));
        assert_eq!(f.color, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(f.size, None);
        assert_eq!(
            s.cell_style(StyleId(0)).unwrap(),
            CellStyle::new(),
            "the default format has no overrides"
        );
    }

    #[test]
    fn styles_round_trip_through_cell_style() {
        let mut s = Styles::new_default();
        let style = CellStyle::new()
            .bold()
            .italic()
            .underline()
            .strike()
            .font_size(12.0)
            .font_name("Arial")
            .font_color(Color::Theme(4))
            .fill_color(Color::Rgb(0xDD, 0xEE, 0xFF))
            .border(Border::all(BorderStyle::Thin, Some(Color::Indexed(8))))
            .number_format(NumberFormat::custom("#,##0.0"))
            .horizontal(HorizontalAlignment::Center)
            .vertical(VerticalAlignment::Top)
            .wrap_text()
            .indent(2)
            .rotation(45);
        let id = s.add(&style);
        let back = s.cell_style(id).unwrap();
        assert_eq!(back, style);
        let xml = sml::elements::STYLE_SHEET.to_xml(&s.to_stylesheet());
        let reparsed = Styles::from_stylesheet(sml::elements::STYLE_SHEET.parse(&xml).unwrap());
        assert_eq!(reparsed.cell_style(id).unwrap(), style);
        assert!(s.cell_style(StyleId(99)).is_none());
    }

    #[test]
    fn alternate_content_entries_occupy_index_slots() {
        let xml = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
          <fonts count="4"><font><sz val="11"/><name val="A"/></font><font><b/><sz val="11"/><name val="A"/></font>
            <mc:AlternateContent><mc:Choice Requires="hs"><font><sz val="9"/><name val="A"/></font></mc:Choice>
              <mc:Fallback><font><i/><sz val="9"/><name val="A"/></font></mc:Fallback></mc:AlternateContent>
            <font><sz val="20"/><name val="A"/></font></fonts>
          <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
          <borders count="1"><border/></borders>
          <cellXfs count="3"><xf numFmtId="0" fontId="0"/>
            <mc:AlternateContent><mc:Choice Requires="hs"><xf numFmtId="14" fontId="2"/></mc:Choice>
              <mc:Fallback><xf numFmtId="14" fontId="2"/></mc:Fallback></mc:AlternateContent>
            <xf numFmtId="0" fontId="3"/></cellXfs>
        </styleSheet>"#;
        let mut s = Styles::from_stylesheet(sml::elements::STYLE_SHEET.parse(xml).unwrap());
        assert_eq!(s.len(), 3);
        assert!(s.is_date_style(1), "the fallback record of the wrapper is used");
        assert!(!s.is_date_style(2));
        assert_eq!(s.number_format_of(StyleId(1)).0, 14);
        let wrapped = s.cell_style(StyleId(1)).unwrap().font.unwrap();
        assert!(
            wrapped.italic && wrapped.size == Some(9.0),
            "font 2 is the wrapped fallback"
        );
        assert_eq!(
            s.cell_style(StyleId(2)).unwrap().font.unwrap().size,
            Some(20.0),
            "font 3 follows the wrapper"
        );
        let id = s.add(&CellStyle::new().underline());
        assert_eq!(id, StyleId(3));
        let sheet = s.to_stylesheet();
        assert_eq!(sheet.fonts.as_ref().unwrap().count, Some(5));
        assert_eq!(sheet.cell_xfs.as_ref().unwrap().count, Some(4));
        assert_eq!(
            s.xf(3).unwrap().font_id,
            Some(4),
            "new records are appended after the wrappers"
        );
        let reparsed = Styles::from_stylesheet(
            sml::elements::STYLE_SHEET
                .parse(&sml::elements::STYLE_SHEET.to_xml(&sheet))
                .unwrap(),
        );
        assert!(reparsed.cell_style(StyleId(3)).unwrap().font.unwrap().underline);
        assert!(reparsed.is_date_style(1));
        assert_eq!(s.add(&CellStyle::new().underline()), StyleId(3), "deduplicated");
    }

    #[test]
    fn missing_collections_are_created() {
        let mut s = Styles::from_stylesheet(sml::CT_Stylesheet::default());
        assert!(s.is_empty());
        let id = s.add(&CellStyle::new().fill_color(Color::Rgb(0, 0, 0)));
        assert_eq!(id, StyleId(1));
        let sheet = s.to_stylesheet();
        let fills = &sheet.fills.as_ref().unwrap().fill;
        assert_eq!(fills.len(), 3, "reserved fills come first");
        assert!(sheet.cell_style_xfs.is_some() && sheet.cell_styles.is_some());
    }

    #[test]
    fn colors() {
        assert_eq!(Color::from_hex("#FF8000"), Some(Color::Rgb(255, 128, 0)));
        assert_eq!(Color::from_hex("80ff8000"), Some(Color::Rgb(255, 128, 0)));
        assert_eq!(Color::from_hex("12345"), None);
        assert_eq!(Color::from_hex("GGGGGG"), None);
        let ct = Color::Rgb(1, 2, 3).to_ct();
        assert_eq!(ct.rgb.as_ref().unwrap().as_bytes(), &[0xFF, 1, 2, 3]);
        assert_eq!(Color::from_ct(&ct), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(Color::from_ct(&Color::Theme(3).to_ct()), Some(Color::Theme(3)));
        assert_eq!(
            Color::from_ct(&Color::Indexed(9).to_ct()),
            Some(Color::Indexed(9))
        );
        assert_eq!(Color::from_ct(&sml::CT_Color::default()), None);
    }

    #[test]
    fn differential_formats() {
        let mut s = Styles::new_default();
        let style = CellStyle::new()
            .bold()
            .font_color(Color::Rgb(1, 2, 3))
            .fill_color(Color::Rgb(4, 5, 6))
            .number_format(NumberFormat::custom("0.0%"))
            .border(Border {
                bottom: Some(BorderSide {
                    style: BorderStyle::Thin,
                    color: None,
                }),
                ..Border::default()
            });
        let a = s.add_dxf(&style);
        assert_eq!(s.add_dxf(&style), a, "identical formats are shared");
        assert_eq!(s.dxf_count(), 1);
        let dxf = &s.stylesheet().dxfs.as_ref().unwrap().dxf[0];
        let Some(sml::CT_Fill_Choice::PatternFill(p)) = &dxf.fill.as_ref().unwrap().choice else {
            panic!("pattern fill")
        };
        assert_eq!(
            p.pattern_type, None,
            "a solid differential fill has no pattern type"
        );
        assert!(
            p.bg_color.is_some() && p.fg_color.is_none(),
            "its colour is the background"
        );
        assert_eq!(
            dxf.font.as_ref().unwrap().choice.len(),
            2,
            "only the given font properties"
        );
        assert!(
            dxf.border.as_ref().unwrap().left.is_none(),
            "unset edges are omitted"
        );
        assert_eq!(dxf.num_fmt.as_ref().unwrap().format_code.as_deref(), Some("0.0%"));
        assert_eq!(s.dxf_style(a).unwrap(), style);
        let b = s.add_dxf(&CellStyle::new().number_format(NumberFormat::PERCENT));
        assert_eq!(b, 1);
        assert_eq!(s.dxf_style(b).unwrap().number_format, Some(NumberFormat::PERCENT));
        assert_eq!(s.to_stylesheet().dxfs.unwrap().count, Some(2));
        assert!(s.dxf_style(9).is_none());
    }

    #[test]
    fn named_styles_have_their_own_records() {
        let mut s = Styles::new_default();
        let style = CellStyle::new().italic().fill_color(Color::Theme(5));
        let id = s.add_named_style("Accent", &style).unwrap();
        assert_eq!(s.add_named_style("accent", &style).unwrap(), id);
        assert!(s.add_named_style("Accent", &CellStyle::new().bold()).is_err());
        assert!(s.add_named_style("  ", &style).is_err());
        assert_eq!(s.style_name(id).as_deref(), Some("Accent"));
        assert_eq!(s.style_name(StyleId(0)).as_deref(), Some("Normal"));
        assert_eq!(s.named_styles(), ["Normal", "Accent"]);
        let xf = s.xf(id.0).unwrap();
        assert_eq!(xf.xf_id, Some(1), "points to its cellStyleXfs record");
        let sheet = s.to_stylesheet();
        assert_eq!(sheet.cell_style_xfs.as_ref().unwrap().xf.len(), 2);
        assert_eq!(sheet.cell_styles.as_ref().unwrap().count, Some(2));
        assert_eq!(s.cell_style(id).unwrap(), style);
        let link = s
            .add_builtin_style("Hyperlink", 8, &CellStyle::new().underline())
            .unwrap();
        assert_eq!(
            s.stylesheet().cell_styles.as_ref().unwrap().cell_style[2].builtin_id,
            Some(8)
        );
        assert_ne!(link, id);
        // A file's own definition of a built-in style is reused, not refused.
        let theirs = s
            .add_builtin_style("Hyperlink", 8, &CellStyle::new().font_color(Color::Theme(10)))
            .unwrap();
        assert_eq!(s.style_name(theirs).as_deref(), Some("Hyperlink"));
        assert_eq!(s.xf(theirs.0).unwrap().font_id, s.xf(link.0).unwrap().font_id);
    }

    #[test]
    fn fills_borders_and_alignment_records() {
        let gradient = Fill::gradient(GradientFill {
            kind: GradientKind::Path {
                left: 0.2,
                right: 0.8,
                top: 0.2,
                bottom: 0.8,
            },
            stops: vec![(0.0, Color::Rgb(1, 1, 1)), (1.0, Color::Theme(3))],
        });
        let record = fill_record(&gradient, false);
        let Some(sml::CT_Fill_Choice::GradientFill(g)) = &record.choice else {
            panic!("gradient")
        };
        assert_eq!(g.type_, Some(sml::ST_GradientType::Path));
        assert_eq!(g.stop.len(), 2);
        assert_eq!(fill_properties(&record, false), Some(gradient));
        let linear = Fill::linear_gradient(0.0, Color::Theme(0), Color::Theme(1));
        let record = fill_record(&linear, false);
        let Some(sml::CT_Fill_Choice::GradientFill(g)) = &record.choice else {
            panic!("gradient")
        };
        assert_eq!(g.degree, None, "0 degrees is the default");
        assert_eq!(fill_properties(&record, false), Some(linear));
        let border = Border::default().with_diagonal(
            BorderSide {
                style: BorderStyle::Thick,
                color: None,
            },
            false,
            true,
        );
        let record = border_record(&border);
        assert_eq!((record.diagonal_up, record.diagonal_down), (None, Some(true)));
        assert_eq!(border_properties(&record), border);
        let a = Alignment {
            vertical_text: true,
            shrink_to_fit: true,
            ..Alignment::default()
        };
        let record = alignment_record(&a);
        assert!(matches!(
            record.text_rotation,
            Some(sml::ST_TextRotation::Member2(_))
        ));
        assert_eq!(alignment_properties(&record), a);
        assert_eq!(
            protection_properties(&protection_record(&CellProtection::default())),
            CellProtection::default()
        );
    }

    #[test]
    fn number_format_helpers() {
        assert_eq!(NumberFormat::decimal(0, false), NumberFormat::INTEGER);
        assert_eq!(NumberFormat::decimal(0, true), NumberFormat::THOUSANDS);
        assert_eq!(NumberFormat::percent(2), NumberFormat::PERCENT_DECIMAL_2);
        assert_eq!(NumberFormat::scientific(1), NumberFormat::custom("0.0E+00"));
        assert_eq!(
            NumberFormat::currency("$", 2),
            NumberFormat::custom("\"$\"#,##0.00;-\"$\"#,##0.00")
        );
        assert_eq!(
            NumberFormat::accounting("$", 2),
            NumberFormat::custom("_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"),
            "Excel's built-in accounting layout (id 44)"
        );
        assert_eq!(
            NumberFormat::accounting("€", 0),
            NumberFormat::custom("_(\"€\"* #,##0_);_(\"€\"* \\(#,##0\\);_(\"€\"* \"-\"_);_(@_)")
        );
        assert_eq!(NumberFormat::TIME_12H.code(), Some("h:mm AM/PM"));
        assert_eq!(NumberFormat::Builtin(5).code(), None);
        assert_eq!(NumberFormat::custom("x").code(), Some("x"));
    }

    #[test]
    fn builtin_codes() {
        assert_eq!(builtin_format_code(0), Some("General"));
        assert_eq!(builtin_format_code(49), Some("@"));
        assert_eq!(builtin_format_code(5), None);
        let fill = Fill::solid(Color::Theme(1));
        assert_eq!(fill.pattern, PatternType::Solid);
        assert_eq!(NumberFormat::custom("x"), NumberFormat::Custom("x".into()));
    }
}
