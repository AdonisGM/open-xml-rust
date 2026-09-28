//! Small conversions between high-level values and generated schema types.

use openxml_core::{Error, Length, Result};
use openxml_schema::shared_types::{ST_OnOff, ST_OnOff1, ST_TwipsMeasure};
use openxml_schema::wml::{self, ST_HexColor, ST_HexColorAuto, ST_SignedTwipsMeasure};
use openxml_xml::HexBinary;

/// An on/off property that is on (`<w:b/>`).
pub(crate) fn on() -> Box<wml::CT_OnOff> {
    Box::default()
}

/// An on/off property that is explicitly off (`<w:b w:val="0"/>`).
pub(crate) fn off() -> Box<wml::CT_OnOff> {
    Box::new(wml::CT_OnOff {
        val: Some(ST_OnOff::Boolean(false)),
        ..Default::default()
    })
}

/// The value of an on/off property element (`val` omitted means on).
pub(crate) fn on_off_value(v: &wml::CT_OnOff) -> bool {
    match &v.val {
        None => true,
        Some(ST_OnOff::Boolean(b)) => *b,
        Some(ST_OnOff::OnOff1(ST_OnOff1::On)) => true,
        Some(ST_OnOff::OnOff1(ST_OnOff1::Off)) => false,
    }
}

/// The value of an optional on/off property (absent means off).
pub(crate) fn is_on(v: &Option<Box<wml::CT_OnOff>>) -> bool {
    v.as_deref().is_some_and(on_off_value)
}

/// A `w:val` string property.
pub(crate) fn string_val(s: &str) -> Box<wml::CT_String> {
    Box::new(wml::CT_String {
        val: Some(s.to_owned()),
        ..Default::default()
    })
}

/// A `w:val` decimal number property.
pub(crate) fn decimal(v: i64) -> Box<wml::CT_DecimalNumber> {
    Box::new(wml::CT_DecimalNumber {
        val: Some(v),
        ..Default::default()
    })
}

/// A non-negative twips measure.
pub(crate) fn twips(len: Length) -> ST_TwipsMeasure {
    ST_TwipsMeasure::UnsignedDecimalNumber(len.as_twips().max(0) as u64)
}

/// A signed twips measure.
pub(crate) fn signed_twips(len: Length) -> ST_SignedTwipsMeasure {
    ST_SignedTwipsMeasure::Integer(len.as_twips())
}

/// Parses an XML Schema "universal measure" such as `2.5cm` or `12pt`.
pub(crate) fn universal_measure(s: &str) -> Option<Length> {
    let s = s.trim();
    let split = s.len().checked_sub(2)?;
    let (number, unit) = s.split_at(split);
    let v: f64 = number.parse().ok()?;
    Some(match unit {
        "mm" => Length::mm(v),
        "cm" => Length::cm(v),
        "in" => Length::inches(v),
        "pt" => Length::pt(v),
        "pc" | "pi" => Length::pt(v * 12.0),
        _ => return None,
    })
}

/// Converts a twips measure to a length.
pub(crate) fn twips_value(v: &ST_TwipsMeasure) -> Option<Length> {
    match v {
        ST_TwipsMeasure::UnsignedDecimalNumber(n) => Some(Length::twips(*n as i64)),
        ST_TwipsMeasure::PositiveUniversalMeasure(s) => universal_measure(s),
    }
}

/// Converts a signed twips measure to a length.
pub(crate) fn signed_twips_value(v: &ST_SignedTwipsMeasure) -> Option<Length> {
    match v {
        ST_SignedTwipsMeasure::Integer(n) => Some(Length::twips(*n)),
        ST_SignedTwipsMeasure::UniversalMeasure(s) => universal_measure(s),
    }
}

/// Parses `RRGGBB` or `#RRGGBB` into three bytes.
pub(crate) fn parse_rgb(hex: &str) -> Result<HexBinary> {
    let h = hex.trim().trim_start_matches('#');
    let valid = h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid {
        return Err(Error::InvalidArgument(format!(
            "invalid RGB color {hex:?}, expected RRGGBB"
        )));
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).expect("validated hex digits");
    Ok(HexBinary(vec![byte(0), byte(2), byte(4)]))
}

/// Parses a color for a `w:color`-like attribute (`auto` or an RGB hex value).
pub(crate) fn hex_color(hex: &str) -> Result<ST_HexColor> {
    if hex.trim().eq_ignore_ascii_case("auto") {
        return Ok(ST_HexColor::HexColorAuto(ST_HexColorAuto::Auto));
    }
    parse_rgb(hex).map(ST_HexColor::HexColorRGB)
}

/// Formats a color attribute as `RRGGBB` (or `auto`).
pub(crate) fn hex_color_string(c: &ST_HexColor) -> String {
    match c {
        ST_HexColor::HexColorAuto(_) => "auto".into(),
        ST_HexColor::HexColorRGB(b) => b.0.iter().map(|x| format!("{x:02X}")).collect(),
    }
}

/// A `w:t` element; `xml:space="preserve"` is set when the text has leading
/// or trailing whitespace (otherwise consumers may trim it).
pub(crate) fn text_node(s: &str) -> Box<wml::CT_Text> {
    let needs_preserve = s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace);
    Box::new(wml::CT_Text {
        value: s.to_owned(),
        xml_space: needs_preserve.then(|| "preserve".to_owned()),
        ..Default::default()
    })
}

/// Updates `xml:space` of an existing text node after its value changed.
pub(crate) fn fix_space(t: &mut wml::CT_Text) {
    let needs_preserve = t.value.starts_with(char::is_whitespace) || t.value.ends_with(char::is_whitespace);
    if needs_preserve {
        t.xml_space = Some("preserve".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_off_semantics() {
        assert!(on_off_value(&on()));
        assert!(!on_off_value(&off()));
        let explicit_on = wml::CT_OnOff {
            val: Some(ST_OnOff::OnOff1(ST_OnOff1::On)),
            ..Default::default()
        };
        assert!(on_off_value(&explicit_on));
        let explicit_off = wml::CT_OnOff {
            val: Some(ST_OnOff::OnOff1(ST_OnOff1::Off)),
            ..Default::default()
        };
        assert!(!on_off_value(&explicit_off));
        assert!(!is_on(&None));
        assert!(is_on(&Some(on())));
    }

    #[test]
    fn measures() {
        assert_eq!(
            twips(Length::inches(1.0)),
            ST_TwipsMeasure::UnsignedDecimalNumber(1440)
        );
        assert_eq!(
            twips(Length::inches(-1.0)),
            ST_TwipsMeasure::UnsignedDecimalNumber(0)
        );
        assert_eq!(
            signed_twips(Length::pt(-1.0)),
            ST_SignedTwipsMeasure::Integer(-20)
        );
        assert_eq!(
            twips_value(&ST_TwipsMeasure::UnsignedDecimalNumber(720)),
            Some(Length::inches(0.5))
        );
        assert_eq!(
            twips_value(&ST_TwipsMeasure::PositiveUniversalMeasure("2.54cm".into())),
            Some(Length::inches(1.0))
        );
        assert_eq!(
            signed_twips_value(&ST_SignedTwipsMeasure::UniversalMeasure("-1in".into())),
            Some(Length::inches(-1.0))
        );
        assert_eq!(universal_measure("12pt"), Some(Length::pt(12.0)));
        assert_eq!(universal_measure("1pc"), Some(Length::pt(12.0)));
        assert_eq!(universal_measure("10mm"), Some(Length::cm(1.0)));
        assert_eq!(universal_measure("3px"), None);
        assert_eq!(universal_measure("x"), None);
        assert_eq!(universal_measure("abcm"), None);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_rgb("#FF8000").unwrap().0, vec![255, 128, 0]);
        assert_eq!(parse_rgb("00ff00").unwrap().0, vec![0, 255, 0]);
        assert!(parse_rgb("red").is_err());
        assert!(parse_rgb("12345").is_err());
        assert!(parse_rgb("GG0000").is_err());
        assert_eq!(hex_color_string(&hex_color("auto").unwrap()), "auto");
        assert_eq!(hex_color_string(&hex_color("#1f4e79").unwrap()), "1F4E79");
    }

    #[test]
    fn text_nodes_preserve_edge_whitespace() {
        assert_eq!(text_node("a b").xml_space, None);
        assert_eq!(text_node(" a").xml_space.as_deref(), Some("preserve"));
        assert_eq!(text_node("a\t").xml_space.as_deref(), Some("preserve"));
        let mut t = *text_node("x");
        t.value = "x ".into();
        fix_space(&mut t);
        assert_eq!(t.xml_space.as_deref(), Some("preserve"));
        assert_eq!(string_val("s").val.as_deref(), Some("s"));
        assert_eq!(decimal(3).val, Some(3));
    }
}
