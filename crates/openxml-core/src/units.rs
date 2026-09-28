//! Lengths and their Office Open XML units.
//!
//! Office Open XML mixes several units: DrawingML uses English Metric Units
//! (EMU, 914 400 per inch), WordprocessingML page geometry uses twentieths of
//! a point (twips), and font sizes use half-points (WML) or hundredths of a
//! point (DrawingML). [`Length`] stores EMUs and converts to all of them.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// EMUs per inch.
pub const EMU_PER_INCH: i64 = 914_400;
/// EMUs per centimetre.
pub const EMU_PER_CM: i64 = 360_000;
/// EMUs per point.
pub const EMU_PER_POINT: i64 = 12_700;
/// EMUs per twip (1/20 point).
pub const EMU_PER_TWIP: i64 = 635;
/// EMUs per pixel at 96 DPI.
pub const EMU_PER_PIXEL: i64 = 9_525;

/// A length, stored in EMUs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Length(i64);

fn round(v: f64) -> i64 {
    v.round() as i64
}

impl Length {
    /// Zero length.
    pub const ZERO: Length = Length(0);

    /// From EMUs.
    pub const fn emu(v: i64) -> Self {
        Length(v)
    }
    /// From inches.
    pub fn inches(v: f64) -> Self {
        Length(round(v * EMU_PER_INCH as f64))
    }
    /// From centimetres.
    pub fn cm(v: f64) -> Self {
        Length(round(v * EMU_PER_CM as f64))
    }
    /// From millimetres.
    pub fn mm(v: f64) -> Self {
        Length(round(v * EMU_PER_CM as f64 / 10.0))
    }
    /// From points.
    pub fn pt(v: f64) -> Self {
        Length(round(v * EMU_PER_POINT as f64))
    }
    /// From pixels at 96 DPI.
    pub fn px(v: f64) -> Self {
        Length(round(v * EMU_PER_PIXEL as f64))
    }
    /// From twips (twentieths of a point).
    pub const fn twips(v: i64) -> Self {
        Length(v * EMU_PER_TWIP)
    }

    /// In EMUs.
    pub const fn as_emu(self) -> i64 {
        self.0
    }
    /// In twips (rounded).
    pub fn as_twips(self) -> i64 {
        round(self.0 as f64 / EMU_PER_TWIP as f64)
    }
    /// In points.
    pub fn as_pt(self) -> f64 {
        self.0 as f64 / EMU_PER_POINT as f64
    }
    /// In inches.
    pub fn as_inches(self) -> f64 {
        self.0 as f64 / EMU_PER_INCH as f64
    }
    /// In centimetres.
    pub fn as_cm(self) -> f64 {
        self.0 as f64 / EMU_PER_CM as f64
    }
    /// In pixels at 96 DPI.
    pub fn as_px(self) -> f64 {
        self.0 as f64 / EMU_PER_PIXEL as f64
    }
}

impl Add for Length {
    type Output = Length;
    fn add(self, o: Length) -> Length {
        Length(self.0 + o.0)
    }
}
impl Sub for Length {
    type Output = Length;
    fn sub(self, o: Length) -> Length {
        Length(self.0 - o.0)
    }
}
impl Neg for Length {
    type Output = Length;
    fn neg(self) -> Length {
        Length(-self.0)
    }
}
impl Mul<i64> for Length {
    type Output = Length;
    fn mul(self, k: i64) -> Length {
        Length(self.0 * k)
    }
}
impl Mul<f64> for Length {
    type Output = Length;
    fn mul(self, k: f64) -> Length {
        Length(round(self.0 as f64 * k))
    }
}
impl Div<i64> for Length {
    type Output = Length;
    fn div(self, k: i64) -> Length {
        Length(self.0 / k)
    }
}

/// A font size in points, with the unit conversions used by the formats.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct FontSize(pub f64);

impl FontSize {
    /// WordprocessingML half-points (`w:sz`), rounded.
    pub fn half_points(self) -> u64 {
        (self.0 * 2.0).round().max(0.0) as u64
    }
    /// DrawingML hundredths of a point (`a:rPr/@sz`), rounded.
    pub fn hundredths(self) -> i32 {
        (self.0 * 100.0).round() as i32
    }
    /// From WordprocessingML half-points.
    pub fn from_half_points(v: u64) -> Self {
        FontSize(v as f64 / 2.0)
    }
    /// From DrawingML hundredths of a point.
    pub fn from_hundredths(v: i32) -> Self {
        FontSize(f64::from(v) / 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(Length::inches(1.0).as_emu(), 914_400);
        assert_eq!(Length::cm(2.54).as_emu(), 914_400);
        assert_eq!(Length::mm(10.0), Length::cm(1.0));
        assert_eq!(Length::pt(72.0), Length::inches(1.0));
        assert_eq!(Length::px(96.0), Length::inches(1.0));
        assert_eq!(Length::twips(1440), Length::inches(1.0));
        assert_eq!(Length::inches(1.0).as_twips(), 1440);
        assert!((Length::inches(1.0).as_pt() - 72.0).abs() < 1e-9);
        assert!((Length::emu(914_400).as_inches() - 1.0).abs() < 1e-9);
        assert!((Length::inches(1.0).as_cm() - 2.54).abs() < 1e-9);
        assert!((Length::inches(1.0).as_px() - 96.0).abs() < 1e-9);
        assert_eq!(Length::ZERO.as_emu(), 0);
    }

    #[test]
    fn arithmetic() {
        let a = Length::pt(10.0);
        let b = Length::pt(4.0);
        assert_eq!(a + b, Length::pt(14.0));
        assert_eq!(a - b, Length::pt(6.0));
        assert_eq!(-a, Length::pt(-10.0));
        assert_eq!(a * 2, Length::pt(20.0));
        assert_eq!(a * 0.5, Length::pt(5.0));
        assert_eq!(a / 2, Length::pt(5.0));
        assert!(b < a);
    }

    #[test]
    fn font_sizes() {
        assert_eq!(FontSize(11.0).half_points(), 22);
        assert_eq!(FontSize(10.5).half_points(), 21);
        assert_eq!(FontSize(18.0).hundredths(), 1800);
        assert_eq!(FontSize::from_half_points(24), FontSize(12.0));
        assert_eq!(FontSize::from_hundredths(1450), FontSize(14.5));
        assert_eq!(FontSize(-1.0).half_points(), 0);
    }
}
