//! Data validation: rules that restrict what can be typed into cells, with
//! optional input prompts and error alerts (ECMA-376 Part 1 §18.3.1.32).
//!
//! ```
//! use openxml_xlsx::{Workbook, DataValidation, Comparison, ErrorStyle};
//!
//! let mut wb = Workbook::new();
//! let mut sheet = wb.worksheet_mut("Sheet1")?;
//! sheet.add_data_validation("B2:B100", &DataValidation::list(["Yes", "No", "Maybe"])?)?;
//! sheet.add_data_validation(
//!     "C2:C100",
//!     &DataValidation::whole(Comparison::between(1, 10))
//!         .input_message("Quantity", "A whole number from 1 to 10")
//!         .error_message(ErrorStyle::Stop, "Invalid", "Enter 1–10"),
//! )?;
//! assert_eq!(sheet.as_view().data_validations().len(), 2);
//! # Ok::<(), openxml_core::Error>(())
//! ```

use std::fmt::Display;

use openxml_core::{Error, Result};
use openxml_schema::sml;

use crate::cell_ref::{CellRange, ToCellRange, ToRanges};
use crate::date::DateTime;
use crate::util::{from_sqref, to_sqref};
use crate::worksheet::{Worksheet, WorksheetMut};

pub use sml::ST_DataValidationErrorStyle as ErrorStyle;

/// A comparison with one or two formula operands (`"10"`, `"B1"`, `"DATE(2024,1,1)"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// Between two values (inclusive).
    Between(String, String),
    /// Outside two values.
    NotBetween(String, String),
    /// Equal to.
    Equal(String),
    /// Not equal to.
    NotEqual(String),
    /// Less than.
    LessThan(String),
    /// Less than or equal to.
    LessThanOrEqual(String),
    /// Greater than.
    GreaterThan(String),
    /// Greater than or equal to.
    GreaterThanOrEqual(String),
}

fn operand(v: impl Display) -> String {
    let s = v.to_string();
    s.strip_prefix('=').map(str::to_owned).unwrap_or(s)
}

impl Comparison {
    /// Between `min` and `max` (inclusive).
    pub fn between(min: impl Display, max: impl Display) -> Self {
        Comparison::Between(operand(min), operand(max))
    }
    /// Outside `min`..`max`.
    pub fn not_between(min: impl Display, max: impl Display) -> Self {
        Comparison::NotBetween(operand(min), operand(max))
    }
    /// Equal to `v`.
    pub fn equal(v: impl Display) -> Self {
        Comparison::Equal(operand(v))
    }
    /// Not equal to `v`.
    pub fn not_equal(v: impl Display) -> Self {
        Comparison::NotEqual(operand(v))
    }
    /// Less than `v`.
    pub fn less_than(v: impl Display) -> Self {
        Comparison::LessThan(operand(v))
    }
    /// Less than or equal to `v`.
    pub fn at_most(v: impl Display) -> Self {
        Comparison::LessThanOrEqual(operand(v))
    }
    /// Greater than `v`.
    pub fn greater_than(v: impl Display) -> Self {
        Comparison::GreaterThan(operand(v))
    }
    /// Greater than or equal to `v`.
    pub fn at_least(v: impl Display) -> Self {
        Comparison::GreaterThanOrEqual(operand(v))
    }

    fn parts(&self) -> (sml::ST_DataValidationOperator, &str, Option<&str>) {
        use sml::ST_DataValidationOperator as Op;
        match self {
            Comparison::Between(a, b) => (Op::Between, a, Some(b)),
            Comparison::NotBetween(a, b) => (Op::NotBetween, a, Some(b)),
            Comparison::Equal(a) => (Op::Equal, a, None),
            Comparison::NotEqual(a) => (Op::NotEqual, a, None),
            Comparison::LessThan(a) => (Op::LessThan, a, None),
            Comparison::LessThanOrEqual(a) => (Op::LessThanOrEqual, a, None),
            Comparison::GreaterThan(a) => (Op::GreaterThan, a, None),
            Comparison::GreaterThanOrEqual(a) => (Op::GreaterThanOrEqual, a, None),
        }
    }

    fn from_parts(op: Option<sml::ST_DataValidationOperator>, a: String, b: String) -> Self {
        use sml::ST_DataValidationOperator as Op;
        match op.unwrap_or(Op::Between) {
            Op::Between => Comparison::Between(a, b),
            Op::NotBetween => Comparison::NotBetween(a, b),
            Op::Equal => Comparison::Equal(a),
            Op::NotEqual => Comparison::NotEqual(a),
            Op::LessThan => Comparison::LessThan(a),
            Op::LessThanOrEqual => Comparison::LessThanOrEqual(a),
            Op::GreaterThan => Comparison::GreaterThan(a),
            Op::GreaterThanOrEqual => Comparison::GreaterThanOrEqual(a),
        }
    }
}

/// A date as a formula operand, `DATE(y,m,d)`.
pub fn date_operand(d: DateTime) -> String {
    format!("DATE({},{},{})", d.year(), d.month(), d.day())
}

/// A time of day as a formula operand, `TIME(h,m,s)`.
pub fn time_operand(hour: u8, minute: u8, second: u8) -> String {
    format!("TIME({hour},{minute},{second})")
}

/// What a data validation allows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationRule {
    /// Any value (a rule that only shows a prompt).
    Any,
    /// One of the listed values (an in-cell drop-down).
    List(Vec<String>),
    /// One of the values of a range or formula, e.g. `$E$1:$E$5` or `Choices`.
    ListSource(String),
    /// Whole numbers.
    WholeNumber(Comparison),
    /// Decimal numbers.
    Decimal(Comparison),
    /// Dates.
    Date(Comparison),
    /// Times.
    Time(Comparison),
    /// Text whose length satisfies the comparison.
    TextLength(Comparison),
    /// Values for which the formula is true (relative to the top-left cell).
    Custom(String),
}

/// A data validation rule with its prompt and error alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataValidation {
    /// The rule.
    pub rule: ValidationRule,
    /// Whether empty cells are accepted.
    pub allow_blank: bool,
    /// Whether a list shows its in-cell drop-down arrow.
    pub show_dropdown: bool,
    /// Whether the input prompt is shown when the cell is selected.
    pub show_input_message: bool,
    /// Title of the input prompt (at most 32 characters).
    pub input_title: Option<String>,
    /// Text of the input prompt (at most 255 characters).
    pub input_message: Option<String>,
    /// Whether invalid input raises the error alert.
    pub show_error_message: bool,
    /// Kind of error alert.
    pub error_style: ErrorStyle,
    /// Title of the error alert (at most 32 characters).
    pub error_title: Option<String>,
    /// Text of the error alert (at most 225 characters).
    pub error_message: Option<String>,
}

impl DataValidation {
    /// A validation with a rule and Excel's defaults (blanks allowed,
    /// prompt and alert enabled).
    pub fn new(rule: ValidationRule) -> Self {
        DataValidation {
            rule,
            allow_blank: true,
            show_dropdown: true,
            show_input_message: true,
            input_title: None,
            input_message: None,
            show_error_message: true,
            error_style: ErrorStyle::Stop,
            error_title: None,
            error_message: None,
        }
    }

    /// A drop-down list of values. The values cannot contain commas and
    /// their total length is limited to 255 characters (use
    /// [`DataValidation::list_source`] for longer lists).
    pub fn list<I, S>(items: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        if items.is_empty() {
            return Err(Error::InvalidArgument("an empty validation list".into()));
        }
        if let Some(bad) = items.iter().find(|i| i.contains(',')) {
            return Err(Error::InvalidArgument(format!(
                "list values cannot contain commas: {bad:?}"
            )));
        }
        let len = items.iter().map(|i| i.chars().count() + 1).sum::<usize>();
        if len > 256 {
            return Err(Error::InvalidArgument(
                "an inline validation list is limited to 255 characters".into(),
            ));
        }
        Ok(Self::new(ValidationRule::List(items)))
    }

    /// A drop-down list taken from a range or formula (`$E$1:$E$5`, `'Lists'!$A$1:$A$9`, a name).
    pub fn list_source(formula: impl Display) -> Self {
        Self::new(ValidationRule::ListSource(operand(formula)))
    }

    /// Whole numbers.
    pub fn whole(c: Comparison) -> Self {
        Self::new(ValidationRule::WholeNumber(c))
    }

    /// Decimal numbers.
    pub fn decimal(c: Comparison) -> Self {
        Self::new(ValidationRule::Decimal(c))
    }

    /// Dates (operands such as [`date_operand`]).
    pub fn date(c: Comparison) -> Self {
        Self::new(ValidationRule::Date(c))
    }

    /// Times (operands such as [`time_operand`]).
    pub fn time(c: Comparison) -> Self {
        Self::new(ValidationRule::Time(c))
    }

    /// Text length.
    pub fn text_length(c: Comparison) -> Self {
        Self::new(ValidationRule::TextLength(c))
    }

    /// A custom formula.
    pub fn custom(formula: impl Display) -> Self {
        Self::new(ValidationRule::Custom(operand(formula)))
    }

    /// Whether empty cells are accepted.
    pub fn allow_blank(mut self, allow: bool) -> Self {
        self.allow_blank = allow;
        self
    }

    /// Whether a list shows its drop-down arrow.
    pub fn dropdown(mut self, show: bool) -> Self {
        self.show_dropdown = show;
        self
    }

    /// The prompt shown when a cell is selected.
    pub fn input_message(mut self, title: impl Into<String>, message: impl Into<String>) -> Self {
        self.show_input_message = true;
        self.input_title = Some(title.into()).filter(|t: &String| !t.is_empty());
        self.input_message = Some(message.into()).filter(|t: &String| !t.is_empty());
        self
    }

    /// The alert shown for invalid input.
    pub fn error_message(
        mut self,
        style: ErrorStyle,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.show_error_message = true;
        self.error_style = style;
        self.error_title = Some(title.into()).filter(|t: &String| !t.is_empty());
        self.error_message = Some(message.into()).filter(|t: &String| !t.is_empty());
        self
    }

    /// Accepts any input without an alert (only the prompt is used).
    pub fn no_error_alert(mut self) -> Self {
        self.show_error_message = false;
        self
    }

    fn check(&self) -> Result<()> {
        let limit = |text: &Option<String>, max: usize, what: &str| match text {
            Some(t) if t.chars().count() > max => Err(Error::InvalidArgument(format!(
                "the {what} is longer than {max} characters"
            ))),
            _ => Ok(()),
        };
        limit(&self.input_title, 32, "input title")?;
        limit(&self.input_message, 255, "input message")?;
        limit(&self.error_title, 32, "error title")?;
        limit(&self.error_message, 225, "error message")
    }

    fn to_ct(&self, ranges: &[CellRange]) -> sml::CT_DataValidation {
        use sml::ST_DataValidationType as T;
        let mut dv = sml::CT_DataValidation {
            allow_blank: self.allow_blank.then_some(true),
            show_input_message: self.show_input_message.then_some(true),
            show_error_message: self.show_error_message.then_some(true),
            error_style: (self.error_style != ErrorStyle::Stop).then_some(self.error_style),
            prompt_title: self.input_title.clone(),
            prompt: self.input_message.clone(),
            error_title: self.error_title.clone(),
            error: self.error_message.clone(),
            sqref: Some(to_sqref(ranges)),
            ..Default::default()
        };
        let mut compare = |t: T, c: &Comparison| {
            let (op, a, b) = c.parts();
            dv.type_ = Some(t);
            dv.operator = (op != sml::ST_DataValidationOperator::Between).then_some(op);
            dv.formula1 = Some(a.to_owned());
            dv.formula2 = b.map(str::to_owned);
        };
        match &self.rule {
            ValidationRule::Any => {}
            ValidationRule::List(items) => {
                dv.type_ = Some(T::List);
                dv.formula1 = Some(format!("\"{}\"", items.join(",").replace('"', "\"\"")));
            }
            ValidationRule::ListSource(f) => {
                dv.type_ = Some(T::List);
                dv.formula1 = Some(f.clone());
            }
            ValidationRule::WholeNumber(c) => compare(T::Whole, c),
            ValidationRule::Decimal(c) => compare(T::Decimal, c),
            ValidationRule::Date(c) => compare(T::Date, c),
            ValidationRule::Time(c) => compare(T::Time, c),
            ValidationRule::TextLength(c) => compare(T::TextLength, c),
            ValidationRule::Custom(f) => {
                dv.type_ = Some(T::Custom);
                dv.formula1 = Some(f.clone());
            }
        }
        // Excel's attribute is inverted: showDropDown="1" hides the arrow.
        if dv.type_ == Some(T::List) && !self.show_dropdown {
            dv.show_drop_down = Some(true);
        }
        dv
    }

    fn from_ct(dv: &sml::CT_DataValidation) -> Self {
        use sml::ST_DataValidationType as T;
        let f1 = dv.formula1.clone().unwrap_or_default();
        let f2 = dv.formula2.clone().unwrap_or_default();
        let cmp = || Comparison::from_parts(dv.operator, f1.clone(), f2.clone());
        let rule = match dv.type_.unwrap_or(T::None) {
            T::None => ValidationRule::Any,
            T::List => match f1.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(inline) => ValidationRule::List(
                    inline
                        .replace("\"\"", "\"")
                        .split(',')
                        .map(str::to_owned)
                        .collect(),
                ),
                None => ValidationRule::ListSource(f1.clone()),
            },
            T::Whole => ValidationRule::WholeNumber(cmp()),
            T::Decimal => ValidationRule::Decimal(cmp()),
            T::Date => ValidationRule::Date(cmp()),
            T::Time => ValidationRule::Time(cmp()),
            T::TextLength => ValidationRule::TextLength(cmp()),
            T::Custom => ValidationRule::Custom(f1.clone()),
        };
        DataValidation {
            rule,
            allow_blank: dv.allow_blank.unwrap_or(false),
            show_dropdown: !dv.show_drop_down.unwrap_or(false),
            show_input_message: dv.show_input_message.unwrap_or(false),
            input_title: dv.prompt_title.clone(),
            input_message: dv.prompt.clone(),
            show_error_message: dv.show_error_message.unwrap_or(false),
            error_style: dv.error_style.unwrap_or(ErrorStyle::Stop),
            error_title: dv.error_title.clone(),
            error_message: dv.error.clone(),
        }
    }
}

impl Worksheet<'_> {
    /// The data validations of the sheet with the ranges they apply to.
    pub fn data_validations(&self) -> Vec<(Vec<CellRange>, DataValidation)> {
        self.data
            .data_validations
            .iter()
            .flat_map(|d| d.data_validation.iter())
            .map(|dv| {
                (
                    dv.sqref.as_ref().map(from_sqref).unwrap_or_default(),
                    DataValidation::from_ct(dv),
                )
            })
            .collect()
    }
}

impl WorksheetMut<'_> {
    /// Adds a data validation to one or more ranges.
    pub fn add_data_validation(&mut self, ranges: impl ToRanges, validation: &DataValidation) -> Result<()> {
        let ranges = ranges.to_ranges()?;
        validation.check()?;
        let dvs = self.data.data_validations.get_or_insert_with(Box::default);
        dvs.data_validation.push(validation.to_ct(&ranges));
        dvs.count = Some(dvs.data_validation.len() as u32);
        Ok(())
    }

    /// Removes the validation of every cell in `range`: ranges of existing
    /// validations that overlap it are dropped (they are not split), and
    /// validations left without ranges are removed. Returns the number of
    /// validations removed.
    pub fn remove_data_validations(&mut self, range: impl ToCellRange) -> Result<usize> {
        let range = range.to_cell_range()?;
        let Some(dvs) = self.data.data_validations.as_mut() else {
            return Ok(0);
        };
        let before = dvs.data_validation.len();
        dvs.data_validation.retain_mut(|dv| {
            let kept: Vec<CellRange> = dv
                .sqref
                .as_ref()
                .map(from_sqref)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| !r.intersects(&range))
                .collect();
            dv.sqref = Some(to_sqref(&kept));
            !kept.is_empty()
        });
        let removed = before - dvs.data_validation.len();
        dvs.count = Some(dvs.data_validation.len() as u32);
        if dvs.data_validation.is_empty() {
            self.data.data_validations = None;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip() {
        let r = [CellRange::parse("A1:A5").unwrap()];
        let cases = [
            DataValidation::list(["a", "b \"q\""]).unwrap().dropdown(false),
            DataValidation::list_source("=$E$1:$E$4"),
            DataValidation::whole(Comparison::between(1, 10)),
            DataValidation::decimal(Comparison::not_between(0.5, "B1")),
            DataValidation::date(Comparison::at_least(date_operand(
                DateTime::from_ymd(2024, 1, 31).unwrap(),
            ))),
            DataValidation::time(Comparison::less_than(time_operand(18, 0, 0))),
            DataValidation::text_length(Comparison::at_most(20)).allow_blank(false),
            DataValidation::custom("=ISNUMBER(A1)")
                .input_message("Title", "Message")
                .error_message(ErrorStyle::Warning, "Oops", "Not a number"),
            DataValidation::new(ValidationRule::Any)
                .input_message("Hint", "")
                .no_error_alert(),
            DataValidation::whole(Comparison::equal(3)),
            DataValidation::whole(Comparison::not_equal(3)),
            DataValidation::whole(Comparison::greater_than(3)),
        ];
        for dv in cases {
            let ct = dv.to_ct(&r);
            let xml = sml::elements::WORKSHEET.to_xml(&sml::CT_Worksheet {
                data_validations: Some(Box::new(sml::CT_DataValidations {
                    data_validation: vec![ct],
                    ..Default::default()
                })),
                ..Default::default()
            });
            let back = sml::elements::WORKSHEET.parse(&xml).unwrap();
            let got = DataValidation::from_ct(&back.data_validations.unwrap().data_validation[0]);
            assert_eq!(got, dv, "{xml}");
        }
        assert_eq!(
            DataValidation::list_source("=$E$1:$E$4").rule,
            ValidationRule::ListSource("$E$1:$E$4".into()),
            "a leading = is dropped"
        );
    }

    #[test]
    fn limits_are_checked() {
        assert!(DataValidation::list(["a,b"]).is_err());
        assert!(DataValidation::list(Vec::<String>::new()).is_err());
        assert!(DataValidation::list(vec!["x".repeat(200), "y".repeat(60)]).is_err());
        assert!(DataValidation::list(vec!["x".repeat(254)]).is_ok());
        let long = DataValidation::whole(Comparison::equal(1)).input_message("t".repeat(33), "m");
        assert!(long.check().is_err());
        let ok =
            DataValidation::whole(Comparison::equal(1)).error_message(ErrorStyle::Stop, "t", "m".repeat(225));
        assert!(ok.check().is_ok());
    }
}
