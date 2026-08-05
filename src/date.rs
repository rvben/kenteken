//! Just enough calendar arithmetic to say whether an APK has expired.
//!
//! RDW dates arrive as `YYYYMMDD`, sometimes as a JSON number and sometimes as a
//! string. Whether an inspection is overdue is the single most useful thing this
//! tool can tell a human, and it needs today's date to say it.
//!
//! "Today" is the Dutch calendar day, not the machine's and not UTC. An APK
//! expiry is a date on a Dutch document: it is expired when the Netherlands has
//! moved past it, whichever timezone the question is asked from. Under UTC, a
//! lookup between midnight and 02:00 Amsterdam time would call a certificate
//! that ran out yesterday valid today.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds in a day.
const DAY: i64 = 86_400;

/// Netherlands standard time (CET), in seconds east of UTC.
const CET: i64 = 3_600;

/// Netherlands summer time (CEST), in seconds east of UTC.
const CEST: i64 = 7_200;

/// A calendar date with no time and no zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Parse RDW's `YYYYMMDD` form, rejecting anything that is not a real date.
    ///
    /// RDW uses `0` as a placeholder in some columns (an undemounted object's
    /// `demontagedatum`, for instance). That is an absent date, not the year
    /// zero, and it must come back as `None`.
    pub fn parse_compact(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.len() != 8 || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let year: i32 = raw[0..4].parse().ok()?;
        let month: u32 = raw[4..6].parse().ok()?;
        let day: u32 = raw[6..8].parse().ok()?;
        Self::new(year, month, day)
    }

    /// Build a date, returning `None` for impossible ones such as 31 February.
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date { year, month, day })
    }

    /// ISO 8601, the form used in JSON output.
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Days since 1970-01-01, negative before it.
    ///
    /// Howard Hinnant's `days_from_civil`, which is exact for the proleptic
    /// Gregorian calendar over the whole range this tool can encounter.
    pub fn days_from_epoch(&self) -> i64 {
        let y = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.month as i64;
        let d = self.day as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Whole days from `self` to `other`; negative when `other` is in the past.
    pub fn days_until(&self, other: &Date) -> i64 {
        other.days_from_epoch() - self.days_from_epoch()
    }
}

/// Today's date in the Netherlands.
///
/// Returns `None` if the clock is set before the Unix epoch, in which case the
/// tool reports dates without a relative phrase rather than inventing one.
pub fn today() -> Option<Date> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    dutch_date(i64::try_from(secs).ok()?)
}

/// The Dutch calendar date at a given Unix timestamp.
///
/// Split from [`today`] so the boundary behaviour is testable without a clock.
pub fn dutch_date(unix_secs: i64) -> Option<Date> {
    let local = unix_secs + dutch_utc_offset(unix_secs);
    from_days_since_epoch(local.div_euclid(DAY))
}

/// Seconds east of UTC in the Netherlands at a given instant.
///
/// The EU rule since 1996: summer time runs from 01:00 UTC on the last Sunday of
/// March to 01:00 UTC on the last Sunday of October. Computed rather than looked
/// up, so there is no timezone database to ship or to go stale, and it is exact
/// for every year this tool can be asked about.
fn dutch_utc_offset(unix_secs: i64) -> i64 {
    let Some(date) = from_days_since_epoch(unix_secs.div_euclid(DAY)) else {
        return CET;
    };
    let starts = last_sunday(date.year, 3);
    let ends = last_sunday(date.year, 10);
    // Both transitions happen at the same instant across Europe, 01:00 UTC.
    let start = starts.days_from_epoch() * DAY + 3_600;
    let end = ends.days_from_epoch() * DAY + 3_600;
    if unix_secs >= start && unix_secs < end {
        CEST
    } else {
        CET
    }
}

/// The last Sunday of a month, which is where the EU puts its clock changes.
fn last_sunday(year: i32, month: u32) -> Date {
    let last = Date::new(year, month, days_in_month(year, month))
        .expect("the last day of a month is a real date");
    // 1970-01-01 was a Thursday, so shifting by 4 puts Sunday at 0.
    let weekday = (last.days_from_epoch() + 4).rem_euclid(7);
    from_days_since_epoch(last.days_from_epoch() - weekday)
        .expect("subtracting under a week stays in the same month")
}

/// Inverse of [`Date::days_from_epoch`] (Hinnant's `civil_from_days`).
pub fn from_days_since_epoch(days: i64) -> Option<Date> {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    Date::new(i32::try_from(year).ok()?, m as u32, d as u32)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rdw_compact_dates() {
        assert_eq!(
            Date::parse_compact("20271211"),
            Some(Date {
                year: 2027,
                month: 12,
                day: 11
            })
        );
    }

    #[test]
    fn rejects_rdws_zero_placeholder_rather_than_reading_it_as_a_date() {
        assert_eq!(Date::parse_compact("0"), None);
        assert_eq!(Date::parse_compact("00000000"), None);
    }

    #[test]
    fn rejects_malformed_and_impossible_dates() {
        for raw in [
            "",
            "2027121",
            "202712111",
            "2027-12-11",
            "abcdefgh",
            "20271301",
            "20271200",
            "20270231",
            "20260229",
        ] {
            assert_eq!(Date::parse_compact(raw), None, "input {raw:?}");
        }
    }

    #[test]
    fn accepts_a_real_leap_day() {
        assert!(Date::parse_compact("20240229").is_some());
        assert!(
            Date::parse_compact("20000229").is_some(),
            "2000 is a leap year"
        );
        assert!(Date::parse_compact("19000229").is_none(), "1900 is not");
    }

    #[test]
    fn epoch_day_zero_is_the_epoch() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().days_from_epoch(), 0);
    }

    #[test]
    fn day_arithmetic_matches_known_values() {
        // Cross-checked against `date -j -f %Y-%m-%d ... +%s` / epoch converters.
        let cases = [
            (Date::new(1969, 12, 31).unwrap(), -1),
            (Date::new(1970, 1, 2).unwrap(), 1),
            (Date::new(2000, 1, 1).unwrap(), 10_957),
            (Date::new(2026, 8, 4).unwrap(), 20_669),
            (Date::new(2027, 12, 11).unwrap(), 21_163),
        ];
        for (date, expected) in cases {
            assert_eq!(date.days_from_epoch(), expected, "date {}", date.iso());
        }
    }

    #[test]
    fn round_trips_every_day_across_a_wide_range() {
        // 1900-01-01 through 2100-01-01: a positive control that the two
        // directions agree, which a hand-picked table cannot prove.
        let start = Date::new(1900, 1, 1).unwrap().days_from_epoch();
        let end = Date::new(2100, 1, 1).unwrap().days_from_epoch();
        for days in start..=end {
            let date = from_days_since_epoch(days).expect("every day is a real date");
            assert_eq!(date.days_from_epoch(), days, "round trip at day {days}");
        }
    }

    #[test]
    fn days_until_is_signed_and_symmetric() {
        let a = Date::new(2026, 8, 4).unwrap();
        let b = Date::new(2027, 12, 11).unwrap();
        assert_eq!(a.days_until(&b), 494);
        assert_eq!(b.days_until(&a), -494);
        assert_eq!(a.days_until(&a), 0);
    }

    #[test]
    fn iso_pads_single_digit_components() {
        assert_eq!(Date::new(2026, 1, 5).unwrap().iso(), "2026-01-05");
    }

    #[test]
    fn today_is_a_plausible_date() {
        let today = today().expect("system clock is after the epoch");
        assert!(
            (2020..2200).contains(&today.year),
            "today came out as {}",
            today.iso()
        );
    }

    #[test]
    fn today_is_the_dutch_calendar_day_and_not_the_utc_one() {
        // 22:30 UTC on 3 August is already 00:30 on 4 August in Amsterdam. Under
        // UTC, an APK that ran out on the 3rd would still read as valid "today".
        // Every expected value cross-checked against Python's zoneinfo for
        // Europe/Amsterdam.
        let cases = [
            (
                1_785_796_200,
                "2026-08-04",
                "22:30 UTC in summer is tomorrow in NL",
            ),
            (
                1_785_792_600,
                "2026-08-03",
                "21:30 UTC in summer is still today",
            ),
            (
                1_768_519_800,
                "2026-01-16",
                "23:30 UTC in winter is tomorrow in NL",
            ),
            (
                1_798_759_800,
                "2027-01-01",
                "the year rolls over an hour early",
            ),
            (
                1_782_856_800,
                "2026-07-01",
                "22:00 UTC is midnight in summer",
            ),
        ];
        for (unix, expected, why) in cases {
            assert_eq!(dutch_date(unix).unwrap().iso(), expected, "{why}");
        }
    }

    #[test]
    fn the_dutch_offset_follows_the_eu_clock_changes() {
        // Transitions are at 01:00 UTC on the last Sunday of March and October.
        let cases = [
            (
                1_774_744_200,
                CET,
                "2026-03-29 00:30 UTC, half an hour before",
            ),
            (
                1_774_747_800,
                CEST,
                "2026-03-29 01:30 UTC, half an hour after",
            ),
            (
                1_792_888_200,
                CEST,
                "2026-10-25 00:30 UTC, still summer time",
            ),
            (
                1_792_891_800,
                CET,
                "2026-10-25 01:30 UTC, back to winter time",
            ),
        ];
        for (unix, expected, why) in cases {
            assert_eq!(dutch_utc_offset(unix), expected, "{why}");
        }
    }

    #[test]
    fn the_last_sunday_of_a_month_is_a_sunday_in_that_month() {
        // A positive control over two decades: the transition rule is only right
        // if this lands on a Sunday in the right month every single time.
        for year in 2000..2050 {
            for month in [3, 10] {
                let sunday = last_sunday(year, month);
                assert_eq!(sunday.month, month, "{} left the month", sunday.iso());
                assert_eq!(
                    (sunday.days_from_epoch() + 4).rem_euclid(7),
                    0,
                    "{} is not a Sunday",
                    sunday.iso()
                );
                // And it is the LAST one: seven days later is the next month.
                let next = from_days_since_epoch(sunday.days_from_epoch() + 7).unwrap();
                assert_ne!(next.month, month, "{} is not the last Sunday", sunday.iso());
            }
        }
    }

    #[test]
    fn a_clock_before_the_epoch_does_not_wrap_into_a_future_date() {
        // Negative timestamps must floor, not truncate toward zero: 22:00 UTC on
        // 31 December 1969 is 23:00 the same day in Amsterdam, and integer
        // division toward zero would report it as 1970-01-01.
        assert_eq!(dutch_date(-7_200).unwrap().iso(), "1969-12-31");
    }
}
