//! Conditional formatting: rules that format cells depending on their
//! values (ECMA-376 Part 1 §18.3.1.18). Formatting rules refer to a
//! differential format (`dxf`) in the stylesheet; color scales, data bars
//! and icon sets are drawn by Excel.
//!
//! ```
//! use openxml_xlsx::{Workbook, ConditionalFormat, CfOperator, CellStyle, Color};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! let red = CellStyle::new().font_color(Color::Rgb(0x9C, 0, 6)).fill_color(Color::Rgb(0xFF, 0xC7, 0xCE));
//! sheet.add_conditional_format("B2:B20", &ConditionalFormat::cell_is(CfOperator::LessThan, "0").style(red))?;
//! sheet.add_conditional_format("C2:C20", &ConditionalFormat::color_scale_3(
//!     Color::Rgb(0xF8, 0x69, 0x6B), Color::Rgb(0xFF, 0xEB, 0x84), Color::Rgb(0x63, 0xBE, 0x7B)))?;
//! assert_eq!(sheet.as_view().conditional_formats().len(), 2);
//! # Ok::<(), openxml_core::Error>(())
//! ```

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{CellRange, ToCellRange, ToRanges};
use crate::styles::{CellStyle, Color};
use crate::util::{formula_string, from_sqref, to_sqref};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use sml::{
    ST_ConditionalFormattingOperator as CfOperator, ST_IconSetType as IconSetType,
    ST_TimePeriod as TimePeriod,
};

/// A threshold of a color scale, data bar or icon set.
#[derive(Clone, Debug, PartialEq)]
pub enum CfValue {
    /// The lowest value of the range.
    Min,
    /// The highest value of the range.
    Max,
    /// A number.
    Number(f64),
    /// A percentage of the value range (0–100).
    Percent(f64),
    /// A percentile (0–100).
    Percentile(f64),
    /// The result of a formula.
    Formula(String),
}

impl CfValue {
    fn to_ct(&self) -> sml::CT_Cfvo {
        use sml::ST_CfvoType as T;
        let num = |n: f64| Some(crate::value::format_number(n));
        let (t, val) = match self {
            CfValue::Min => (T::Min, None),
            CfValue::Max => (T::Max, None),
            CfValue::Number(n) => (T::Num, num(*n)),
            CfValue::Percent(n) => (T::Percent, num(*n)),
            CfValue::Percentile(n) => (T::Percentile, num(*n)),
            CfValue::Formula(f) => (T::Formula, Some(f.trim_start_matches('=').to_owned())),
        };
        sml::CT_Cfvo {
            type_: Some(t),
            val,
            ..Default::default()
        }
    }

    fn from_ct(c: &sml::CT_Cfvo) -> Self {
        use sml::ST_CfvoType as T;
        let val = c.val.clone().unwrap_or_default();
        let num = || val.trim().parse::<f64>();
        match c.type_.unwrap_or(T::Num) {
            T::Min => CfValue::Min,
            T::Max => CfValue::Max,
            T::Num => num().map_or(CfValue::Formula(val.clone()), CfValue::Number),
            T::Percent => num().map_or(CfValue::Formula(val.clone()), CfValue::Percent),
            T::Percentile => num().map_or(CfValue::Formula(val.clone()), CfValue::Percentile),
            T::Formula => CfValue::Formula(val.clone()),
        }
    }
}

/// What a conditional format tests.
#[derive(Clone, Debug, PartialEq)]
pub enum CfRule {
    /// The cell value compared with one or two formulas (`"0"`, `"$B$1"`, `"\"text\""`).
    CellIs {
        /// The comparison.
        operator: CfOperator,
        /// Operands (two for `Between`/`NotBetween`).
        formulas: Vec<String>,
    },
    /// A formula that is true for the cells to format (relative to the
    /// top-left cell of the first range).
    Expression(String),
    /// A two- or three-color gradient.
    ColorScale(Vec<(CfValue, Color)>),
    /// A data bar.
    DataBar {
        /// Value of the shortest bar.
        min: CfValue,
        /// Value of the longest bar.
        max: CfValue,
        /// Bar colour.
        color: Color,
        /// Whether the cell value is shown next to the bar.
        show_value: bool,
    },
    /// An icon set.
    IconSet {
        /// Icons.
        icons: IconSetType,
        /// Lower bound of each icon (one per icon, the first is ignored by Excel).
        thresholds: Vec<CfValue>,
        /// Reverses the icon order.
        reverse: bool,
        /// Whether the cell value is shown next to the icon.
        show_value: bool,
    },
    /// The top (or bottom) N values or percent.
    Top {
        /// N.
        rank: u32,
        /// N is a percentage.
        percent: bool,
        /// Bottom instead of top.
        bottom: bool,
    },
    /// Values above (or below) the average.
    AboveAverage {
        /// Below instead of above.
        below: bool,
        /// Include values equal to the average.
        equal: bool,
        /// Number of standard deviations from the average.
        std_dev: Option<i32>,
    },
    /// Values that occur more than once.
    DuplicateValues,
    /// Values that occur once.
    UniqueValues,
    /// Text containing a string.
    ContainsText(String),
    /// Text not containing a string.
    NotContainsText(String),
    /// Text beginning with a string.
    BeginsWith(String),
    /// Text ending with a string.
    EndsWith(String),
    /// Empty cells.
    ContainsBlanks,
    /// Non-empty cells.
    NotContainsBlanks,
    /// Cells with errors.
    ContainsErrors,
    /// Cells without errors.
    NotContainsErrors,
    /// Dates in a period relative to today.
    TimePeriod(TimePeriod),
}

/// A conditional format: a rule and the formatting it applies.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionalFormat {
    /// The rule.
    pub rule: CfRule,
    /// Formatting applied to matching cells (formatting rules only).
    pub style: Option<CellStyle>,
    /// Lower-priority rules are not evaluated for cells this rule matches.
    pub stop_if_true: bool,
    /// Evaluation order (1 = first). Assigned after the existing rules when `None`.
    pub priority: Option<i32>,
}

fn icon_count(icons: IconSetType) -> usize {
    let name = format!("{icons:?}");
    match name.as_bytes().get(1) {
        Some(b'4') => 4,
        Some(b'5') => 5,
        _ => 3,
    }
}

/// Excel's default thresholds of an icon set: equal percentage bands.
fn default_thresholds(icons: IconSetType) -> Vec<CfValue> {
    let n = icon_count(icons);
    (0..n)
        .map(|i| CfValue::Percent(((i * 100) as f64 / n as f64).round()))
        .collect()
}

impl ConditionalFormat {
    /// A conditional format with a rule.
    pub fn new(rule: CfRule) -> Self {
        ConditionalFormat {
            rule,
            style: None,
            stop_if_true: false,
            priority: None,
        }
    }

    /// Cell value compared with a formula.
    pub fn cell_is(operator: CfOperator, formula: impl Into<String>) -> Self {
        Self::new(CfRule::CellIs {
            operator,
            formulas: vec![formula.into()],
        })
    }

    /// Cell value between two formulas (inclusive).
    pub fn between(low: impl Into<String>, high: impl Into<String>) -> Self {
        Self::new(CfRule::CellIs {
            operator: CfOperator::Between,
            formulas: vec![low.into(), high.into()],
        })
    }

    /// A formula rule.
    pub fn expression(formula: impl Into<String>) -> Self {
        Self::new(CfRule::Expression(formula.into()))
    }

    /// A two-color scale from the lowest to the highest value.
    pub fn color_scale_2(min: Color, max: Color) -> Self {
        Self::new(CfRule::ColorScale(vec![(CfValue::Min, min), (CfValue::Max, max)]))
    }

    /// A three-color scale with the middle color at the 50th percentile.
    pub fn color_scale_3(min: Color, mid: Color, max: Color) -> Self {
        Self::new(CfRule::ColorScale(vec![
            (CfValue::Min, min),
            (CfValue::Percentile(50.0), mid),
            (CfValue::Max, max),
        ]))
    }

    /// A data bar from the lowest to the highest value.
    pub fn data_bar(color: Color) -> Self {
        Self::new(CfRule::DataBar {
            min: CfValue::Min,
            max: CfValue::Max,
            color,
            show_value: true,
        })
    }

    /// An icon set with equal percentage bands.
    pub fn icon_set(icons: IconSetType) -> Self {
        Self::new(CfRule::IconSet {
            icons,
            thresholds: default_thresholds(icons),
            reverse: false,
            show_value: true,
        })
    }

    /// The `n` highest values.
    pub fn top(n: u32) -> Self {
        Self::new(CfRule::Top {
            rank: n,
            percent: false,
            bottom: false,
        })
    }

    /// The `n` lowest values.
    pub fn bottom(n: u32) -> Self {
        Self::new(CfRule::Top {
            rank: n,
            percent: false,
            bottom: true,
        })
    }

    /// The highest `percent` percent of the values.
    pub fn top_percent(percent: u32) -> Self {
        Self::new(CfRule::Top {
            rank: percent,
            percent: true,
            bottom: false,
        })
    }

    /// Values above the average.
    pub fn above_average() -> Self {
        Self::new(CfRule::AboveAverage {
            below: false,
            equal: false,
            std_dev: None,
        })
    }

    /// Values below the average.
    pub fn below_average() -> Self {
        Self::new(CfRule::AboveAverage {
            below: true,
            equal: false,
            std_dev: None,
        })
    }

    /// Duplicate values.
    pub fn duplicates() -> Self {
        Self::new(CfRule::DuplicateValues)
    }

    /// Unique values.
    pub fn unique() -> Self {
        Self::new(CfRule::UniqueValues)
    }

    /// Text containing `text` (case-insensitive).
    pub fn contains_text(text: impl Into<String>) -> Self {
        Self::new(CfRule::ContainsText(text.into()))
    }

    /// Text beginning with `text`.
    pub fn begins_with(text: impl Into<String>) -> Self {
        Self::new(CfRule::BeginsWith(text.into()))
    }

    /// Text ending with `text`.
    pub fn ends_with(text: impl Into<String>) -> Self {
        Self::new(CfRule::EndsWith(text.into()))
    }

    /// Dates within a period relative to today.
    pub fn time_period(period: TimePeriod) -> Self {
        Self::new(CfRule::TimePeriod(period))
    }

    /// The formatting applied to matching cells.
    pub fn style(mut self, style: CellStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Stops evaluating lower-priority rules for matching cells.
    pub fn stop_if_true(mut self) -> Self {
        self.stop_if_true = true;
        self
    }

    /// Sets the priority explicitly (1 = evaluated first).
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority);
        self
    }

    fn check(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::InvalidArgument(m.to_owned()));
        match &self.rule {
            CfRule::CellIs { operator, formulas } => {
                let two = matches!(operator, CfOperator::Between | CfOperator::NotBetween);
                if formulas.len() != if two { 2 } else { 1 } {
                    return bad("wrong number of operands for the comparison");
                }
                if matches!(
                    operator,
                    CfOperator::ContainsText
                        | CfOperator::NotContains
                        | CfOperator::BeginsWith
                        | CfOperator::EndsWith
                ) {
                    return bad("text operators belong to the text rules");
                }
            }
            CfRule::ColorScale(stops) if !(2..=3).contains(&stops.len()) => {
                return bad("a color scale has two or three stops");
            }
            CfRule::IconSet {
                icons, thresholds, ..
            } if thresholds.len() != icon_count(*icons) => {
                return bad("an icon set needs one threshold per icon");
            }
            CfRule::Top { rank, percent, .. } if *rank == 0 || (*percent && *rank > 100) => {
                return bad("invalid rank");
            }
            _ => {}
        }
        Ok(())
    }

    /// The rule element; `cell` is the top-left cell text formulas refer to.
    fn to_ct(&self, cell: &str, dxf_id: Option<u32>, priority: i32) -> sml::CT_CfRule {
        use sml::ST_CfType as T;
        let mut r = sml::CT_CfRule {
            dxf_id,
            priority: Some(priority),
            stop_if_true: self.stop_if_true.then_some(true),
            ..Default::default()
        };
        let strip = |f: &str| f.trim_start_matches('=').to_owned();
        let text_rule = |r: &mut sml::CT_CfRule, t: T, op: CfOperator, text: &str, formula: String| {
            r.type_ = Some(t);
            r.operator = Some(op);
            r.text = Some(text.to_owned());
            r.formula = vec![formula];
        };
        match &self.rule {
            CfRule::CellIs { operator, formulas } => {
                r.type_ = Some(T::CellIs);
                r.operator = Some(*operator);
                r.formula = formulas.iter().map(|f| strip(f)).collect();
            }
            CfRule::Expression(f) => {
                r.type_ = Some(T::Expression);
                r.formula = vec![strip(f)];
            }
            CfRule::ColorScale(stops) => {
                r.type_ = Some(T::ColorScale);
                r.color_scale = Some(Box::new(sml::CT_ColorScale {
                    cfvo: stops.iter().map(|(v, _)| v.to_ct()).collect(),
                    color: stops.iter().map(|(_, c)| c.to_ct()).collect(),
                    ..Default::default()
                }));
            }
            CfRule::DataBar {
                min,
                max,
                color,
                show_value,
            } => {
                r.type_ = Some(T::DataBar);
                r.data_bar = Some(Box::new(sml::CT_DataBar {
                    show_value: (!show_value).then_some(false),
                    cfvo: vec![min.to_ct(), max.to_ct()],
                    color: Some(Box::new(color.to_ct())),
                    ..Default::default()
                }));
            }
            CfRule::IconSet {
                icons,
                thresholds,
                reverse,
                show_value,
            } => {
                r.type_ = Some(T::IconSet);
                r.icon_set = Some(Box::new(sml::CT_IconSet {
                    icon_set: (*icons != IconSetType::V3TrafficLights1).then_some(*icons),
                    show_value: (!show_value).then_some(false),
                    reverse: reverse.then_some(true),
                    cfvo: thresholds.iter().map(CfValue::to_ct).collect(),
                    ..Default::default()
                }));
            }
            CfRule::Top {
                rank,
                percent,
                bottom,
            } => {
                r.type_ = Some(T::Top10);
                r.rank = Some(*rank);
                r.percent = percent.then_some(true);
                r.bottom = bottom.then_some(true);
            }
            CfRule::AboveAverage {
                below,
                equal,
                std_dev,
            } => {
                r.type_ = Some(T::AboveAverage);
                r.above_average = below.then_some(false);
                r.equal_average = equal.then_some(true);
                r.std_dev = *std_dev;
            }
            CfRule::DuplicateValues => r.type_ = Some(T::DuplicateValues),
            CfRule::UniqueValues => r.type_ = Some(T::UniqueValues),
            CfRule::ContainsText(t) => text_rule(
                &mut r,
                T::ContainsText,
                CfOperator::ContainsText,
                t,
                format!("NOT(ISERROR(SEARCH({},{cell})))", formula_string(t)),
            ),
            CfRule::NotContainsText(t) => text_rule(
                &mut r,
                T::NotContainsText,
                CfOperator::NotContains,
                t,
                format!("ISERROR(SEARCH({},{cell}))", formula_string(t)),
            ),
            CfRule::BeginsWith(t) => {
                let s = formula_string(t);
                text_rule(
                    &mut r,
                    T::BeginsWith,
                    CfOperator::BeginsWith,
                    t,
                    format!("LEFT({cell},LEN({s}))={s}"),
                )
            }
            CfRule::EndsWith(t) => {
                let s = formula_string(t);
                text_rule(
                    &mut r,
                    T::EndsWith,
                    CfOperator::EndsWith,
                    t,
                    format!("RIGHT({cell},LEN({s}))={s}"),
                )
            }
            CfRule::ContainsBlanks => {
                r.type_ = Some(T::ContainsBlanks);
                r.formula = vec![format!("LEN(TRIM({cell}))=0")];
            }
            CfRule::NotContainsBlanks => {
                r.type_ = Some(T::NotContainsBlanks);
                r.formula = vec![format!("LEN(TRIM({cell}))>0")];
            }
            CfRule::ContainsErrors => {
                r.type_ = Some(T::ContainsErrors);
                r.formula = vec![format!("ISERROR({cell})")];
            }
            CfRule::NotContainsErrors => {
                r.type_ = Some(T::NotContainsErrors);
                r.formula = vec![format!("NOT(ISERROR({cell}))")];
            }
            CfRule::TimePeriod(p) => {
                r.type_ = Some(T::TimePeriod);
                r.time_period = Some(*p);
                r.formula = vec![time_period_formula(*p, cell)];
            }
        }
        r
    }

    fn from_ct(r: &sml::CT_CfRule, style: Option<CellStyle>) -> Option<Self> {
        use sml::ST_CfType as T;
        let text = || r.text.clone().unwrap_or_default();
        let rule = match r.type_? {
            T::CellIs => CfRule::CellIs {
                operator: r.operator.unwrap_or(CfOperator::Equal),
                formulas: r.formula.clone(),
            },
            T::Expression => CfRule::Expression(r.formula.first().cloned().unwrap_or_default()),
            T::ColorScale => {
                let cs = r.color_scale.as_deref()?;
                CfRule::ColorScale(
                    cs.cfvo
                        .iter()
                        .zip(&cs.color)
                        .map(|(v, c)| {
                            (
                                CfValue::from_ct(v),
                                Color::from_ct(c).unwrap_or(Color::Rgb(0, 0, 0)),
                            )
                        })
                        .collect(),
                )
            }
            T::DataBar => {
                let db = r.data_bar.as_deref()?;
                CfRule::DataBar {
                    min: db.cfvo.first().map_or(CfValue::Min, CfValue::from_ct),
                    max: db.cfvo.get(1).map_or(CfValue::Max, CfValue::from_ct),
                    color: db
                        .color
                        .as_deref()
                        .and_then(Color::from_ct)
                        .unwrap_or(Color::Rgb(0x63, 0x8E, 0xC6)),
                    show_value: db.show_value.unwrap_or(true),
                }
            }
            T::IconSet => {
                let is = r.icon_set.as_deref()?;
                CfRule::IconSet {
                    icons: is.icon_set.unwrap_or(IconSetType::V3TrafficLights1),
                    thresholds: is.cfvo.iter().map(CfValue::from_ct).collect(),
                    reverse: is.reverse.unwrap_or(false),
                    show_value: is.show_value.unwrap_or(true),
                }
            }
            T::Top10 => CfRule::Top {
                rank: r.rank.unwrap_or(10),
                percent: r.percent.unwrap_or(false),
                bottom: r.bottom.unwrap_or(false),
            },
            T::AboveAverage => CfRule::AboveAverage {
                below: r.above_average == Some(false),
                equal: r.equal_average.unwrap_or(false),
                std_dev: r.std_dev,
            },
            T::DuplicateValues => CfRule::DuplicateValues,
            T::UniqueValues => CfRule::UniqueValues,
            T::ContainsText => CfRule::ContainsText(text()),
            T::NotContainsText => CfRule::NotContainsText(text()),
            T::BeginsWith => CfRule::BeginsWith(text()),
            T::EndsWith => CfRule::EndsWith(text()),
            T::ContainsBlanks => CfRule::ContainsBlanks,
            T::NotContainsBlanks => CfRule::NotContainsBlanks,
            T::ContainsErrors => CfRule::ContainsErrors,
            T::NotContainsErrors => CfRule::NotContainsErrors,
            T::TimePeriod => CfRule::TimePeriod(r.time_period.unwrap_or(TimePeriod::Today)),
        };
        Some(ConditionalFormat {
            rule,
            style,
            stop_if_true: r.stop_if_true.unwrap_or(false),
            priority: r.priority,
        })
    }
}

/// The formula Excel writes for a time-period rule.
fn time_period_formula(p: TimePeriod, c: &str) -> String {
    match p {
        TimePeriod::Today => format!("FLOOR({c},1)=TODAY()"),
        TimePeriod::Yesterday => format!("FLOOR({c},1)=TODAY()-1"),
        TimePeriod::Tomorrow => format!("FLOOR({c},1)=TODAY()+1"),
        TimePeriod::Last7Days => format!("AND(TODAY()-FLOOR({c},1)<=6,FLOOR({c},1)<=TODAY())"),
        TimePeriod::ThisMonth => format!("AND(MONTH({c})=MONTH(TODAY()),YEAR({c})=YEAR(TODAY()))"),
        TimePeriod::LastMonth => {
            format!("AND(MONTH({c})=MONTH(EDATE(TODAY(),0-1)),YEAR({c})=YEAR(EDATE(TODAY(),0-1)))")
        }
        TimePeriod::NextMonth => {
            format!("AND(MONTH({c})=MONTH(EDATE(TODAY(),0+1)),YEAR({c})=YEAR(EDATE(TODAY(),0+1)))")
        }
        TimePeriod::ThisWeek => format!(
            "AND(TODAY()-ROUNDDOWN({c},0)<=WEEKDAY(TODAY())-1,ROUNDDOWN({c},0)-TODAY()<=7-WEEKDAY(TODAY()))"
        ),
        TimePeriod::LastWeek => format!(
            "AND(TODAY()-ROUNDDOWN({c},0)>=(WEEKDAY(TODAY())),TODAY()-ROUNDDOWN({c},0)<(WEEKDAY(TODAY())+7))"
        ),
        TimePeriod::NextWeek => format!(
            "AND(ROUNDDOWN({c},0)-TODAY()>(7-WEEKDAY(TODAY())),ROUNDDOWN({c},0)-TODAY()<(15-WEEKDAY(TODAY())))"
        ),
    }
}

fn max_priority(ws: &sml::CT_Worksheet) -> i32 {
    ws.conditional_formatting
        .iter()
        .flat_map(|c| c.cf_rule.iter())
        .filter_map(|r| r.priority)
        .max()
        .unwrap_or(0)
}

impl Worksheet<'_> {
    /// The conditional formats of the sheet with the ranges they apply to,
    /// in document order.
    pub fn conditional_formats(&self) -> Vec<(Vec<CellRange>, ConditionalFormat)> {
        let mut out = Vec::new();
        for cf in &self.data.conditional_formatting {
            let ranges = cf.sqref.as_ref().map(from_sqref).unwrap_or_default();
            for r in &cf.cf_rule {
                let style = r.dxf_id.and_then(|id| self.ctx.styles.dxf_style(id));
                if let Some(f) = ConditionalFormat::from_ct(r, style) {
                    out.push((ranges.clone(), f));
                }
            }
        }
        out
    }
}

impl WorksheetMut<'_> {
    /// Adds a conditional format to one or more ranges and returns its priority.
    pub fn add_conditional_format(
        &mut self,
        ranges: impl ToRanges,
        format: &ConditionalFormat,
    ) -> Result<i32> {
        let ranges = ranges.to_ranges()?;
        format.check()?;
        let formatting_rule = !matches!(
            format.rule,
            CfRule::ColorScale(_) | CfRule::DataBar { .. } | CfRule::IconSet { .. }
        );
        let dxf_id = match (&format.style, formatting_rule) {
            (Some(style), true) => Some(self.styles.add_dxf(style)),
            (Some(_), false) => {
                return Err(Error::InvalidArgument(
                    "color scales, data bars and icon sets take no cell style".into(),
                ));
            }
            (None, _) => None,
        };
        let priority = format.priority.unwrap_or_else(|| max_priority(self.data) + 1);
        let cell = ranges[0].start().to_string();
        self.data
            .conditional_formatting
            .push(sml::CT_ConditionalFormatting {
                sqref: Some(to_sqref(&ranges)),
                cf_rule: vec![format.to_ct(&cell, dxf_id, priority)],
                ..Default::default()
            });
        Ok(priority)
    }

    /// Removes the conditional formats of every cell in `range`: ranges of
    /// existing formats that overlap it are dropped (not split), and formats
    /// left without ranges are removed. Returns the number of rules removed.
    pub fn remove_conditional_formats(&mut self, range: impl ToCellRange) -> Result<usize> {
        let range = range.to_cell_range()?;
        let mut removed = 0;
        self.data.conditional_formatting.retain_mut(|cf| {
            let kept: Vec<CellRange> = cf
                .sqref
                .as_ref()
                .map(from_sqref)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| !r.intersects(&range))
                .collect();
            cf.sqref = Some(to_sqref(&kept));
            if kept.is_empty() {
                removed += cf.cf_rule.len();
            }
            !kept.is_empty()
        });
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip() {
        let formats = [
            ConditionalFormat::cell_is(CfOperator::GreaterThan, "0"),
            ConditionalFormat::between("10", "30").stop_if_true(),
            ConditionalFormat::expression("MOD(ROW(),2)=0"),
            ConditionalFormat::color_scale_2(Color::Rgb(255, 255, 255), Color::Rgb(0, 128, 0)),
            ConditionalFormat::color_scale_3(Color::Rgb(1, 2, 3), Color::Theme(4), Color::Rgb(4, 5, 6)),
            ConditionalFormat::data_bar(Color::Rgb(0x63, 0x8E, 0xC6)),
            ConditionalFormat::new(CfRule::DataBar {
                min: CfValue::Number(0.0),
                max: CfValue::Formula("$A$1".into()),
                color: Color::Rgb(1, 1, 1),
                show_value: false,
            }),
            ConditionalFormat::icon_set(IconSetType::V3Arrows),
            ConditionalFormat::icon_set(IconSetType::V5Quarters),
            ConditionalFormat::new(CfRule::IconSet {
                icons: IconSetType::V3TrafficLights1,
                thresholds: vec![
                    CfValue::Number(0.0),
                    CfValue::Percentile(40.0),
                    CfValue::Percent(90.0),
                ],
                reverse: true,
                show_value: false,
            }),
            ConditionalFormat::top(5),
            ConditionalFormat::bottom(3),
            ConditionalFormat::top_percent(10),
            ConditionalFormat::above_average(),
            ConditionalFormat::below_average(),
            ConditionalFormat::new(CfRule::AboveAverage {
                below: false,
                equal: true,
                std_dev: Some(2),
            }),
            ConditionalFormat::duplicates(),
            ConditionalFormat::unique(),
            ConditionalFormat::contains_text("err \"x\""),
            ConditionalFormat::new(CfRule::NotContainsText("ok".into())),
            ConditionalFormat::begins_with("A"),
            ConditionalFormat::ends_with("z"),
            ConditionalFormat::new(CfRule::ContainsBlanks),
            ConditionalFormat::new(CfRule::NotContainsBlanks),
            ConditionalFormat::new(CfRule::ContainsErrors),
            ConditionalFormat::new(CfRule::NotContainsErrors),
            ConditionalFormat::time_period(TimePeriod::Last7Days),
            ConditionalFormat::time_period(TimePeriod::NextWeek),
        ];
        for (i, f) in formats.iter().enumerate() {
            assert!(f.check().is_ok(), "{f:?}");
            let ct = f.to_ct("B2", None, i as i32 + 1);
            let xml = sml::elements::WORKSHEET.to_xml(&sml::CT_Worksheet {
                conditional_formatting: vec![sml::CT_ConditionalFormatting {
                    sqref: Some(to_sqref(&[CellRange::parse("B2:B9").unwrap()])),
                    cf_rule: vec![ct],
                    ..Default::default()
                }],
                ..Default::default()
            });
            let back = sml::elements::WORKSHEET.parse(&xml).unwrap();
            let got = ConditionalFormat::from_ct(&back.conditional_formatting[0].cf_rule[0], None).unwrap();
            let mut expected = f.clone();
            expected.priority = Some(i as i32 + 1);
            assert_eq!(got, expected, "{xml}");
        }
        let ct = ConditionalFormat::contains_text("a\"b").to_ct("C3", Some(0), 1);
        assert_eq!(ct.formula, ["NOT(ISERROR(SEARCH(\"a\"\"b\",C3)))"]);
        let ct = ConditionalFormat::begins_with("x").to_ct("C3", None, 1);
        assert_eq!(ct.formula, ["LEFT(C3,LEN(\"x\"))=\"x\""]);
        let ct = ConditionalFormat::time_period(TimePeriod::Today).to_ct("D1", None, 1);
        assert_eq!(ct.formula, ["FLOOR(D1,1)=TODAY()"]);
    }

    #[test]
    fn invalid_rules_are_rejected() {
        assert!(
            ConditionalFormat::cell_is(CfOperator::Between, "1")
                .check()
                .is_err()
        );
        assert!(
            ConditionalFormat::cell_is(CfOperator::BeginsWith, "\"a\"")
                .check()
                .is_err()
        );
        assert!(
            ConditionalFormat::new(CfRule::ColorScale(vec![(CfValue::Min, Color::Theme(1))]))
                .check()
                .is_err()
        );
        assert!(
            ConditionalFormat::new(CfRule::IconSet {
                icons: IconSetType::V4Arrows,
                thresholds: vec![CfValue::Min],
                reverse: false,
                show_value: true
            })
            .check()
            .is_err()
        );
        assert!(ConditionalFormat::top(0).check().is_err());
        assert!(ConditionalFormat::top_percent(101).check().is_err());
        assert_eq!(default_thresholds(IconSetType::V4Rating).len(), 4);
        assert_eq!(
            default_thresholds(IconSetType::V3Flags),
            [
                CfValue::Percent(0.0),
                CfValue::Percent(33.0),
                CfValue::Percent(67.0)
            ]
        );
    }
}
