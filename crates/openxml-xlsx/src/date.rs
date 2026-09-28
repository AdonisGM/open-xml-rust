//! Excel date serial numbers (ECMA-376 Part 1 §18.17.4).
//!
//! SpreadsheetML stores dates as numbers: whole days since an epoch plus a
//! fraction of a day. Two epochs exist:
//!
//! * the **1900 date system** (default): serial 1 is 1900-01-01. For
//!   compatibility with early spreadsheet programs it contains the
//!   non-existent day 1900-02-29 (serial 60), so every serial from 61 on is
//!   one larger than the real day count;
//! * the **1904 date system** (`workbookPr/@date1904`): serial 0 is 1904-01-01.

use std::fmt;

/// The epoch used by a workbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DateSystem {
    /// Serial 1 = 1900-01-01, including the fictitious 1900-02-29.
    #[default]
    V1900,
    /// Serial 0 = 1904-01-01.
    V1904,
}

/// A calendar date and time of day, as stored in a cell.
///
/// `DateTime` accepts the day 1900-02-29, which exists only in the 1900 date
/// system, so that every serial number maps to a distinct value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DateTime {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    millisecond: u16,
}

const MS_PER_DAY: i64 = 86_400_000;
/// Largest serial Excel accepts (9999-12-31) in the 1900 system.
const MAX_SERIAL_1900: f64 = 2_958_466.0;

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

impl DateTime {
    /// A date at midnight. Returns `None` for invalid dates.
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Option<Self> {
        Self::from_ymd_hms_milli(year, month, day, 0, 0, 0, 0)
    }

    /// A date and time. Returns `None` for invalid values.
    pub fn from_ymd_hms(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        Self::from_ymd_hms_milli(year, month, day, hour, minute, second, 0)
    }

    /// A date and time with milliseconds. Returns `None` for invalid values.
    pub fn from_ymd_hms_milli(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        millisecond: u16,
    ) -> Option<Self> {
        let fictitious_leap_day = year == 1900 && month == 2 && day == 29;
        let valid_date = (1..=9999).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && (day <= days_in_month(year, month) || fictitious_leap_day);
        let valid_time = hour < 24 && minute < 60 && second < 60 && millisecond < 1000;
        (valid_date && valid_time).then_some(DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        })
    }

    /// Year.
    pub fn year(&self) -> i32 {
        self.year
    }
    /// Month (1–12).
    pub fn month(&self) -> u8 {
        self.month
    }
    /// Day of month (1–31).
    pub fn day(&self) -> u8 {
        self.day
    }
    /// Hour (0–23).
    pub fn hour(&self) -> u8 {
        self.hour
    }
    /// Minute (0–59).
    pub fn minute(&self) -> u8 {
        self.minute
    }
    /// Second (0–59).
    pub fn second(&self) -> u8 {
        self.second
    }
    /// Millisecond (0–999).
    pub fn millisecond(&self) -> u16 {
        self.millisecond
    }

    /// Whether the time of day is midnight.
    pub fn is_midnight(&self) -> bool {
        self.hour == 0 && self.minute == 0 && self.second == 0 && self.millisecond == 0
    }

    fn day_serial(&self, system: DateSystem) -> Option<i64> {
        let (y, m, d) = (i64::from(self.year), u32::from(self.month), u32::from(self.day));
        match system {
            DateSystem::V1900 => {
                if self.year == 1900 && self.month == 2 && self.day == 29 {
                    return Some(60);
                }
                let days = days_from_civil(y, m, d);
                let before_leap_bug = (self.year, self.month) < (1900, 3);
                let serial = if before_leap_bug {
                    days - days_from_civil(1899, 12, 31)
                } else {
                    days - days_from_civil(1899, 12, 30)
                };
                (serial >= 0).then_some(serial)
            }
            DateSystem::V1904 => {
                if self.year == 1900 && self.month == 2 && self.day == 29 {
                    return None;
                }
                let serial = days_from_civil(y, m, d) - days_from_civil(1904, 1, 1);
                (serial >= 0).then_some(serial)
            }
        }
    }

    /// The serial number of this value, or `None` if it precedes the epoch
    /// (1899-12-31 for the 1900 system, 1904-01-01 for the 1904 system).
    pub fn to_serial(&self, system: DateSystem) -> Option<f64> {
        let days = self.day_serial(system)?;
        let ms = i64::from(self.hour) * 3_600_000
            + i64::from(self.minute) * 60_000
            + i64::from(self.second) * 1000
            + i64::from(self.millisecond);
        Some(days as f64 + ms as f64 / MS_PER_DAY as f64)
    }

    /// Converts a serial number, rounding the time to the nearest millisecond.
    /// Returns `None` for negative, non-finite or too large serials.
    pub fn from_serial(serial: f64, system: DateSystem) -> Option<Self> {
        if !serial.is_finite() || serial < 0.0 {
            return None;
        }
        let max = match system {
            DateSystem::V1900 => MAX_SERIAL_1900,
            DateSystem::V1904 => MAX_SERIAL_1900 - 1462.0,
        };
        if serial >= max {
            return None;
        }
        let mut days = serial.floor() as i64;
        let mut ms = ((serial - days as f64) * MS_PER_DAY as f64).round() as i64;
        if ms >= MS_PER_DAY {
            days += 1;
            ms -= MS_PER_DAY;
        }
        let (y, m, d) = match system {
            DateSystem::V1900 if days == 60 => (1900, 2, 29),
            DateSystem::V1900 if days < 60 => civil_from_days(days_from_civil(1899, 12, 31) + days),
            DateSystem::V1900 => civil_from_days(days_from_civil(1899, 12, 30) + days),
            DateSystem::V1904 => civil_from_days(days_from_civil(1904, 1, 1) + days),
        };
        if y > 9999 {
            return None;
        }
        DateTime::from_ymd_hms_milli(
            y as i32,
            m as u8,
            d as u8,
            (ms / 3_600_000) as u8,
            (ms / 60_000 % 60) as u8,
            (ms / 1000 % 60) as u8,
            (ms % 1000) as u16,
        )
    }

    /// Parses an ISO 8601 date or date-time: `2024-01-31`, `2024-01-31T08:30`,
    /// `2024-01-31T08:30:15.250Z` (time zones other than `Z` are ignored).
    pub fn parse_iso(s: &str) -> Option<Self> {
        let s = s.trim();
        let (date, time) = match s.split_once(['T', ' ']) {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        };
        let mut parts = date.split('-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u8 = parts.next()?.parse().ok()?;
        let day: u8 = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let (mut hour, mut minute, mut second, mut milli) = (0u8, 0u8, 0u8, 0u16);
        if let Some(t) = time {
            let t = t.trim_end_matches('Z');
            let t = t.split(['+']).next()?;
            let mut hms = t.split(':');
            hour = hms.next()?.parse().ok()?;
            minute = hms.next()?.parse().ok()?;
            if let Some(sec) = hms.next() {
                let (whole, frac) = sec.split_once('.').unwrap_or((sec, ""));
                second = whole.parse().ok()?;
                if !frac.is_empty() {
                    let digits: String = frac.chars().chain("000".chars()).take(3).collect();
                    milli = digits.parse().ok()?;
                }
            }
        }
        DateTime::from_ymd_hms_milli(year, month, day, hour, minute, second, milli)
    }

    /// ISO 8601 text: `YYYY-MM-DD` at midnight, `YYYY-MM-DDTHH:MM:SS[.fff]` otherwise.
    pub fn to_iso(&self) -> String {
        let mut s = format!("{:04}-{:02}-{:02}", self.year, self.month, self.day);
        if !self.is_midnight() {
            s.push_str(&format!(
                "T{:02}:{:02}:{:02}",
                self.hour, self.minute, self.second
            ));
            if self.millisecond != 0 {
                s.push_str(&format!(".{:03}", self.millisecond));
            }
        }
        s
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_iso())
    }
}

/// Whether a number format displays dates or times.
///
/// Built-in formats 14–22, 27–36, 45–47 and 50–58 are date/time formats
/// (ECMA-376 Part 1 §18.8.30). Custom format codes are inspected: quoted
/// text, escaped characters and bracketed modifiers (colours, locales,
/// conditions) are ignored, elapsed-time brackets (`[h]`, `[mm]`, `[ss]`)
/// count as time, and any remaining `y`, `m`, `d`, `h` or `s` marks a date.
pub fn is_date_format(num_fmt_id: u32, format_code: Option<&str>) -> bool {
    if matches!(num_fmt_id, 14..=22 | 27..=36 | 45..=47 | 50..=58) {
        return true;
    }
    match format_code {
        Some(code) => is_date_format_code(code),
        None => false,
    }
}

/// Whether a custom number format code displays dates or times.
pub fn is_date_format_code(code: &str) -> bool {
    let mut chars = code.chars().peekable();
    let mut cleaned = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                let mut inner = String::new();
                for q in chars.by_ref() {
                    if q == ']' {
                        break;
                    }
                    inner.push(q);
                }
                let lower = inner.to_ascii_lowercase();
                if !lower.is_empty() && lower.chars().all(|ch| matches!(ch, 'h' | 'm' | 's')) {
                    return true;
                }
            }
            _ => cleaned.push(c.to_ascii_lowercase()),
        }
    }
    let cleaned = cleaned.replace("general", "");
    cleaned.chars().any(|c| matches!(c, 'y' | 'm' | 'd' | 'h' | 's'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u8, day: u8) -> DateTime {
        DateTime::from_ymd(y, m, day).unwrap()
    }

    #[test]
    fn serials_of_the_1900_system() {
        assert_eq!(d(1899, 12, 31).to_serial(DateSystem::V1900), Some(0.0));
        assert_eq!(d(1900, 1, 1).to_serial(DateSystem::V1900), Some(1.0));
        assert_eq!(d(1900, 2, 28).to_serial(DateSystem::V1900), Some(59.0));
        assert_eq!(
            d(1900, 2, 29).to_serial(DateSystem::V1900),
            Some(60.0),
            "fictitious leap day"
        );
        assert_eq!(d(1900, 3, 1).to_serial(DateSystem::V1900), Some(61.0));
        assert_eq!(d(2000, 1, 1).to_serial(DateSystem::V1900), Some(36_526.0));
        assert_eq!(d(2024, 2, 29).to_serial(DateSystem::V1900), Some(45_351.0));
        assert_eq!(d(9999, 12, 31).to_serial(DateSystem::V1900), Some(2_958_465.0));
        assert_eq!(
            d(1899, 12, 30).to_serial(DateSystem::V1900),
            None,
            "before the epoch"
        );
    }

    #[test]
    fn serials_of_the_1904_system() {
        assert_eq!(d(1904, 1, 1).to_serial(DateSystem::V1904), Some(0.0));
        assert_eq!(d(1904, 1, 2).to_serial(DateSystem::V1904), Some(1.0));
        assert_eq!(d(2000, 1, 1).to_serial(DateSystem::V1904), Some(35_064.0));
        assert_eq!(d(1903, 12, 31).to_serial(DateSystem::V1904), None);
        assert_eq!(d(1900, 2, 29).to_serial(DateSystem::V1904), None);
        // The two systems differ by 1462 days for modern dates.
        let x = d(2024, 6, 1);
        assert_eq!(
            x.to_serial(DateSystem::V1900).unwrap() - x.to_serial(DateSystem::V1904).unwrap(),
            1462.0
        );
    }

    #[test]
    fn from_serial_is_the_inverse() {
        for system in [DateSystem::V1900, DateSystem::V1904] {
            for serial in [0.0, 1.0, 59.0, 60.0, 61.0, 366.0, 36_526.0, 45_351.0, 2_000_000.0] {
                let Some(dt) = DateTime::from_serial(serial, system) else {
                    continue;
                };
                assert_eq!(dt.to_serial(system), Some(serial), "{serial} {system:?}");
            }
        }
        assert_eq!(
            DateTime::from_serial(60.0, DateSystem::V1900),
            Some(d(1900, 2, 29))
        );
        assert_eq!(
            DateTime::from_serial(61.0, DateSystem::V1900),
            Some(d(1900, 3, 1))
        );
        assert_eq!(
            DateTime::from_serial(59.0, DateSystem::V1900),
            Some(d(1900, 2, 28))
        );
        assert_eq!(DateTime::from_serial(0.0, DateSystem::V1904), Some(d(1904, 1, 1)));
    }

    #[test]
    fn every_day_round_trips() {
        for serial in 0..80_000 {
            for system in [DateSystem::V1900, DateSystem::V1904] {
                let dt = DateTime::from_serial(f64::from(serial), system).unwrap();
                assert_eq!(dt.to_serial(system), Some(f64::from(serial)));
            }
        }
    }

    #[test]
    fn time_of_day() {
        let dt = DateTime::from_serial(45_351.75, DateSystem::V1900).unwrap();
        assert_eq!(
            (
                dt.year(),
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second()
            ),
            (2024, 2, 29, 18, 0, 0)
        );
        let noon = DateTime::from_serial(0.5, DateSystem::V1900).unwrap();
        assert_eq!(
            (noon.year(), noon.month(), noon.day(), noon.hour()),
            (1899, 12, 31, 12)
        );
        let t = DateTime::from_ymd_hms_milli(2020, 1, 1, 13, 14, 15, 500).unwrap();
        let back = DateTime::from_serial(t.to_serial(DateSystem::V1900).unwrap(), DateSystem::V1900).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.millisecond(), 500);
        // Values within half a millisecond of midnight round to the next day.
        let almost = DateTime::from_serial(1.999_999_999_9, DateSystem::V1900).unwrap();
        assert_eq!((almost.day(), almost.hour()), (2, 0));
        assert!(almost.is_midnight());
    }

    #[test]
    fn invalid_values() {
        assert!(DateTime::from_ymd(2023, 2, 29).is_none());
        assert!(DateTime::from_ymd(1900, 2, 30).is_none());
        assert!(DateTime::from_ymd(2024, 13, 1).is_none());
        assert!(DateTime::from_ymd(2024, 4, 31).is_none());
        assert!(DateTime::from_ymd(0, 1, 1).is_none());
        assert!(DateTime::from_ymd(10_000, 1, 1).is_none());
        assert!(DateTime::from_ymd_hms(2024, 1, 1, 24, 0, 0).is_none());
        assert!(DateTime::from_ymd_hms(2024, 1, 1, 0, 60, 0).is_none());
        assert!(DateTime::from_ymd_hms_milli(2024, 1, 1, 0, 0, 0, 1000).is_none());
        assert!(DateTime::from_serial(-1.0, DateSystem::V1900).is_none());
        assert!(DateTime::from_serial(f64::NAN, DateSystem::V1900).is_none());
        assert!(DateTime::from_serial(3_000_000.0, DateSystem::V1900).is_none());
        assert!(DateTime::from_ymd(2000, 2, 29).is_some(), "divisible by 400");
        assert!(DateTime::from_ymd(2100, 2, 29).is_none(), "divisible by 100");
    }

    #[test]
    fn iso_format() {
        assert_eq!(DateTime::parse_iso("2024-01-31"), Some(d(2024, 1, 31)));
        assert_eq!(
            DateTime::parse_iso("2024-01-31T08:30"),
            DateTime::from_ymd_hms(2024, 1, 31, 8, 30, 0)
        );
        assert_eq!(
            DateTime::parse_iso("2024-01-31T08:30:15.25Z"),
            DateTime::from_ymd_hms_milli(2024, 1, 31, 8, 30, 15, 250)
        );
        assert_eq!(
            DateTime::parse_iso("2024-01-31 08:30:15"),
            DateTime::from_ymd_hms(2024, 1, 31, 8, 30, 15)
        );
        for bad in [
            "",
            "2024",
            "2024-13-01",
            "2024-01-01T25:00",
            "2024-01-01-01",
            "abc",
        ] {
            assert!(DateTime::parse_iso(bad).is_none(), "{bad:?}");
        }
        assert_eq!(d(2024, 1, 5).to_iso(), "2024-01-05");
        assert_eq!(
            DateTime::from_ymd_hms(2024, 1, 5, 7, 8, 9).unwrap().to_string(),
            "2024-01-05T07:08:09"
        );
        assert_eq!(
            DateTime::from_ymd_hms_milli(2024, 1, 5, 7, 8, 9, 42)
                .unwrap()
                .to_iso(),
            "2024-01-05T07:08:09.042"
        );
    }

    #[test]
    fn date_format_detection() {
        for id in [14, 15, 16, 17, 18, 19, 20, 21, 22, 27, 36, 45, 46, 47, 50, 58] {
            assert!(is_date_format(id, None), "{id}");
        }
        for id in [0, 1, 2, 9, 10, 11, 12, 13, 23, 37, 44, 48, 49, 59, 164] {
            assert!(!is_date_format(id, None), "{id}");
        }
        for code in [
            "yyyy-mm-dd",
            "d/m/yy",
            "hh:mm",
            "[h]:mm:ss",
            "mmm yyyy",
            "dd\\-mm",
            "[$-409]mmmm d, yyyy",
            "[Red]yyyy",
            "h:mm AM/PM",
            "[ss]",
        ] {
            assert!(is_date_format(164, Some(code)), "{code}");
        }
        for code in [
            "General",
            "0.00",
            "#,##0",
            "0%",
            "\"days\" 0",
            "0.00E+00",
            "@",
            "[Red]0.00;[Blue]-0.00",
            "_(* #,##0_)",
            "\"yyyy\"",
        ] {
            assert!(!is_date_format(164, Some(code)), "{code}");
        }
    }
}
