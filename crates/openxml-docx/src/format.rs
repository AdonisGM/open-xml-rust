//! Paragraph, run and table formatting shared by direct formatting and
//! style definitions: borders, shading, tab stops, line spacing, …

use openxml_core::{FontSize, Length, Result};
use openxml_schema::wml::{self, ST_Border, ST_LineSpacingRule, ST_TabJc, ST_TabTlc};
use openxml_xml::{Ns, RawElement, XmlRead, XmlWrite};

use crate::paragraph::Alignment;
use crate::run::{RunMut, VerticalAlign};
use crate::util::{self, hex_color, hex_color_string, off, on, on_off_value};

/// Line style of a border (`w:val` of a border element).
pub type BorderStyle = ST_Border;

/// A border line.
#[derive(Clone, Debug, PartialEq)]
pub struct Border {
    /// Line style.
    pub style: BorderStyle,
    /// Line width in points (stored in eighths of a point).
    pub width: f64,
    /// Color as `RRGGBB` (or `auto`); `None` leaves it to the application.
    pub color: Option<String>,
    /// Distance between the border and the text, in points.
    pub space: u32,
}

impl Border {
    /// A single solid line of `width` points.
    pub fn single(width: f64, color: &str) -> Self {
        Border {
            style: ST_Border::Single,
            width,
            color: Some(color.to_owned()),
            space: 0,
        }
    }

    /// A line of the given style.
    pub fn new(style: BorderStyle, width: f64, color: &str) -> Self {
        Border {
            style,
            width,
            color: Some(color.to_owned()),
            space: 0,
        }
    }

    /// No border (overrides a border inherited from a style).
    pub fn none() -> Self {
        Border {
            style: ST_Border::Nil,
            width: 0.0,
            color: None,
            space: 0,
        }
    }

    /// Sets the distance between the border and the text (points).
    pub fn with_space(mut self, space: u32) -> Self {
        self.space = space;
        self
    }

    pub(crate) fn to_ct(&self) -> Result<wml::CT_Border> {
        let none = matches!(self.style, ST_Border::Nil | ST_Border::None);
        Ok(wml::CT_Border {
            val: Some(self.style),
            color: self.color.as_deref().map(hex_color).transpose()?,
            sz: (!none).then(|| (self.width * 8.0).round().max(0.0) as u64),
            space: (!none).then_some(u64::from(self.space)),
            ..Default::default()
        })
    }

    pub(crate) fn from_ct(b: &wml::CT_Border) -> Option<Border> {
        Some(Border {
            style: b.val?,
            width: b.sz.unwrap_or(0) as f64 / 8.0,
            color: b.color.as_ref().map(hex_color_string),
            space: b.space.unwrap_or(0) as u32,
        })
    }
}

fn ct_border(b: &Option<Border>) -> Result<Option<Box<wml::CT_Border>>> {
    b.as_ref().map(|b| b.to_ct().map(Box::new)).transpose()
}

fn read_border(b: &Option<Box<wml::CT_Border>>) -> Option<Border> {
    b.as_deref().and_then(Border::from_ct)
}

/// Borders of a paragraph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphBorders {
    /// Top border.
    pub top: Option<Border>,
    /// Left border.
    pub left: Option<Border>,
    /// Bottom border.
    pub bottom: Option<Border>,
    /// Right border.
    pub right: Option<Border>,
    /// Border between consecutive paragraphs with the same borders.
    pub between: Option<Border>,
}

impl ParagraphBorders {
    /// The same border on all four sides.
    pub fn around(border: Border) -> Self {
        ParagraphBorders {
            top: Some(border.clone()),
            left: Some(border.clone()),
            bottom: Some(border.clone()),
            right: Some(border),
            between: None,
        }
    }

    pub(crate) fn to_ct(&self) -> Result<wml::CT_PBdr> {
        Ok(wml::CT_PBdr {
            top: ct_border(&self.top)?,
            left: ct_border(&self.left)?,
            bottom: ct_border(&self.bottom)?,
            right: ct_border(&self.right)?,
            between: ct_border(&self.between)?,
            ..Default::default()
        })
    }

    pub(crate) fn from_ct(b: &wml::CT_PBdr) -> Self {
        ParagraphBorders {
            top: read_border(&b.top),
            left: read_border(&b.left),
            bottom: read_border(&b.bottom),
            right: read_border(&b.right),
            between: read_border(&b.between),
        }
    }
}

/// Borders of a table or of a table cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableBorders {
    /// Top edge.
    pub top: Option<Border>,
    /// Left edge.
    pub left: Option<Border>,
    /// Bottom edge.
    pub bottom: Option<Border>,
    /// Right edge.
    pub right: Option<Border>,
    /// Horizontal lines between rows (tables only).
    pub inside_horizontal: Option<Border>,
    /// Vertical lines between columns (tables only).
    pub inside_vertical: Option<Border>,
}

impl TableBorders {
    /// The same border on every edge and inside line.
    pub fn all(border: Border) -> Self {
        TableBorders {
            top: Some(border.clone()),
            left: Some(border.clone()),
            bottom: Some(border.clone()),
            right: Some(border.clone()),
            inside_horizontal: Some(border.clone()),
            inside_vertical: Some(border),
        }
    }

    /// The outer edges only.
    pub fn outside(border: Border) -> Self {
        TableBorders {
            top: Some(border.clone()),
            left: Some(border.clone()),
            bottom: Some(border.clone()),
            right: Some(border),
            inside_horizontal: Some(Border::none()),
            inside_vertical: Some(Border::none()),
        }
    }

    pub(crate) fn to_table(&self) -> Result<wml::CT_TblBorders> {
        Ok(wml::CT_TblBorders {
            top: ct_border(&self.top)?,
            left: ct_border(&self.left)?,
            bottom: ct_border(&self.bottom)?,
            right: ct_border(&self.right)?,
            inside_h: ct_border(&self.inside_horizontal)?,
            inside_v: ct_border(&self.inside_vertical)?,
            ..Default::default()
        })
    }

    pub(crate) fn to_cell(&self) -> Result<wml::CT_TcBorders> {
        Ok(wml::CT_TcBorders {
            top: ct_border(&self.top)?,
            left: ct_border(&self.left)?,
            bottom: ct_border(&self.bottom)?,
            right: ct_border(&self.right)?,
            inside_h: ct_border(&self.inside_horizontal)?,
            inside_v: ct_border(&self.inside_vertical)?,
            ..Default::default()
        })
    }

    pub(crate) fn from_table(b: &wml::CT_TblBorders) -> Self {
        TableBorders {
            top: read_border(&b.top),
            left: read_border(&b.left).or_else(|| read_border(&b.start)),
            bottom: read_border(&b.bottom),
            right: read_border(&b.right).or_else(|| read_border(&b.end)),
            inside_horizontal: read_border(&b.inside_h),
            inside_vertical: read_border(&b.inside_v),
        }
    }
}

/// Line spacing of a paragraph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineSpacing {
    /// Single spacing.
    Single,
    /// 1.5 lines.
    OnePointFive,
    /// Double spacing.
    Double,
    /// A multiple of single spacing (e.g. `1.15`).
    Multiple(f64),
    /// Exactly this line height.
    Exactly(Length),
    /// At least this line height.
    AtLeast(Length),
}

impl LineSpacing {
    fn to_attrs(self) -> (wml::ST_SignedTwipsMeasure, ST_LineSpacingRule) {
        let auto = |lines: f64| {
            (
                wml::ST_SignedTwipsMeasure::Integer((lines * 240.0).round() as i64),
                ST_LineSpacingRule::Auto,
            )
        };
        match self {
            LineSpacing::Single => auto(1.0),
            LineSpacing::OnePointFive => auto(1.5),
            LineSpacing::Double => auto(2.0),
            LineSpacing::Multiple(m) => auto(m),
            LineSpacing::Exactly(l) => (util::signed_twips(l), ST_LineSpacingRule::Exact),
            LineSpacing::AtLeast(l) => (util::signed_twips(l), ST_LineSpacingRule::AtLeast),
        }
    }

    pub(crate) fn from_spacing(s: &wml::CT_Spacing) -> Option<LineSpacing> {
        let line = s.line.as_ref().and_then(util::signed_twips_value)?;
        Some(match s.line_rule.unwrap_or(ST_LineSpacingRule::Auto) {
            ST_LineSpacingRule::Auto => match line.as_twips() {
                240 => LineSpacing::Single,
                360 => LineSpacing::OnePointFive,
                480 => LineSpacing::Double,
                n => LineSpacing::Multiple(n as f64 / 240.0),
            },
            ST_LineSpacingRule::Exact => LineSpacing::Exactly(line),
            ST_LineSpacingRule::AtLeast => LineSpacing::AtLeast(line),
        })
    }
}

/// Alignment of text at a tab stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAlignment {
    /// Text starts at the stop.
    Left,
    /// Text is centered on the stop.
    Center,
    /// Text ends at the stop.
    Right,
    /// Numbers are aligned on their decimal point.
    Decimal,
    /// A vertical bar is drawn at the stop.
    Bar,
}

/// Characters filling the space before a tab stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabLeader {
    /// Blank.
    None,
    /// Dots (`.....`).
    Dot,
    /// Hyphens.
    Hyphen,
    /// Underline.
    Underscore,
    /// Heavy line.
    Heavy,
    /// Middle dots.
    MiddleDot,
}

/// A custom tab stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabStop {
    /// Position from the leading edge of the text area.
    pub position: Length,
    /// Alignment.
    pub alignment: TabAlignment,
    /// Leader.
    pub leader: TabLeader,
}

impl TabStop {
    /// A tab stop without leader.
    pub fn new(position: Length, alignment: TabAlignment) -> Self {
        TabStop {
            position,
            alignment,
            leader: TabLeader::None,
        }
    }

    /// Sets the leader.
    pub fn with_leader(mut self, leader: TabLeader) -> Self {
        self.leader = leader;
        self
    }

    fn to_ct(self) -> wml::CT_TabStop {
        wml::CT_TabStop {
            val: Some(match self.alignment {
                TabAlignment::Left => ST_TabJc::Left,
                TabAlignment::Center => ST_TabJc::Center,
                TabAlignment::Right => ST_TabJc::Right,
                TabAlignment::Decimal => ST_TabJc::Decimal,
                TabAlignment::Bar => ST_TabJc::Bar,
            }),
            leader: match self.leader {
                TabLeader::None => None,
                TabLeader::Dot => Some(ST_TabTlc::Dot),
                TabLeader::Hyphen => Some(ST_TabTlc::Hyphen),
                TabLeader::Underscore => Some(ST_TabTlc::Underscore),
                TabLeader::Heavy => Some(ST_TabTlc::Heavy),
                TabLeader::MiddleDot => Some(ST_TabTlc::MiddleDot),
            },
            pos: Some(util::signed_twips(self.position)),
            ..Default::default()
        }
    }

    pub(crate) fn from_ct(t: &wml::CT_TabStop) -> Option<TabStop> {
        let alignment = match t.val? {
            ST_TabJc::Left | ST_TabJc::Start => TabAlignment::Left,
            ST_TabJc::Center => TabAlignment::Center,
            ST_TabJc::Right | ST_TabJc::End => TabAlignment::Right,
            ST_TabJc::Decimal => TabAlignment::Decimal,
            ST_TabJc::Bar => TabAlignment::Bar,
            _ => return None,
        };
        let leader = match t.leader {
            None | Some(ST_TabTlc::None) => TabLeader::None,
            Some(ST_TabTlc::Dot) => TabLeader::Dot,
            Some(ST_TabTlc::Hyphen) => TabLeader::Hyphen,
            Some(ST_TabTlc::Underscore) => TabLeader::Underscore,
            Some(ST_TabTlc::Heavy) => TabLeader::Heavy,
            Some(ST_TabTlc::MiddleDot) => TabLeader::MiddleDot,
        };
        Some(TabStop {
            position: t.pos.as_ref().and_then(util::signed_twips_value)?,
            alignment,
            leader,
        })
    }
}

/// Paragraph formatting. Fields left at `None` (or empty) are not changed.
///
/// Used for direct formatting ([`crate::ParagraphMut::set_format`]) and in
/// style definitions ([`crate::StyleDefinition`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphFormat {
    /// Horizontal alignment.
    pub alignment: Option<Alignment>,
    /// Space before the paragraph.
    pub space_before: Option<Length>,
    /// Space after the paragraph.
    pub space_after: Option<Length>,
    /// Line spacing.
    pub line_spacing: Option<LineSpacing>,
    /// Left indentation.
    pub indent_left: Option<Length>,
    /// Right indentation.
    pub indent_right: Option<Length>,
    /// First-line indentation; negative for a hanging indent.
    pub first_line_indent: Option<Length>,
    /// Keep on the same page as the next paragraph.
    pub keep_with_next: Option<bool>,
    /// Keep all lines of the paragraph on one page.
    pub keep_together: Option<bool>,
    /// Start on a new page.
    pub page_break_before: Option<bool>,
    /// Prevent widow and orphan lines.
    pub widow_control: Option<bool>,
    /// Outline level (0–8) used by the table of contents.
    pub outline_level: Option<u8>,
    /// Borders.
    pub borders: Option<ParagraphBorders>,
    /// Background color (`RRGGBB`).
    pub shading: Option<String>,
    /// Custom tab stops (replace existing ones when not empty).
    pub tab_stops: Vec<TabStop>,
}

fn flag(v: bool) -> Box<wml::CT_OnOff> {
    if v { on() } else { off() }
}

/// Background shading with a solid fill.
pub(crate) fn solid_shading(hex: &str) -> Result<wml::CT_Shd> {
    Ok(wml::CT_Shd {
        val: Some(wml::ST_Shd::Clear),
        color: Some(hex_color("auto")?),
        fill: Some(hex_color(hex)?),
        ..Default::default()
    })
}

/// Sets the line spacing of paragraph properties.
pub(crate) fn set_line_spacing(ppr: &mut wml::CT_PPr, spacing: LineSpacing) {
    let (line, rule) = spacing.to_attrs();
    let s = ppr.spacing.get_or_insert_with(Default::default);
    s.line = Some(line);
    s.line_rule = Some(rule);
}

/// Adds or replaces a tab stop at the same position, keeping stops sorted.
pub(crate) fn add_tab_stop(ppr: &mut wml::CT_PPr, stop: TabStop) {
    let tabs = ppr.tabs.get_or_insert_with(Default::default);
    let ct = stop.to_ct();
    tabs.tab.retain(|t| t.pos != ct.pos);
    tabs.tab.push(ct);
    tabs.tab.sort_by_key(|t| {
        t.pos
            .as_ref()
            .and_then(util::signed_twips_value)
            .unwrap_or_default()
    });
}

/// Applies paragraph formatting to paragraph properties.
pub(crate) fn apply_paragraph_format(ppr: &mut wml::CT_PPr, f: &ParagraphFormat) -> Result<()> {
    if let Some(a) = f.alignment {
        ppr.jc = Some(Box::new(wml::CT_Jc {
            val: Some(a.to_jc()),
            ..Default::default()
        }));
    }
    if f.space_before.is_some() || f.space_after.is_some() {
        let s = ppr.spacing.get_or_insert_with(Default::default);
        if let Some(v) = f.space_before {
            s.before = Some(util::twips(v));
        }
        if let Some(v) = f.space_after {
            s.after = Some(util::twips(v));
        }
    }
    if let Some(ls) = f.line_spacing {
        set_line_spacing(ppr, ls);
    }
    if f.indent_left.is_some() || f.indent_right.is_some() || f.first_line_indent.is_some() {
        let ind = ppr.ind.get_or_insert_with(Default::default);
        if let Some(v) = f.indent_left {
            ind.left = Some(util::signed_twips(v));
        }
        if let Some(v) = f.indent_right {
            ind.right = Some(util::signed_twips(v));
        }
        if let Some(v) = f.first_line_indent {
            if v.as_emu() >= 0 {
                ind.first_line = Some(util::twips(v));
                ind.hanging = None;
            } else {
                ind.hanging = Some(util::twips(-v));
                ind.first_line = None;
            }
        }
    }
    if let Some(v) = f.keep_with_next {
        ppr.keep_next = Some(flag(v));
    }
    if let Some(v) = f.keep_together {
        ppr.keep_lines = Some(flag(v));
    }
    if let Some(v) = f.page_break_before {
        ppr.page_break_before = Some(flag(v));
    }
    if let Some(v) = f.widow_control {
        ppr.widow_control = Some(flag(v));
    }
    if let Some(level) = f.outline_level {
        ppr.outline_lvl = Some(util::decimal(i64::from(level.min(9))));
    }
    if let Some(b) = &f.borders {
        ppr.p_bdr = Some(Box::new(b.to_ct()?));
    }
    if let Some(hex) = &f.shading {
        ppr.shd = Some(Box::new(solid_shading(hex)?));
    }
    if !f.tab_stops.is_empty() {
        ppr.tabs = None;
        for t in &f.tab_stops {
            add_tab_stop(ppr, *t);
        }
    }
    Ok(())
}

/// Converts between schema types with the same content model by
/// serializing one and parsing it as the other.
pub(crate) fn convert<A: XmlWrite, B: XmlRead>(value: &A, local: &str) -> Result<B> {
    RawElement::from_typed(value, Ns::W, local)
        .to_typed()
        .map_err(|source| openxml_core::Error::Xml {
            part: format!("w:{local}"),
            source,
        })
}

/// Character formatting. Fields left at `None` are not changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunFormat {
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underline style.
    pub underline: Option<crate::UnderlineStyle>,
    /// Single strikethrough.
    pub strike: Option<bool>,
    /// Small capitals.
    pub small_caps: Option<bool>,
    /// Font size.
    pub size: Option<FontSize>,
    /// Text color (`RRGGBB`).
    pub color: Option<String>,
    /// Font name.
    pub font: Option<String>,
    /// Highlight color.
    pub highlight: Option<crate::HighlightColor>,
    /// Superscript/subscript.
    pub vertical_align: Option<VerticalAlign>,
}

/// Applies character formatting to a run.
pub(crate) fn apply_run_format(r: &mut wml::CT_R, f: &RunFormat) -> Result<()> {
    let mut run = RunMut::new(r);
    if let Some(v) = f.bold {
        run.bold(v);
    }
    if let Some(v) = f.italic {
        run.italic(v);
    }
    if let Some(v) = f.underline {
        run.underline(Some(v));
    }
    if let Some(v) = f.strike {
        run.strike(v);
    }
    if let Some(v) = f.small_caps {
        run.small_caps(v);
    }
    if let Some(v) = f.size {
        run.size(v);
    }
    if let Some(v) = &f.color {
        run.color(v)?;
    }
    if let Some(v) = &f.font {
        run.font(v);
    }
    if let Some(v) = f.highlight {
        run.highlight(Some(v));
    }
    if let Some(v) = f.vertical_align {
        run.vertical_align(v);
    }
    Ok(())
}

/// Applies character formatting to run properties (as used by styles).
pub(crate) fn apply_run_properties(rpr: &mut Option<Box<wml::CT_RPr>>, f: &RunFormat) -> Result<()> {
    let mut r = wml::CT_R {
        r_pr: rpr.take(),
        ..Default::default()
    };
    // Explicit "off" values are always written for styles.
    r.r_pr.get_or_insert_with(Default::default);
    apply_run_format(&mut r, f)?;
    *rpr = r
        .r_pr
        .filter(|p| !p.r_pr_base.is_empty() || p.r_pr_change.is_some());
    Ok(())
}

/// Horizontal alignment of a table between the margins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlignment {
    /// Left.
    Left,
    /// Centered.
    Center,
    /// Right.
    Right,
}

/// Column-width algorithm of a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableLayout {
    /// Columns keep their widths.
    Fixed,
    /// Columns are resized to fit their content.
    Autofit,
}

/// Preferred width of a table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableWidth {
    /// Determined by the content.
    Auto,
    /// A fixed width.
    Fixed(Length),
    /// A percentage of the text width.
    Percent(f64),
}

/// Default margins between the cell borders and the cell content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellMargins {
    /// Top margin.
    pub top: Option<Length>,
    /// Left margin.
    pub left: Option<Length>,
    /// Bottom margin.
    pub bottom: Option<Length>,
    /// Right margin.
    pub right: Option<Length>,
}

impl CellMargins {
    /// The same margin on every side.
    pub fn all(margin: Length) -> Self {
        CellMargins {
            top: Some(margin),
            left: Some(margin),
            bottom: Some(margin),
            right: Some(margin),
        }
    }
}

/// How a row height is interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightRule {
    /// The row is at least this high.
    AtLeast,
    /// The row is exactly this high.
    Exact,
}

/// Table-level formatting. Fields left at `None` are not changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableFormat {
    /// Borders.
    pub borders: Option<TableBorders>,
    /// Default cell margins.
    pub cell_margins: Option<CellMargins>,
    /// Alignment between the margins.
    pub alignment: Option<TableAlignment>,
    /// Indentation from the leading margin.
    pub indent: Option<Length>,
    /// Layout algorithm.
    pub layout: Option<TableLayout>,
    /// Preferred width.
    pub width: Option<TableWidth>,
    /// Background color (`RRGGBB`).
    pub shading: Option<String>,
}

/// A width in twentieths of a point.
pub(crate) fn dxa(len: Length) -> Box<wml::CT_TblWidth> {
    Box::new(wml::CT_TblWidth {
        w: Some(wml::ST_MeasurementOrPercent::DecimalNumberOrPercent(
            wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(len.as_twips()),
        )),
        type_: Some(wml::ST_TblWidth::Dxa),
        ..Default::default()
    })
}

fn table_width(w: TableWidth) -> Box<wml::CT_TblWidth> {
    match w {
        TableWidth::Auto => Box::new(wml::CT_TblWidth {
            w: Some(wml::ST_MeasurementOrPercent::DecimalNumberOrPercent(
                wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage(0),
            )),
            type_: Some(wml::ST_TblWidth::Auto),
            ..Default::default()
        }),
        TableWidth::Fixed(l) => dxa(l),
        // Fiftieths of a percent, the unit understood by every consumer.
        TableWidth::Percent(p) => Box::new(wml::CT_TblWidth {
            w: Some(wml::ST_MeasurementOrPercent::DecimalNumberOrPercent(
                wml::ST_DecimalNumberOrPercent::UnqualifiedPercentage((p * 50.0).round() as i64),
            )),
            type_: Some(wml::ST_TblWidth::Pct),
            ..Default::default()
        }),
    }
}

/// Cell margins as `w:tblCellMar`.
pub(crate) fn cell_margins(m: &CellMargins) -> wml::CT_TblCellMar {
    wml::CT_TblCellMar {
        top: m.top.map(dxa),
        left: m.left.map(dxa),
        bottom: m.bottom.map(dxa),
        right: m.right.map(dxa),
        ..Default::default()
    }
}

/// Applies table formatting to table properties.
pub(crate) fn apply_table_format(tpr: &mut wml::CT_TblPr, f: &TableFormat) -> Result<()> {
    if let Some(b) = &f.borders {
        tpr.tbl_borders = Some(Box::new(b.to_table()?));
    }
    if let Some(m) = &f.cell_margins {
        tpr.tbl_cell_mar = Some(Box::new(cell_margins(m)));
    }
    if let Some(a) = f.alignment {
        tpr.jc = Some(Box::new(wml::CT_JcTable {
            val: Some(match a {
                TableAlignment::Left => wml::ST_JcTable::Left,
                TableAlignment::Center => wml::ST_JcTable::Center,
                TableAlignment::Right => wml::ST_JcTable::Right,
            }),
            ..Default::default()
        }));
    }
    if let Some(i) = f.indent {
        tpr.tbl_ind = Some(dxa(i));
    }
    if let Some(l) = f.layout {
        tpr.tbl_layout = Some(Box::new(wml::CT_TblLayoutType {
            type_: Some(match l {
                TableLayout::Fixed => wml::ST_TblLayoutType::Fixed,
                TableLayout::Autofit => wml::ST_TblLayoutType::Autofit,
            }),
            ..Default::default()
        }));
    }
    if let Some(w) = f.width {
        tpr.tbl_w = Some(table_width(w));
    }
    if let Some(hex) = &f.shading {
        tpr.shd = Some(Box::new(solid_shading(hex)?));
    }
    Ok(())
}

/// Whether an optional on/off property is set and on.
pub(crate) fn is_set(v: &Option<Box<wml::CT_OnOff>>) -> Option<bool> {
    v.as_deref().map(on_off_value)
}

impl crate::ParagraphMut<'_> {
    /// Applies paragraph formatting (fields left at `None` are unchanged).
    pub fn set_format(&mut self, format: &ParagraphFormat) -> Result<&mut Self> {
        apply_paragraph_format(self.p_pr(), format)?;
        Ok(self)
    }

    /// Sets the line spacing.
    pub fn set_line_spacing(&mut self, spacing: LineSpacing) -> &mut Self {
        set_line_spacing(self.p_pr(), spacing);
        self
    }

    /// Keeps all lines of the paragraph on the same page.
    pub fn set_keep_together(&mut self, value: bool) -> &mut Self {
        self.p_pr().keep_lines = Some(flag(value));
        self
    }

    /// Sets the paragraph borders.
    pub fn set_borders(&mut self, borders: &ParagraphBorders) -> Result<&mut Self> {
        self.p_pr().p_bdr = Some(Box::new(borders.to_ct()?));
        Ok(self)
    }

    /// Sets the background color (`RRGGBB`).
    pub fn set_shading(&mut self, hex: &str) -> Result<&mut Self> {
        self.p_pr().shd = Some(Box::new(solid_shading(hex)?));
        Ok(self)
    }

    /// Adds a custom tab stop (replacing one at the same position).
    pub fn add_tab_stop(&mut self, stop: TabStop) -> &mut Self {
        add_tab_stop(self.p_pr(), stop);
        self
    }
}

impl crate::Paragraph<'_> {
    fn props(&self) -> Option<&wml::CT_PPr> {
        self.p.p_pr.as_deref()
    }

    /// Line spacing applied directly.
    pub fn line_spacing(&self) -> Option<LineSpacing> {
        LineSpacing::from_spacing(self.props()?.spacing.as_deref()?)
    }

    /// Whether "keep lines together" is applied directly.
    pub fn keep_together(&self) -> bool {
        self.props().and_then(|p| is_set(&p.keep_lines)).unwrap_or(false)
    }

    /// Whether "keep with next" is applied directly.
    pub fn keep_with_next(&self) -> bool {
        self.props().and_then(|p| is_set(&p.keep_next)).unwrap_or(false)
    }

    /// Borders applied directly.
    pub fn borders(&self) -> Option<ParagraphBorders> {
        Some(ParagraphBorders::from_ct(self.props()?.p_bdr.as_deref()?))
    }

    /// Background fill color (`RRGGBB`) applied directly.
    pub fn shading(&self) -> Option<String> {
        self.props()?.shd.as_deref()?.fill.as_ref().map(hex_color_string)
    }

    /// Custom tab stops applied directly.
    pub fn tab_stops(&self) -> Vec<TabStop> {
        self.props()
            .and_then(|p| p.tabs.as_deref())
            .map(|t| t.tab.iter().filter_map(TabStop::from_ct).collect())
            .unwrap_or_default()
    }

    /// Outline level applied directly.
    pub fn outline_level(&self) -> Option<u8> {
        self.props()?
            .outline_lvl
            .as_deref()?
            .val
            .map(|v| v.clamp(0, 9) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_convert_both_ways() {
        let b = Border::single(1.5, "FF0000").with_space(4);
        let ct = b.to_ct().unwrap();
        assert_eq!(ct.sz, Some(12));
        assert_eq!(ct.space, Some(4));
        assert_eq!(Border::from_ct(&ct), Some(b));
        let none = Border::none().to_ct().unwrap();
        assert_eq!(
            (none.val, none.sz, none.space),
            (Some(ST_Border::Nil), None, None)
        );
        assert!(Border::single(1.0, "nope").to_ct().is_err());
        let all = TableBorders::all(Border::single(0.5, "000000"));
        let t = all.to_table().unwrap();
        assert_eq!(TableBorders::from_table(&t), all);
        assert!(all.to_cell().unwrap().inside_v.is_some());
        let out = TableBorders::outside(Border::single(1.0, "000000"));
        assert_eq!(out.inside_horizontal, Some(Border::none()));
        let p = ParagraphBorders::around(Border::single(1.0, "auto"));
        assert_eq!(ParagraphBorders::from_ct(&p.to_ct().unwrap()), p);
    }

    #[test]
    fn line_spacing_variants() {
        for (ls, line, rule) in [
            (LineSpacing::Single, 240, ST_LineSpacingRule::Auto),
            (LineSpacing::OnePointFive, 360, ST_LineSpacingRule::Auto),
            (LineSpacing::Double, 480, ST_LineSpacingRule::Auto),
            (LineSpacing::Multiple(1.15), 276, ST_LineSpacingRule::Auto),
            (
                LineSpacing::Exactly(Length::pt(12.0)),
                240,
                ST_LineSpacingRule::Exact,
            ),
            (
                LineSpacing::AtLeast(Length::pt(18.0)),
                360,
                ST_LineSpacingRule::AtLeast,
            ),
        ] {
            let mut ppr = wml::CT_PPr::default();
            set_line_spacing(&mut ppr, ls);
            let s = ppr.spacing.as_ref().unwrap();
            assert_eq!(s.line, Some(wml::ST_SignedTwipsMeasure::Integer(line)));
            assert_eq!(s.line_rule, Some(rule));
            assert_eq!(LineSpacing::from_spacing(s), Some(ls));
        }
    }

    #[test]
    fn tab_stops_are_sorted_and_replaced() {
        let mut ppr = wml::CT_PPr::default();
        add_tab_stop(
            &mut ppr,
            TabStop::new(Length::inches(3.0), TabAlignment::Right).with_leader(TabLeader::Dot),
        );
        add_tab_stop(&mut ppr, TabStop::new(Length::inches(1.0), TabAlignment::Center));
        add_tab_stop(&mut ppr, TabStop::new(Length::inches(3.0), TabAlignment::Decimal));
        let stops: Vec<_> = ppr
            .tabs
            .as_ref()
            .unwrap()
            .tab
            .iter()
            .filter_map(TabStop::from_ct)
            .collect();
        assert_eq!(
            stops,
            [
                TabStop::new(Length::inches(1.0), TabAlignment::Center),
                TabStop::new(Length::inches(3.0), TabAlignment::Decimal)
            ]
        );
    }

    #[test]
    fn paragraph_format_converts_to_style_properties() {
        let f = ParagraphFormat {
            alignment: Some(Alignment::Center),
            space_before: Some(Length::pt(6.0)),
            first_line_indent: Some(Length::inches(-0.25)),
            keep_together: Some(true),
            outline_level: Some(2),
            shading: Some("EEEEEE".into()),
            ..Default::default()
        };
        let mut ppr = wml::CT_PPr::default();
        apply_paragraph_format(&mut ppr, &f).unwrap();
        assert!(ppr.ind.as_ref().unwrap().hanging.is_some());
        let general: wml::CT_PPrGeneral = convert(&ppr, "pPr").unwrap();
        assert_eq!(general.outline_lvl.as_ref().unwrap().val, Some(2));
        assert_eq!(is_set(&general.keep_lines), Some(true));
        assert!(general.shd.is_some());
    }

    #[test]
    fn run_properties_for_styles() {
        let mut rpr = None;
        apply_run_properties(
            &mut rpr,
            &RunFormat {
                bold: Some(false),
                size: Some(FontSize(9.0)),
                ..Default::default()
            },
        )
        .unwrap();
        let rpr = rpr.unwrap();
        // Bold off is explicit (w:b w:val="0"), size is 18 half-points.
        assert_eq!(rpr.r_pr_base.len(), 4);
        let mut empty = None;
        apply_run_properties(&mut empty, &RunFormat::default()).unwrap();
        assert!(empty.is_none());
    }

    #[test]
    fn table_format_sets_properties() {
        let mut tpr = wml::CT_TblPr::default();
        apply_table_format(
            &mut tpr,
            &TableFormat {
                cell_margins: Some(CellMargins::all(Length::pt(4.0))),
                alignment: Some(TableAlignment::Center),
                indent: Some(Length::inches(0.5)),
                layout: Some(TableLayout::Fixed),
                width: Some(TableWidth::Percent(100.0)),
                shading: Some("FFFF00".into()),
                borders: Some(TableBorders::all(Border::single(1.0, "000000"))),
            },
        )
        .unwrap();
        assert_eq!(tpr.jc.as_ref().unwrap().val, Some(wml::ST_JcTable::Center));
        assert_eq!(tpr.tbl_w.as_ref().unwrap().type_, Some(wml::ST_TblWidth::Pct));
        assert_eq!(tpr.tbl_cell_mar.as_ref().unwrap().top, Some(dxa(Length::pt(4.0))));
        assert_eq!(table_width(TableWidth::Auto).type_, Some(wml::ST_TblWidth::Auto));
    }
}
