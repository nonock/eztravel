//! Which days and hours the commute is watched.
//!
//! Office days follow a two-week cycle, minus French public holidays and time
//! away. Watch windows are in the user's local time, so the caller passes the
//! current instant and the time zone.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

/// Office days as a two-week cycle.
pub struct OfficeWeeks {
    /// Any day of a week that uses `first_week`. The weeks then alternate from there.
    pub anchor: Date,
    pub first_week: Vec<Weekday>,
    pub second_week: Vec<Weekday>,
}

pub struct Calendar {
    pub weeks: OfficeWeeks,
    /// Whit Monday is a public holiday, but many employers make it a working "solidarity day".
    pub whit_monday_off: bool,
    /// Last day away from the office, included.
    pub away_through: Option<Date>,
}

/// A half-open span of local time: `start` is included, `end` is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: Time,
    pub end: Time,
}

pub struct WatchWindows {
    pub outbound: Window,
    pub inbound: Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Outbound,
    Inbound,
}

impl Calendar {
    pub fn is_office_day(&self, date: Date) -> bool {
        if self.away_through.is_some_and(|last| date <= last)
            || is_public_holiday(date, self.whit_monday_off)
        {
            return false;
        }
        // Count whole weeks from the anchor's Monday. ISO week numbers would not
        // do: a year with 53 weeks puts two odd weeks in a row.
        let anchor = self.weeks.anchor;
        let monday = anchor - i64::from(anchor.weekday().to_monday_zero_offset()).days();
        let weeks = (date - monday).get_days().div_euclid(7);
        let days = if weeks % 2 == 0 {
            &self.weeks.first_week
        } else {
            &self.weeks.second_week
        };
        days.contains(&date.weekday())
    }
}

/// Gregorian Easter Sunday (anonymous algorithm, Meeus/Jones/Butcher).
pub fn easter_sunday(year: i16) -> Date {
    let y = i32::from(year);
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let g = (b - (b + 8) / 25 + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let n = h + l - 7 * m + 114;
    jiff::civil::date(year, (n / 31) as i8, (n % 31 + 1) as i8)
}

/// Public holidays of metropolitan France.
pub fn is_public_holiday(date: Date, whit_monday_off: bool) -> bool {
    const FIXED: [(i8, i8); 8] = [
        (1, 1),
        (5, 1),
        (5, 8),
        (7, 14),
        (8, 15),
        (11, 1),
        (11, 11),
        (12, 25),
    ];
    if FIXED.contains(&(date.month(), date.day())) {
        return true;
    }
    // Easter Monday, Ascension Thursday and Whit Monday.
    match (date - easter_sunday(date.year())).get_days() {
        1 | 39 => true,
        50 => whit_monday_off,
        _ => false,
    }
}

/// The direction being watched at `now`, if any.
pub fn watched_direction(
    now: Timestamp,
    tz: &TimeZone,
    calendar: &Calendar,
    windows: &WatchWindows,
) -> Option<Direction> {
    let local = now.to_zoned(tz.clone());
    if !calendar.is_office_day(local.date()) {
        return None;
    }
    let time = local.time();
    [
        (Direction::Outbound, windows.outbound),
        (Direction::Inbound, windows.inbound),
    ]
    .into_iter()
    .find(|(_, window)| window.start <= time && time < window.end)
    .map(|(direction, _)| direction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::Weekday::{Friday, Monday, Thursday, Tuesday, Wednesday};
    use jiff::civil::{date, time};

    // An example commute, not anyone's real schedule: Tuesday and Thursday one
    // week, Monday, Wednesday and Friday the next.
    fn calendar() -> Calendar {
        Calendar {
            weeks: OfficeWeeks {
                anchor: date(2026, 11, 4),
                first_week: vec![Tuesday, Thursday],
                second_week: vec![Monday, Wednesday, Friday],
            },
            whit_monday_off: true,
            away_through: None,
        }
    }

    fn windows() -> WatchWindows {
        WatchWindows {
            outbound: Window {
                start: time(6, 45, 0, 0),
                end: time(8, 15, 0, 0),
            },
            inbound: Window {
                start: time(16, 0, 0, 0),
                end: time(17, 30, 0, 0),
            },
        }
    }

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    fn at(utc: &str) -> Timestamp {
        utc.parse().unwrap()
    }

    #[test]
    fn office_days_alternate_between_the_two_weeks() {
        let calendar = calendar();
        // Week of Monday 2 November 2026: first week.
        assert!(!calendar.is_office_day(date(2026, 11, 2)));
        assert!(calendar.is_office_day(date(2026, 11, 3)));
        assert!(calendar.is_office_day(date(2026, 11, 5)));
        assert!(!calendar.is_office_day(date(2026, 11, 6)));
        // Week of Monday 9 November: second week (Wednesday 11 November is a holiday).
        assert!(calendar.is_office_day(date(2026, 11, 9)));
        assert!(!calendar.is_office_day(date(2026, 11, 10)));
        assert!(calendar.is_office_day(date(2026, 11, 13)));
        // Week of Monday 16 November: first week again.
        assert!(calendar.is_office_day(date(2026, 11, 17)));
        assert!(!calendar.is_office_day(date(2026, 11, 16)));
    }

    #[test]
    fn weeks_before_the_anchor_alternate_too() {
        let calendar = calendar();
        // Week of 26 October: second week. Week of 19 October: first week.
        assert!(calendar.is_office_day(date(2026, 10, 26)));
        assert!(!calendar.is_office_day(date(2026, 10, 27)));
        assert!(calendar.is_office_day(date(2026, 10, 20)));
        assert!(!calendar.is_office_day(date(2026, 10, 19)));
    }

    #[test]
    fn alternation_ignores_iso_week_numbers() {
        // 2026 has 53 ISO weeks: weeks 53 and 1 are both odd, but must still alternate.
        let calendar = Calendar {
            whit_monday_off: true,
            away_through: None,
            weeks: OfficeWeeks {
                anchor: date(2026, 12, 28),
                ..calendar().weeks
            },
        };
        assert!(calendar.is_office_day(date(2026, 12, 29)));
        assert!(!calendar.is_office_day(date(2027, 1, 5)));
        assert!(calendar.is_office_day(date(2027, 1, 6)));
        assert!(calendar.is_office_day(date(2027, 1, 12)));
    }

    #[test]
    fn no_office_on_a_public_holiday() {
        // Wednesday 11 November 2026 falls in a second week.
        assert!(!calendar().is_office_day(date(2026, 11, 11)));
    }

    #[test]
    fn no_office_while_away_last_day_included() {
        let calendar = Calendar {
            away_through: Some(date(2026, 11, 9)),
            ..calendar()
        };
        assert!(!calendar.is_office_day(date(2026, 11, 3)));
        assert!(!calendar.is_office_day(date(2026, 11, 9)));
        assert!(calendar.is_office_day(date(2026, 11, 13)));
    }

    #[test]
    fn easter_sunday_matches_published_dates() {
        let published = [
            (2024, date(2024, 3, 31)),
            (2025, date(2025, 4, 20)),
            (2026, date(2026, 4, 5)),
            (2027, date(2027, 3, 28)),
            (2028, date(2028, 4, 16)),
            (2029, date(2029, 4, 1)),
            (2030, date(2030, 4, 21)),
            (2035, date(2035, 3, 25)),
            (2038, date(2038, 4, 25)),
        ];
        for (year, expected) in published {
            assert_eq!(easter_sunday(year), expected, "{year}");
        }
    }

    #[test]
    fn french_public_holidays_of_2026() {
        // Official list for 2026.
        let holidays = [
            date(2026, 1, 1),
            date(2026, 4, 6),
            date(2026, 5, 1),
            date(2026, 5, 8),
            date(2026, 5, 14),
            date(2026, 5, 25),
            date(2026, 7, 14),
            date(2026, 8, 15),
            date(2026, 11, 1),
            date(2026, 11, 11),
            date(2026, 12, 25),
        ];
        for holiday in holidays {
            assert!(is_public_holiday(holiday, true), "{holiday}");
        }
        let mut count = 0;
        let mut day = date(2026, 1, 1);
        while day.year() == 2026 {
            count += usize::from(is_public_holiday(day, true));
            day = day.tomorrow().unwrap();
        }
        assert_eq!(count, holidays.len());
    }

    #[test]
    fn whit_monday_can_be_a_working_day() {
        assert!(is_public_holiday(date(2027, 5, 17), true));
        assert!(!is_public_holiday(date(2027, 5, 17), false));
        // Easter Monday and Ascension stay holidays.
        assert!(is_public_holiday(date(2027, 3, 29), false));
        assert!(is_public_holiday(date(2027, 5, 6), false));
    }

    #[test]
    fn watched_direction_follows_local_time() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Tuesday 3 November 2026, Paris is UTC+1.
        assert_eq!(
            watched_direction(at("2026-11-03T05:44:59Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-11-03T05:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T07:14:59Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T07:15:00Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-11-03T15:00:00Z"), &tz, &calendar, &windows),
            Some(Direction::Inbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T16:30:00Z"), &tz, &calendar, &windows),
            None
        );
    }

    #[test]
    fn watched_direction_across_the_change_to_winter_time() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Tuesday 20 October 2026, before 25 October: Paris is UTC+2.
        assert_eq!(
            watched_direction(at("2026-10-20T04:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        // Monday 26 October, after the change: UTC+1.
        assert_eq!(
            watched_direction(at("2026-10-26T04:45:00Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-10-26T05:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
    }

    #[test]
    fn nothing_is_watched_on_a_day_off() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Monday 2 November 2026 is not an office day in a first week.
        assert_eq!(
            watched_direction(at("2026-11-02T06:00:00Z"), &tz, &calendar, &windows),
            None
        );
    }
}
