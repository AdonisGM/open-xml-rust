//! Conversion between XML Schema simple-type lexical forms and Rust values.

use std::fmt::Write as _;

/// A value with an XML Schema lexical representation.
///
/// Implemented for the Rust types that the code generator maps XML Schema
/// built-in types to, and for every generated simple type.
pub trait XmlValue: Sized {
    /// Parses the lexical form. Returns `None` if the text is not a valid value.
    fn parse_xml(s: &str) -> Option<Self>;

    /// Appends the canonical lexical form to `out`.
    fn write_xml(&self, out: &mut String);

    /// Returns the lexical form as a new `String`.
    fn to_xml_string(&self) -> String {
        let mut s = String::new();
        self.write_xml(&mut s);
        s
    }
}

impl XmlValue for String {
    fn parse_xml(s: &str) -> Option<Self> {
        Some(s.to_owned())
    }
    fn write_xml(&self, out: &mut String) {
        out.push_str(self);
    }
}

impl XmlValue for bool {
    /// `xsd:boolean`: `true`, `false`, `1` or `0` (surrounding whitespace allowed).
    fn parse_xml(s: &str) -> Option<Self> {
        match s.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }
    fn write_xml(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

macro_rules! int_value {
    ($($t:ty),*) => {$(
        impl XmlValue for $t {
            fn parse_xml(s: &str) -> Option<Self> {
                s.trim().parse().ok()
            }
            fn write_xml(&self, out: &mut String) {
                let _ = write!(out, "{self}");
            }
        }
    )*};
}
int_value!(i8, i16, i32, i64, u8, u16, u32, u64);

macro_rules! float_value {
    ($($t:ty),*) => {$(
        impl XmlValue for $t {
            /// `xsd:double` / `xsd:float` / `xsd:decimal`, including `INF`, `-INF` and `NaN`.
            fn parse_xml(s: &str) -> Option<Self> {
                match s.trim() {
                    "INF" | "+INF" => Some(<$t>::INFINITY),
                    "-INF" => Some(<$t>::NEG_INFINITY),
                    "NaN" => Some(<$t>::NAN),
                    t => {
                        // Rust accepts spellings such as "inf" or "infinity" that XML Schema does not.
                        if t.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
                            return None;
                        }
                        t.parse().ok()
                    }
                }
            }
            fn write_xml(&self, out: &mut String) {
                if self.is_nan() {
                    out.push_str("NaN");
                } else if self.is_infinite() {
                    out.push_str(if *self > 0.0 { "INF" } else { "-INF" });
                } else {
                    let _ = write!(out, "{self}");
                }
            }
        }
    )*};
}
float_value!(f32, f64);

/// Binary data with the `xsd:hexBinary` lexical form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HexBinary(pub Vec<u8>);

impl HexBinary {
    /// Creates a value from bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        HexBinary(bytes.into())
    }

    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Interprets up to eight bytes as a big-endian unsigned number.
    pub fn to_u64(&self) -> Option<u64> {
        (self.0.len() <= 8).then(|| self.0.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
    }
}

impl XmlValue for HexBinary {
    fn parse_xml(s: &str) -> Option<Self> {
        let s = s.trim();
        if !s.len().is_multiple_of(2) {
            return None;
        }
        let digits = s.as_bytes();
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        digits
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| Some(hex(p[0])? << 4 | hex(p[1])?))
            .collect::<Option<Vec<u8>>>()
            .map(HexBinary)
    }
    fn write_xml(&self, out: &mut String) {
        for b in &self.0 {
            let _ = write!(out, "{b:02X}");
        }
    }
}

/// Binary data with the `xsd:base64Binary` lexical form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Base64Binary(pub Vec<u8>);

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl XmlValue for Base64Binary {
    fn parse_xml(s: &str) -> Option<Self> {
        let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if !clean.len().is_multiple_of(4) {
            return None;
        }
        let val = |b: u8| -> Option<u32> {
            Some(match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            } as u32)
        };
        let mut out = Vec::with_capacity(clean.len() / 4 * 3);
        let chunks = clean.len() / 4;
        for (i, q) in clean.as_chunks::<4>().0.iter().enumerate() {
            let pad = q.iter().rev().take_while(|&&b| b == b'=').count();
            if pad > 2 || (pad > 0 && i + 1 != chunks) || q[..4 - pad].contains(&b'=') {
                return None;
            }
            let mut n = 0u32;
            for &b in &q[..4 - pad] {
                n = n << 6 | val(b)?;
            }
            n <<= 6 * pad as u32;
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..3 - pad]);
        }
        Some(Base64Binary(out))
    }
    fn write_xml(&self, out: &mut String) {
        for chunk in self.0.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
    }
}

/// A whitespace-separated list (`xsd:list`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct XmlList<T>(pub Vec<T>);

impl<T> Default for XmlList<T> {
    fn default() -> Self {
        XmlList(Vec::new())
    }
}

impl<T: XmlValue> XmlValue for XmlList<T> {
    fn parse_xml(s: &str) -> Option<Self> {
        s.split_ascii_whitespace()
            .map(T::parse_xml)
            .collect::<Option<Vec<T>>>()
            .map(XmlList)
    }
    fn write_xml(&self, out: &mut String) {
        for (i, item) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            item.write_xml(out);
        }
    }
}

impl<T> From<Vec<T>> for XmlList<T> {
    fn from(v: Vec<T>) -> Self {
        XmlList(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt<T: XmlValue + PartialEq + std::fmt::Debug>(s: &str, v: T, canonical: &str) {
        assert_eq!(T::parse_xml(s), Some(v), "parse {s:?}");
        let parsed = T::parse_xml(s).unwrap();
        assert_eq!(parsed.to_xml_string(), canonical, "format {s:?}");
    }

    #[test]
    fn booleans() {
        rt("true", true, "true");
        rt("1", true, "true");
        rt(" false ", false, "false");
        rt("0", false, "false");
        assert_eq!(bool::parse_xml("on"), None);
        assert_eq!(bool::parse_xml("TRUE"), None);
    }

    #[test]
    fn integers() {
        rt("42", 42i32, "42");
        rt("+7", 7i64, "7");
        rt(" -3 ", -3i16, "-3");
        rt("255", 255u8, "255");
        assert_eq!(u8::parse_xml("256"), None);
        assert_eq!(u32::parse_xml("-1"), None);
        assert_eq!(i32::parse_xml("1.0"), None);
        assert_eq!(i32::parse_xml(""), None);
        rt("18446744073709551615", u64::MAX, "18446744073709551615");
    }

    #[test]
    fn floats() {
        rt("1.5", 1.5f64, "1.5");
        rt("100", 100.0f64, "100");
        rt("1e3", 1000.0f64, "1000");
        rt("-0.25", -0.25f32, "-0.25");
        rt("INF", f64::INFINITY, "INF");
        rt("-INF", f64::NEG_INFINITY, "-INF");
        assert!(f64::parse_xml("NaN").unwrap().is_nan());
        assert_eq!(f64::NAN.to_xml_string(), "NaN");
        assert_eq!(f64::parse_xml("inf"), None);
        assert_eq!(f64::parse_xml("infinity"), None);
        assert_eq!(f64::parse_xml("abc"), None);
    }

    #[test]
    fn strings_are_verbatim() {
        rt("  a b  ", String::from("  a b  "), "  a b  ");
        rt("", String::new(), "");
    }

    #[test]
    fn hex_binary() {
        rt("00ff10", HexBinary(vec![0, 255, 16]), "00FF10");
        rt("", HexBinary(vec![]), "");
        assert_eq!(HexBinary::parse_xml("abc"), None);
        assert_eq!(HexBinary::parse_xml("zz"), None);
        assert_eq!(HexBinary::new([0x01, 0x02]).to_u64(), Some(0x0102));
        assert_eq!(HexBinary::new(vec![0; 9]).to_u64(), None);
        assert_eq!(HexBinary::new([7]).as_bytes(), &[7]);
    }

    #[test]
    fn base64_binary() {
        for (text, bytes) in [
            ("", &b""[..]),
            ("Zg==", b"f"),
            ("Zm8=", b"fo"),
            ("Zm9v", b"foo"),
            ("Zm9vYg==", b"foob"),
            ("Zm9vYmE=", b"fooba"),
            ("Zm9vYmFy", b"foobar"),
        ] {
            rt(text, Base64Binary(bytes.to_vec()), text);
        }
        assert_eq!(Base64Binary::parse_xml("Zm9v\n YmFy").unwrap().0, b"foobar");
        assert_eq!(Base64Binary::parse_xml("Zm9"), None);
        assert_eq!(Base64Binary::parse_xml("Z==="), None);
        assert_eq!(Base64Binary::parse_xml("Zg==Zg=="), None);
        assert_eq!(Base64Binary::parse_xml("Zm!v"), None);
        let all: Vec<u8> = (0..=255).collect();
        let enc = Base64Binary(all.clone()).to_xml_string();
        assert_eq!(Base64Binary::parse_xml(&enc).unwrap().0, all);
    }

    #[test]
    fn lists() {
        rt("1 2  3", XmlList(vec![1u32, 2, 3]), "1 2 3");
        rt("", XmlList::<u32>(vec![]), "");
        assert_eq!(XmlList::<u32>::parse_xml("1 x"), None);
        assert_eq!(XmlList::<u8>::default(), XmlList(vec![]));
        assert_eq!(XmlList::from(vec![true]).to_xml_string(), "true");
    }
}
