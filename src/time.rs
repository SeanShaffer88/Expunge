//! Dates and times without a library: civil-calendar arithmetic (Howard
//! Hinnant's days-from-civil algorithms, exact for every Gregorian date)
//! and local time from Windows, which applies the daylight-saving rules in
//! force on the date being converted.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970-01-01 00:00 UTC.
pub fn unix_now() -> i64 {
    unix_of(SystemTime::now())
}

pub fn unix_of(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// A calendar date (no time zone), counted in days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    days: i64,
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

impl Date {
    /// `None` for a day that doesn't exist (February 30th).
    pub fn from_ymd(y: i32, m: u32, d: u32) -> Option<Date> {
        let y = y as i64;
        if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
            return None;
        }
        Some(Date {
            days: days_from_civil(y, m, d),
        })
    }

    pub fn from_days_since_epoch(days: i64) -> Date {
        Date { days }
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        let (y, m, d) = civil_from_days(self.days);
        (y as i32, m, d)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }
    pub fn month(self) -> u32 {
        self.ymd().1
    }
    pub fn day(self) -> u32 {
        self.ymd().2
    }

    pub fn add_days(self, n: i64) -> Date {
        Date {
            days: self.days + n,
        }
    }

    /// Whole days from `earlier` to `self` (negative if `earlier` is later).
    pub fn days_since(self, earlier: Date) -> i64 {
        self.days - earlier.days
    }

    /// `YYYY-MM-DD` (the first ten characters are read; the rest ignored).
    pub fn parse_ymd(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        Date::from_ymd(
            num(&s[0..4])?,
            num(&s[5..7])? as u32,
            num(&s[8..10])? as u32,
        )
    }

    /// `YYYYMMDD` (the first eight characters).
    pub fn parse_compact(s: &str) -> Option<Date> {
        if s.len() < 8 || !s.is_char_boundary(8) {
            return None;
        }
        Date::from_ymd(num(&s[0..4])?, num(&s[4..6])? as u32, num(&s[6..8])? as u32)
    }

    pub fn today_local() -> Date {
        DateTime::now_local().date
    }
}

fn num(s: &str) -> Option<i32> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// A local date and time to the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl DateTime {
    /// The UTC time for a Unix timestamp.
    pub fn from_unix_utc(ts: i64) -> DateTime {
        let secs = ts.rem_euclid(86_400) as u32;
        DateTime {
            date: Date::from_days_since_epoch(ts.div_euclid(86_400)),
            hour: secs / 3600,
            minute: secs / 60 % 60,
            second: secs % 60,
        }
    }

    /// The local time for a Unix timestamp (UTC off Windows).
    pub fn from_unix_local(ts: i64) -> DateTime {
        #[cfg(windows)]
        {
            if let Some(l) = crate::platform::windows::local_time_of(ts) {
                return l;
            }
        }
        DateTime::from_unix_utc(ts)
    }

    pub fn now_local() -> DateTime {
        DateTime::from_unix_local(unix_now())
    }

    pub fn from_system_time_local(t: SystemTime) -> DateTime {
        DateTime::from_unix_local(unix_of(t))
    }

    /// `2026-10-16`
    pub fn ymd(&self) -> String {
        self.date.to_string()
    }
    /// `2026-10-16 05:07:09`
    pub fn ymd_hms(&self) -> String {
        format!(
            "{} {:02}:{:02}:{:02}",
            self.date, self.hour, self.minute, self.second
        )
    }
    /// `2026-10-16 05:07`
    pub fn ymd_hm(&self) -> String {
        format!("{} {:02}:{:02}", self.date, self.hour, self.minute)
    }
    /// `20261016-050709`, for file names.
    pub fn compact(&self) -> String {
        let (y, m, d) = self.date.ymd();
        format!(
            "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
            self.hour, self.minute, self.second
        )
    }
    /// `05:07:09`
    pub fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trips_across_centuries_and_leap_days() {
        for (y, m, d, days) in [
            (1970, 1, 1, 0),
            (2000, 2, 29, 11_016),
            (1969, 12, 31, -1),
            (1601, 1, 1, -134_774),
            (2026, 10, 16, 20_742),
        ] {
            let date = Date::from_ymd(y, m, d).unwrap();
            assert_eq!(date.days, days, "{y}-{m}-{d}");
            assert_eq!(date.ymd(), (y, m, d));
        }
        // Every day for 400 years maps back to itself.
        let start = Date::from_ymd(1900, 1, 1).unwrap();
        let mut prev = start.ymd();
        for i in 1..146_097 {
            let cur = start.add_days(i).ymd();
            assert!(cur > prev);
            prev = cur;
        }
        assert!(Date::from_ymd(2023, 2, 29).is_none());
        assert!(Date::from_ymd(1900, 2, 29).is_none());
        assert!(Date::from_ymd(2026, 13, 1).is_none());
    }

    #[test]
    fn parsing_and_formatting() {
        let d = Date::parse_ymd("2026-03-09 extra").unwrap();
        assert_eq!(d.to_string(), "2026-03-09");
        assert_eq!(Date::parse_compact("20260309").unwrap(), d);
        for bad in ["2026-3-09", "20260230", "abcd-ef-gh", "", "2026-02-30"] {
            assert!(
                Date::parse_ymd(bad).is_none() && Date::parse_compact(bad).is_none(),
                "{bad}"
            );
        }
        let t = DateTime::from_unix_utc(1_792_134_429); // 2026-10-16 07:07:09 UTC
        assert_eq!(t.ymd_hms(), "2026-10-16 07:07:09");
        assert_eq!(t.compact(), "20261016-070709");
        assert_eq!(t.ymd_hm(), "2026-10-16 07:07");
        assert_eq!(t.hms(), "07:07:09");
        assert_eq!(DateTime::from_unix_utc(-1).ymd_hms(), "1969-12-31 23:59:59");
        assert_eq!(
            Date::from_ymd(2026, 1, 1)
                .unwrap()
                .days_since(Date::from_ymd(2025, 12, 1).unwrap()),
            31
        );
    }
}
